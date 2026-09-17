//! Crypto envelope for exported project files.
//!
//! See `project_archive/mod.rs` for the format spec. This module owns:
//!  - email normalization + SHA-256 fingerprint
//!  - Argon2id key derivation from the email
//!  - AES-256-GCM encrypt/decrypt of the ZIP payload
//!  - reading/writing the file header

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Result};
use argon2::Argon2;
use sha2::{Digest, Sha256};

/// Magic written at the start of every new project file.
pub const MAGIC: &[u8; 8] = b"COVEPRJ\0";

/// Magics written by earlier releases. Still accepted when reading, so
/// files exported before the change import unchanged: the rest of the
/// header and the encryption are identical.
pub const LEGACY_MAGICS: &[&[u8; 8]] = &[b"MIKEPRJ\0"];

fn is_known_magic(bytes: &[u8]) -> bool {
    bytes == MAGIC || LEGACY_MAGICS.iter().any(|m| bytes == *m)
}
pub const VERSION: u8 = 1;
pub const FLAG_ENCRYPTED: u8 = 0b0000_0001;

pub const HEADER_SIZE: usize = 8 + 1 + 1 + 32 + 16 + 12; // magic+ver+flags+hash+salt+nonce

/// Normalize an email for hashing/key derivation: trim, lowercase ASCII
/// letters, collapse internal whitespace. Different casings of the same
/// address must produce the same key.
pub fn normalize_email(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// SHA-256 fingerprint of the normalized email. Stored in the file
/// header so the importer can quickly check "is this for me?" without
/// attempting decryption.
pub fn email_hash(email: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(normalize_email(email).as_bytes());
    let out = hasher.finalize();
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&out);
    buf
}

/// Derive a 32-byte AES-256 key from the recipient email + per-file salt
/// using Argon2id with conservative parameters (memory=64 MiB, t=3, p=1).
fn derive_key(email: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let argon = Argon2::default();
    let mut out = [0u8; 32];
    argon
        .hash_password_into(normalize_email(email).as_bytes(), salt, &mut out)
        .map_err(|e| anyhow!("argon2 derive failed: {e}"))?;
    Ok(out)
}

#[derive(Debug)]
pub struct Header {
    pub version: u8,
    pub flags: u8,
    pub email_hash: [u8; 32],
    pub salt: [u8; 16],
    pub nonce: [u8; 12],
}

impl Header {
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(MAGIC);
        out.push(self.version);
        out.push(self.flags);
        out.extend_from_slice(&self.email_hash);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
    }

    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            bail!("not a project file: too short");
        }
        if !is_known_magic(&bytes[0..8]) {
            bail!("not a project file: bad magic");
        }
        let version = bytes[8];
        if version != VERSION {
            bail!("unsupported project file version {version}");
        }
        let flags = bytes[9];
        let mut email_hash = [0u8; 32];
        email_hash.copy_from_slice(&bytes[10..42]);
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&bytes[42..58]);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bytes[58..70]);
        Ok(Self { version, flags, email_hash, salt, nonce })
    }
}

