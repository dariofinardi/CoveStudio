//! `.coveprj` manifest schema (v1).
//!
//! All structs are versioned via the top-level `schema_version`. Adding
//! optional fields to existing structs is backward-compatible; renaming or
//! removing fields requires bumping `schema_version` and handling both
//! shapes in the importer.
//!
//! v0.5.4 widened several records with optional fields that the v0.5.4
//! pre-amendment exporter didn't carry. Older `.coveprj` archives are
//! still readable — every new field is `Option<…>` with
//! `#[serde(default)]` so missing keys deserialize as `None`.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Top-level manifest, written as `manifest.json` inside the ZIP.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// Free-form, e.g. "Cove Studio 0.1.0".
    pub exporter: String,
    /// ISO-8601 UTC timestamp.
    pub exported_at: String,
    /// Display name of the user who exported, for provenance only — the
    /// importer never trusts this for authorization decisions.
    pub exported_by_display_name: Option<String>,
    /// What the importer should expect to find in the archive.
    pub contents: ManifestContents,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ManifestContents {
    pub project: bool,
    pub document_count: u32,
    pub tabular_review_count: u32,
    pub workflow_count: u32,
    pub chat_count: u32,
    /// Whether chat history was opted into at export.
    pub includes_chats: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub cm_number: Option<String>,
    pub created_at: String,
    /// Email of the original creator. Used only for display ("imported
    /// from alice@…"); the importer creates a fresh project owned by the
    /// current user.
    pub original_creator_email: Option<String>,
    /// Professional-domain column (migration 0018). One of the 11
    /// canonical ids (`legal`, `medical`, …). Missing in older
    /// archives → importer falls back to the schema default
    /// (`'legal'`). Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// `'shared'` or `'strict'` — controls the project's
    /// retrieval-scope dropdown. Missing → defaults to `'shared'`.
    /// Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation_mode: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentRecord {
    pub id: String,
    pub filename: String,
    pub file_type: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub sha256: String,
    pub created_at: String,
    /// Document domain (migration 0018). Falls back to the parent
    /// project's domain when None. Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Original `project_folder_id` (migration 0026 sub-folder tree).
    /// The importer cannot blindly reuse the id — folders have their
    /// own per-project UUIDs — so this field travels purely as a
    /// **path hint** by carrying the original id; the importer
    /// remaps it through `folder_id_remap` after rebuilding the
    /// folder tree (or drops the field if the original tree wasn't
    /// reconstructable). Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_folder_id: Option<String>,
    /// Accept / reject decision on a generated document (migration
    /// 0029). `'accepted'` or `'rejected'`. Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// User-supplied motive for a rejection. Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    /// LLM-generated short summary of a rejected document, recorded
    /// at the moment of rejection so subsequent turns can reason
    /// about the dismissed file without re-loading its bytes. Added
    /// in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TabularReviewRecord {
    pub id: String,
    pub title: Option<String>,
    pub columns_config: serde_json::Value,
    /// Document ids in the original archive — re-mapped at import time.
    pub document_ids: Vec<String>,
    pub created_at: String,
    /// Review-level domain. Falls back to the parent project's
    /// domain when None. Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub id: String,
    pub title: String,
    pub r#type: String,
    pub prompt_md: Option<String>,
    pub columns_config: Option<serde_json::Value>,
    pub practice: Option<String>,
    /// Workflow domain (migration 0018). Missing in older archives
    /// → importer falls back to the schema default. Added in v0.5.4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatRecord {
    pub id: String,
    pub title: Option<String>,
    pub created_at: String,
    pub messages: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manifest_roundtrips_through_json() {
        let m = Manifest {
            schema_version: SCHEMA_VERSION,
            exporter: "Cove Studio 0.1.0".into(),
            exported_at: "2026-05-06T10:30:00Z".into(),
            exported_by_display_name: Some("Dario".into()),
            contents: ManifestContents {
                project: true,
                document_count: 3,
                tabular_review_count: 1,
                workflow_count: 2,
                chat_count: 0,
                includes_chats: false,
            },
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.schema_version, SCHEMA_VERSION);
        assert_eq!(back.exporter, "Cove Studio 0.1.0");
        assert_eq!(back.contents.document_count, 3);
        assert_eq!(back.contents.includes_chats, false);
        assert_eq!(back.exported_by_display_name.as_deref(), Some("Dario"));
    }

    #[test]
    fn manifest_contents_default_is_zeroes() {
        let c = ManifestContents::default();
        assert!(!c.project);
        assert_eq!(c.document_count, 0);
        assert_eq!(c.tabular_review_count, 0);
        assert_eq!(c.workflow_count, 0);
        assert_eq!(c.chat_count, 0);
        assert!(!c.includes_chats);
    }

    #[test]
    fn missing_optional_fields_deserialize_fine() {
        // Older exports won't have `exported_by_display_name`.
        // Without `#[serde(default)]` this would fail; we use Option to
        // handle it. Verify the absent-key case round-trips as None.
        let raw = json!({
            "schema_version": 1,
            "exporter": "X",
            "exported_at": "2026-01-01T00:00:00Z",
            "exported_by_display_name": null,
            "contents": {
                "project": true,
                "document_count": 0,
                "tabular_review_count": 0,
                "workflow_count": 0,
                "chat_count": 0,
                "includes_chats": false
            }
        });
        let m: Manifest = serde_json::from_value(raw).unwrap();
        assert!(m.exported_by_display_name.is_none());
    }

    #[test]
    fn document_record_serializes_with_all_fields() {
        let d = DocumentRecord {
            id: "doc-1".into(),
            filename: "contract.docx".into(),
            file_type: Some("docx".into()),
            mime_type: Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document".into()),
            size_bytes: Some(12_345),
            sha256: "deadbeef".into(),
            created_at: "2026-05-06T00:00:00Z".into(),
            domain: Some("legal".into()),
            project_folder_id: None,
            decision: Some("accepted".into()),
            decision_reason: None,
            decision_summary: None,
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: DocumentRecord = serde_json::from_str(&s).unwrap();
        assert_eq!(back.size_bytes, Some(12_345));
        assert_eq!(back.sha256, "deadbeef");
        assert_eq!(back.domain.as_deref(), Some("legal"));
        assert_eq!(back.decision.as_deref(), Some("accepted"));
    }

    #[test]
    fn document_record_roundtrips_pre_v054_archives() {
        // v0.5.4 widened DocumentRecord with optional domain /
        // project_folder_id / decision* fields. Older `.coveprj`
        // archives don't carry them — the importer must still parse
        // them and treat the missing keys as None.
        let raw = json!({
            "id": "doc-pre054",
            "filename": "old.pdf",
            "file_type": "pdf",
            "mime_type": null,
            "size_bytes": 999,
            "sha256": "cafebabe",
            "created_at": "2026-04-01T00:00:00Z"
            // domain / project_folder_id / decision* deliberately omitted
        });
        let d: DocumentRecord = serde_json::from_value(raw).unwrap();
        assert_eq!(d.sha256, "cafebabe");
        assert!(d.domain.is_none());
        assert!(d.project_folder_id.is_none());
        assert!(d.decision.is_none());
    }

    #[test]
    fn workflow_record_handles_optional_columns_config() {
        // Assistant workflows have no `columns_config`; tabular workflows do.
        let asst = WorkflowRecord {
            id: "w1".into(),
            title: "Asst".into(),
            r#type: "assistant".into(),
            prompt_md: Some("Do X".into()),
            columns_config: None,
            practice: None,
            domain: Some("legal".into()),
        };
        let tab = WorkflowRecord {
            id: "w2".into(),
            title: "Tab".into(),
            r#type: "tabular".into(),
            prompt_md: None,
            columns_config: Some(json!([{"name":"col1"}])),
            practice: Some("contracts".into()),
            domain: Some("medical".into()),
        };
        let s1 = serde_json::to_string(&asst).unwrap();
        let s2 = serde_json::to_string(&tab).unwrap();
        let parsed1: WorkflowRecord = serde_json::from_str(&s1).unwrap();
        let parsed2: WorkflowRecord = serde_json::from_str(&s2).unwrap();
        assert_eq!(parsed1.domain.as_deref(), Some("legal"));
        assert_eq!(parsed2.columns_config.unwrap()[0]["name"], "col1");
        assert_eq!(parsed2.domain.as_deref(), Some("medical"));
    }

    #[test]
    fn project_record_roundtrips_pre_v054_archives() {
        // `domain` and `isolation_mode` were added in v0.5.4. Older
        // archives don't carry them; the importer falls back to
        // schema defaults.
        let raw = json!({
            "id": "p1",
            "name": "Studio 2024",
            "cm_number": "CM-1",
            "created_at": "2026-01-01T00:00:00Z",
            "original_creator_email": null
        });
        let p: ProjectRecord = serde_json::from_value(raw).unwrap();
        assert!(p.domain.is_none());
        assert!(p.isolation_mode.is_none());
    }

    #[test]
    fn schema_version_is_1() {
        assert_eq!(SCHEMA_VERSION, 1);
    }
}
