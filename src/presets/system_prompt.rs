// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.

//! Domain-aware system-prompt **prologue**. Read once at chat-turn
//! time from `config/system-prompts/<locale>/<domain>.md` and
//! prepended to the base instructions (`config/system-prompts/base.md`,
//! see [`base_instructions`]) so the
//! assistant boots with a professional-vertical persona before the
//! generic tool-use / citation rules kick in.
//!
//! Resolution fall-back chain (first hit wins):
//!
//!   1. requested locale + requested domain
//!   2. `"it"` + requested domain (`it` is the primary curated
//!      locale — Cove Studio's first-class users are Italian)
//!   3. `"en"` + requested domain (every domain ships an English
//!      version, so this is the last-stand language fallback)
//!   4. `None` — the caller composes a prologue without a domain body
//!
//! The directory hosting the files is found the same way the other
//! `crate::presets::*` registries find theirs: env-var override
//! (`COVE_SYSTEM_PROMPTS_DIR`), CWD ancestor walk, then exe-dir
//! ancestor walk — so dev (cwd = workspace root) and installed-MSI
//! (cwd = anywhere, exe = `<install>/`) both land on the bundled
//! files without configuration.
//!
//! Country / jurisdiction handling is **prompt-time text**: there is
//! no `country` column on `user_settings` or `projects`. The
//! composer hard-codes a locale → default-country mapping (Italian →
//! Italy, French → France, …) and instructs the model to ASK the
//! user whenever the conversation suggests a different jurisdiction.
//! Adding a database-backed override is cheap if the present approach
//! turns out to be too coarse for power users.

use std::path::{Path, PathBuf};

const FALLBACK_LOCALES: &[&str] = &["it", "en"];

/// Locate the `config/system-prompts/` root directory. Mirrors the
/// `presets_dir` / `config_subdir` pattern in `crate::presets` so the
/// installed-MSI layout (`<install>/config/system-prompts/`) and the
/// dev workspace layout (`<repo>/config/system-prompts/`) both work
/// without an env-var override.
fn root_dir() -> PathBuf {
    if let Some(dir) = crate::product::env_var("SYSTEM_PROMPTS_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_for_root(&cwd) {
            return found;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = walk_for_root(&exe) {
            return found;
        }
    }
    PathBuf::from("./config/system-prompts")
}

fn walk_for_root(start: &Path) -> Option<PathBuf> {
    for anc in start.ancestors() {
        let candidate = anc.join("config").join("system-prompts");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Load the `.md` body for `(locale, domain)` walking the fallback
/// chain. Returns `None` when neither the requested pair nor any
/// fallback (`it/<domain>`, `en/<domain>`) exists.
///
/// The returned string is the raw file content trimmed of trailing
/// whitespace — the caller composes it into a wrapper section so the
/// stored `.md` can stay readable as a stand-alone document.
pub fn resolve(locale: &str, domain: &str) -> Option<String> {
    let root = root_dir();
    let domain_safe = sanitize_segment(domain)?;
    // Locale that fails sanitize_segment carries path-traversal or
    // other shell-metacharacter intent — hard reject rather than
    // silently substituting a fallback locale, otherwise a request
    // for `resolve("../etc", "medical")` would happily serve
    // `it/medical.md` (the fallback chain finds it) which masks the
    // attack. Sanitize-passing-but-unknown locales (e.g. "ja") still
    // walk the fallback chain normally — that path is benign.
    let locale_safe = sanitize_segment(locale)?;
    // 1. Requested locale.
    if let Some(text) = try_read(&root, &locale_safe, &domain_safe) {
        return Some(text);
    }
    // 2-3. Fallback locales in order.
    for fb in FALLBACK_LOCALES {
        if *fb == locale_safe {
            continue;
        }
        if let Some(text) = try_read(&root, fb, &domain_safe) {
            return Some(text);
        }
    }
    None
}

/// Base assistant instructions, stored in `config/system-prompts/base.md`.
/// The same file is embedded at build time as the fallback, so the
/// Markdown stays the single source of the text.
const EMBEDDED_BASE_INSTRUCTIONS: &str = include_str!("../../config/system-prompts/base.md");

/// File name of the base instructions inside the system-prompts root.
const BASE_INSTRUCTIONS_FILE: &str = "base.md";

/// Structural tokens the citation parser and the chat normalisers rely
/// on. An edited `base.md` missing any of them is rejected in favour of
/// the embedded copy, so a prompt edit can't silently break citations,
/// workflows or DOCX templates.
pub const REQUIRED_BASE_TOKENS: &[&str] = &[
    "[c1]",
    "<CITATIONS>",
    "</CITATIONS>",
    "[[PAGE_BREAK]]",
    "doc-N",
    "[Workflow: <titolo> (id: <id>)]",
    "[Template: <titolo> (id: <id>)]",
    "read_workflow",
    "describe_docx_template",
    "generate_docx",
    "edit_document",
    "generate_xlsx",
];

/// Base instructions for every chat turn: `base.md` from the
/// system-prompts root when present and valid, otherwise the copy
/// embedded at build time. Read on each call so an edited file takes
/// effect on the next message without rebuilding.
pub fn base_instructions() -> String {
    let path = root_dir().join(BASE_INSTRUCTIONS_FILE);
    let on_disk = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!("[system-prompts] failed to read {}: {e}", path.display());
            None
        }
    };
    select_base_instructions(on_disk)
}

