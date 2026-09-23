//! Folder-sync orchestrator.
//!
//! Walks user-configured folders, extracts text from supported formats,
//! and pushes chunks to the embedding service. Skipped files (scanned
//! PDFs, image-only documents, unsupported formats) are tracked so the
//! UI can surface them, but they don't fail the whole scan.

#[cfg(feature = "rag")]
pub mod scanner;

#[cfg(feature = "rag")]
pub use scanner::{ScanProgress, ScanReport, ScanStatus, scan_folder};

/// Fixed list of file extensions the sync pipeline considers
/// "text-bearing". Anything else is recorded as `skipped` with reason
/// "format not supported". Per user requirement, scanned PDFs are
/// detected at extraction time and also marked skipped.
///
/// `rtf` uses the `striprtf` crate (pure Rust, ~500 LOC) — fonts,
/// pictures and field codes are dropped, paragraph breaks survive,
/// which is exactly what the chunker needs.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "rtf", "xlsx", "xls", "xlsb", "xlsm", "ods", "csv", "txt", "md",
    // Legacy Office, readable since the onboarding funnel: a synced
    // folder of old practice files is exactly where these live.
    "doc", "ppt", "pps", "pot",
];

/// Returns true if the given filename's extension is in the
/// supported-text set. Case-insensitive. Used by the scanner to
/// short-circuit before reading the file.
pub fn extension_is_supported(filename: &str) -> bool {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    SUPPORTED_EXTENSIONS.iter().any(|s| *s == ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_recognized() {
        for ext in ["pdf", "docx", "rtf", "xlsx", "xls", "xlsb", "ods", "csv", "txt", "md"] {
            assert!(extension_is_supported(&format!("doc.{ext}")), "should accept .{ext}");
        }
    }

    #[test]
    fn extension_check_is_case_insensitive() {
        assert!(extension_is_supported("doc.PDF"));
        assert!(extension_is_supported("doc.Docx"));
        assert!(extension_is_supported("doc.MD"));
    }

    #[test]
    fn unsupported_extensions_rejected() {
        for name in ["a.exe", "image.png", "video.mp4", "noext", "archive.zip"] {
            assert!(!extension_is_supported(name), "should reject {name}");
        }
    }

    #[test]
    fn dotfile_with_no_extension_rejected() {
        // ".gitignore" → extension is "gitignore" per std::path semantics,
        // which isn't in the supported list. Still rejected.
        assert!(!extension_is_supported(".gitignore"));
    }

    #[test]
    fn full_path_works() {
        assert!(extension_is_supported("/some/long/path/contract.docx"));
        assert!(extension_is_supported("c:\\users\\me\\notes.txt"));
    }
}