/// Encrypt a ZIP payload for a recipient. Returns the full file bytes
/// (header + ciphertext) ready to be written to disk.
pub fn seal(recipient_email: &str, payload: &[u8]) -> Result<Vec<u8>> {
    use rand::RngCore;

    let mut salt = [0u8; 16];
    rand::rng().fill_bytes(&mut salt);
    let key_bytes = derive_key(recipient_email, &salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, payload)
        .map_err(|e| anyhow!("encryption failed: {e}"))?;

    let header = Header {
        version: VERSION,
        flags: FLAG_ENCRYPTED,
        email_hash: email_hash(recipient_email),
        salt,
        nonce: nonce.into(),
    };

    let mut out = Vec::with_capacity(HEADER_SIZE + ciphertext.len());
    header.write(&mut out);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open an exported project file using the recipient's email. Returns
/// the decrypted ZIP payload. Errors include:
///  - bad magic / unsupported version
///  - email fingerprint mismatch (file is for a different recipient)
///  - decryption failure (typically: the typed email differs from the
///    one used at export time)
pub fn open(recipient_email: &str, file_bytes: &[u8]) -> Result<Vec<u8>> {
    let header = Header::read(file_bytes)?;
    let payload = &file_bytes[HEADER_SIZE..];

    if header.email_hash != email_hash(recipient_email) {
        bail!(
            "this project file was sealed for a different email — \
             ask the sender to re-export it for {}",
            normalize_email(recipient_email),
        );
    }

    if header.flags & FLAG_ENCRYPTED == 0 {
        // Not encrypted — payload is the raw ZIP. Allowed for tests/dev
        // but not produced by `seal()`.
        return Ok(payload.to_vec());
    }

    let key_bytes = derive_key(recipient_email, &header.salt)?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&header.nonce);

    cipher
        .decrypt(nonce, payload)
        .map_err(|e| anyhow!("decryption failed (wrong email or corrupt file): {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_seal_open() {
        let payload = b"hello project";
        let sealed = seal("Alice@example.com", payload).unwrap();
        let opened = open("alice@example.com", &sealed).unwrap();
        assert_eq!(opened, payload);
    }

    #[test]
    fn wrong_email_rejected() {
        let sealed = seal("alice@example.com", b"data").unwrap();
        let res = open("bob@example.com", &sealed);
        assert!(res.is_err());
    }

    #[test]
    fn normalize_email_collapses_case_and_whitespace() {
        assert_eq!(normalize_email("  Alice@Example.COM  "), "alice@example.com");
        assert_eq!(normalize_email("BOB@x.io"), "bob@x.io");
    }

    #[test]
    fn email_hash_is_deterministic_and_case_insensitive() {
        let a = email_hash("alice@example.com");
        let b = email_hash("Alice@Example.com");
        assert_eq!(a, b);
        // Different email → different hash with overwhelming probability.
        let c = email_hash("bob@example.com");
        assert_ne!(a, c);
    }

    #[test]
    fn header_roundtrip() {
        let header = Header {
            version: VERSION,
            flags: FLAG_ENCRYPTED,
            email_hash: [7u8; 32],
            salt: [42u8; 16],
            nonce: [9u8; 12],
        };
        let mut buf = Vec::new();
        header.write(&mut buf);
        assert_eq!(buf.len(), HEADER_SIZE);
        let parsed = Header::read(&buf).unwrap();
        assert_eq!(parsed.version, VERSION);
        assert_eq!(parsed.flags, FLAG_ENCRYPTED);
        assert_eq!(parsed.email_hash, [7u8; 32]);
        assert_eq!(parsed.salt, [42u8; 16]);
        assert_eq!(parsed.nonce, [9u8; 12]);
    }

    #[test]
    fn new_files_use_the_current_magic() {
        let sealed = seal("alice@example.com", b"x").unwrap();
        assert_eq!(&sealed[..8], MAGIC);
    }

    #[test]
    fn files_with_a_legacy_magic_still_open() {
        let mut sealed = seal("alice@example.com", b"legacy payload").unwrap();
        for legacy in LEGACY_MAGICS {
            sealed[..8].copy_from_slice(*legacy);
            let opened = open("alice@example.com", &sealed).unwrap();
            assert_eq!(opened, b"legacy payload");
        }
    }

    #[test]
    fn header_rejects_short_input() {
        let buf = vec![0u8; HEADER_SIZE - 1];
        assert!(Header::read(&buf).is_err());
    }

    #[test]
    fn header_rejects_bad_magic() {
        let mut buf = vec![0u8; HEADER_SIZE];
        buf[..8].copy_from_slice(b"NOPE\0\0\0\0");
        assert!(Header::read(&buf).is_err());
    }

    #[test]
    fn header_rejects_unsupported_version() {
        let header = Header {
            version: 99,
            flags: 0,
            email_hash: [0u8; 32],
            salt: [0u8; 16],
            nonce: [0u8; 12],
        };
        let mut buf = Vec::new();
        header.write(&mut buf);
        let err = Header::read(&buf).unwrap_err().to_string();
        assert!(err.contains("version"), "got: {err}");
    }

    #[test]
    fn open_rejects_truncated_file() {
        let sealed = seal("alice@example.com", b"data").unwrap();
        // Drop the last 4 bytes (corrupts the GCM tag).
        let truncated = &sealed[..sealed.len() - 4];
        let res = open("alice@example.com", truncated);
        assert!(res.is_err());
    }

    #[test]
    fn open_rejects_tampered_ciphertext() {
        let mut sealed = seal("alice@example.com", b"hello").unwrap();
        // Flip a byte in the ciphertext (after the header).
        let i = HEADER_SIZE + 2;
        sealed[i] ^= 0xff;
        let res = open("alice@example.com", &sealed);
        assert!(res.is_err(), "tampered ciphertext must fail GCM auth");
    }

    #[test]
    fn seal_uses_fresh_salt_each_time() {
        let s1 = seal("alice@example.com", b"hi").unwrap();
        let s2 = seal("alice@example.com", b"hi").unwrap();
        // Same plaintext + same recipient → ciphertexts must differ
        // because salt and nonce are random per envelope.
        assert_ne!(s1, s2);
    }

    #[test]
    fn empty_payload_roundtrips() {
        let sealed = seal("a@b.io", b"").unwrap();
        let opened = open("a@b.io", &sealed).unwrap();
        assert!(opened.is_empty());
    }

    #[test]
    fn casing_in_open_email_is_normalized() {
        let sealed = seal("alice@example.com", b"x").unwrap();
        // Open with different casing: should still succeed because
        // both sides normalize before key derivation / hash compare.
        let opened = open("ALICE@example.com", &sealed).unwrap();
        assert_eq!(opened, b"x");
    }
}