fn select_base_instructions(on_disk: Option<String>) -> String {
    if let Some(text) = on_disk {
        let text = text.trim();
        let missing: Vec<&str> = REQUIRED_BASE_TOKENS
            .iter()
            .copied()
            .filter(|token| !text.contains(token))
            .collect();
        if !text.is_empty() && missing.is_empty() {
            return text.to_string();
        }
        tracing::warn!(
            "[system-prompts] {BASE_INSTRUCTIONS_FILE} ignored (empty or missing tokens {missing:?}); using the built-in copy"
        );
    }
    EMBEDDED_BASE_INSTRUCTIONS.trim().to_string()
}

/// Map a UI / chat locale to its conventional default country, in
/// Italian like the rest of the prologue (`Paese predefinito: Italia`),
/// so the model knows what jurisdiction to assume absent explicit
/// signals. The English mapping is intentionally vague because the en
/// locale legitimately spans US / UK / IE / AU / CA / NZ / IN.
pub fn default_country_for_locale(locale: &str) -> &'static str {
    match locale {
        "it" => "Italia",
        "fr" => "Francia",
        "de" => "Germania",
        "es" => "Spagna",
        "pt" => "Portogallo",
        _ => "non specificato (chiedilo all'utente)",
    }
}

/// Italian name of the working language, for the prologue.
fn italian_language_name_for_locale(locale: &str) -> &'static str {
    match locale {
        "it" => "italiano",
        "fr" => "francese",
        "de" => "tedesco",
        "es" => "spagnolo",
        "pt" => "portoghese",
        _ => "inglese",
    }
}

/// Human-facing language name surfaced in the prologue. Mirrors the
/// frontend's locale dropdown.
pub fn language_name_for_locale(locale: &str) -> &'static str {
    match locale {
        "it" => "Italian",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        _ => "English",
    }
}

/// Assemble the full prologue section that gets prepended to
/// the base chat instructions. Wraps the per-domain `.md` body in a
/// metadata header (Domain / Working language / Default country) and
/// a country-disambiguation reminder. Returns an empty string when
/// nothing meaningful can be assembled (no `.md` found AND no domain
/// known) — the caller then skips the section entirely.
pub fn assemble_prologue(locale: &str, domain: &str) -> String {
    let body = resolve(locale, domain).unwrap_or_default();
    let lang = italian_language_name_for_locale(locale);
    let country = default_country_for_locale(locale);
    let mut out = String::new();
    out.push_str(
        "=== Contesto del dominio (leggilo per primo: definisce il tuo ruolo in questa chat) ===\n",
    );
    out.push_str(&format!("Dominio: {domain}\n"));
    out.push_str(&format!(
        "Lingua di lavoro: {lang} (predefinita solo se la lingua dell'utente non è chiara: \
         la risposta segue sempre la lingua dell'ultimo messaggio dell'utente)\n"
    ));
    out.push_str(&format!("Paese / giurisdizione predefiniti: {country}\n\n"));
    if body.is_empty() {
        out.push_str(
            "Non sono disponibili indicazioni specifiche per questo dominio: comportati da \
             assistente professionale generico del settore, cita le fonti quando pertinente e \
             lascia all'utente le scelte che dipendono dalla giurisdizione.\n",
        );
    } else {
        out.push_str(&body);
        out.push('\n');
    }
    out.push_str(
        "\nGiurisdizione: se la richiesta riguarda un paese, una normativa o un quadro \
         legale, medico o professionale diverso da quello predefinito indicato sopra, \
         CHIEDI all'utente quale paese o giurisdizione si applica PRIMA di dare indicazioni \
         che ne dipendono. Non dare nulla per scontato.\n",
    );
    out
}

fn try_read(root: &Path, locale: &str, domain: &str) -> Option<String> {
    let path = root.join(locale).join(format!("{domain}.md"));
    if !path.is_file() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        Err(e) => {
            tracing::warn!(
                "[system-prompts] failed to read {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Guard against `..` / absolute path injection through user-supplied
/// locale or domain. Accepts only `[a-zA-Z0-9_-]+`; anything else
/// returns `None` and the caller treats the file as missing.
fn sanitize_segment(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(t.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// The fixture points the resolver at a temp tree through a
    /// process-wide env var, so two fixtures cannot be installed at the
    /// same time. `cargo test` runs tests in parallel threads, so each
    /// test holds this lock for as long as its tree must stay in place.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Temp tree + the lock that keeps it the only one installed. Kept
    /// alive by the `let _tmp = …` binding in each test.
    struct Fixture {
        _tmp: TempDir,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    fn make_tree(files: &[(&str, &str, &str)]) -> Fixture {
        // A panicking test poisons the lock; the env var is rewritten
        // below anyway, so the poison carries no stale state.
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        for (locale, domain, body) in files {
            let dir = tmp.path().join(locale);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(format!("{domain}.md")), body).unwrap();
        }
        // SAFETY: `ENV_LOCK` makes this the only fixture writing the
        // variable, and no other thread in this binary reads it.
        unsafe {
            std::env::set_var(crate::product::env_var_name("SYSTEM_PROMPTS_DIR"), tmp.path());
        }
        Fixture { _tmp: tmp, _guard: guard }
    }

    #[test]
    fn resolve_uses_requested_locale_when_present() {
        let _tmp = make_tree(&[
            ("it", "medical", "ITALIANO"),
            ("en", "medical", "ENGLISH"),
        ]);
        assert_eq!(resolve("it", "medical").as_deref(), Some("ITALIANO"));
    }

    #[test]
    fn resolve_falls_back_to_italian_when_locale_missing() {
        let _tmp = make_tree(&[("it", "medical", "ITALIANO")]);
        assert_eq!(resolve("fr", "medical").as_deref(), Some("ITALIANO"));
    }

    #[test]
    fn resolve_falls_back_to_english_when_italian_missing() {
        let _tmp = make_tree(&[("en", "medical", "ENGLISH")]);
        assert_eq!(resolve("de", "medical").as_deref(), Some("ENGLISH"));
    }

    #[test]
    fn resolve_returns_none_when_domain_missing_everywhere() {
        let _tmp = make_tree(&[("it", "medical", "ITALIANO")]);
        assert!(resolve("it", "finance").is_none());
    }

    #[test]
    fn resolve_rejects_path_traversal() {
        let _tmp = make_tree(&[("it", "medical", "ITALIANO")]);
        assert!(resolve("../etc", "medical").is_none());
        assert!(resolve("it", "../passwd").is_none());
    }

    #[test]
    fn assemble_prologue_wraps_body() {
        let _tmp = make_tree(&[("it", "medical", "BODY")]);
        let p = assemble_prologue("it", "medical");
        assert!(p.contains("Dominio: medical"));
        assert!(p.contains("Lingua di lavoro: italiano"));
        assert!(p.contains("Paese / giurisdizione predefiniti: Italia"));
        assert!(p.contains("BODY"));
        assert!(p.contains("Giurisdizione:"));
    }

    #[test]
    fn assemble_prologue_falls_back_when_md_missing() {
        let _tmp = make_tree(&[("it", "medical", "BODY")]);
        let p = assemble_prologue("it", "ip");
        assert!(p.contains("Dominio: ip"));
        assert!(p.contains("Non sono disponibili indicazioni specifiche"));
        assert!(p.contains("Giurisdizione:"));
    }

    #[test]
    fn default_country_for_locale_known_locales() {
        assert_eq!(default_country_for_locale("it"), "Italia");
        assert_eq!(default_country_for_locale("fr"), "Francia");
        assert_eq!(default_country_for_locale("de"), "Germania");
        assert_eq!(default_country_for_locale("es"), "Spagna");
        assert_eq!(default_country_for_locale("pt"), "Portogallo");
        assert!(default_country_for_locale("en").contains("chiedilo"));
    }

    #[test]
    fn embedded_base_instructions_carry_every_required_token() {
        for token in REQUIRED_BASE_TOKENS {
            assert!(EMBEDDED_BASE_INSTRUCTIONS.contains(token), "token mancante: {token}");
        }
    }

    #[test]
    fn embedded_base_instructions_put_the_reply_language_rule_first() {
        let rule = EMBEDDED_BASE_INSTRUCTIONS.find("# 0. Lingua della risposta").unwrap();
        let next = EMBEDDED_BASE_INSTRUCTIONS.find("# 1.").unwrap();
        assert!(rule < next);
        assert!(EMBEDDED_BASE_INSTRUCTIONS.contains("Se scrive in inglese rispondi in inglese"));
        assert!(!EMBEDDED_BASE_INSTRUCTIONS.contains("Mike,"));
    }

    #[test]
    fn valid_file_on_disk_wins_over_embedded_copy() {
        let edited = format!("{}\n\nRegola aggiuntiva di prova.", EMBEDDED_BASE_INSTRUCTIONS);
        let chosen = select_base_instructions(Some(edited));
        assert!(chosen.ends_with("Regola aggiuntiva di prova."));
    }

    #[test]
    fn broken_or_missing_file_falls_back_to_embedded_copy() {
        let embedded = EMBEDDED_BASE_INSTRUCTIONS.trim();
        assert_eq!(select_base_instructions(None), embedded);
        assert_eq!(select_base_instructions(Some("   ".into())), embedded);
        let without_citations = EMBEDDED_BASE_INSTRUCTIONS.replace("<CITATIONS>", "<FONTI>");
        assert_eq!(select_base_instructions(Some(without_citations)), embedded);
    }
}
