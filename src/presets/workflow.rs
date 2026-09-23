//! Workflow preset registry — JSON files under `workflow-presets/<domain>/`.
//!
//! Each file declares one system-shipped workflow (the in-app
//! equivalent of the upstream-Mike "built-in" entries that used to
//! live in `frontend/.../builtinWorkflows.ts`). Drop a JSON into the
//! right domain subfolder and restart the app — the registry will
//! merge it into `/workflow` responses with `is_system: true` and a
//! `null` user_id so the existing UI grey-out logic kicks in.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Single column definition inside a tabular workflow's
/// `columns_config`. Mirrors `ColumnConfig` in the frontend.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowColumn {
    pub index: i64,
    pub name: String,
    /// Per-cell extraction prompt. Free-form Markdown.
    pub prompt: String,
    /// One of: `text` | `bulleted_list` | `number` | `currency` |
    /// `monetary_amount` | `percentage` | `yes_no` | `date` | `tag`.
    /// Optional — defaults to `text` at consumer side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// One workflow preset loaded from JSON. The shape projects directly
/// into the Cove Studio workflow JSON the `/workflow` endpoint emits — the
/// only extra fields on the wire (`is_system`, `created_at`,
/// `user_id`) are synthesised by the route handler when it merges
/// presets into a list response, so they don't appear here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkflowPreset {
    /// Stable identifier. Convention: `builtin-<slug>`. Used as the
    /// row id on the wire and by the frontend to detect built-ins.
    pub id: String,
    pub title: String,
    /// One of `assistant` | `tabular` | `extraction`.
    ///
    /// `extraction` presets are not offered in the chat picker: they
    /// are steps of a pipeline, where the answer is structured data for
    /// the next step rather than prose for a person. They carry a
    /// `response_schema` the model's answer is constrained to.
    #[serde(rename = "type")]
    pub kind: String,
    /// Primary professional vertical — see `crate::domain::DOMAINS`.
    /// Required. Also the folder the preset lives under by convention.
    pub domain: String,
    /// Additional domains this preset should also surface under, beyond
    /// the primary `domain`. Mirrors the DOCX-template
    /// `also_applicable_to` mechanism: a workflow useful to more than
    /// one vertical (e.g. fixed-asset analysis is relevant to both
    /// `fiscale` and `finance`) is registered ONCE and listed in every
    /// applicable domain's picker. Empty / omitted = the preset shows
    /// strictly under its primary `domain`. Each entry is validated
    /// against the canonical domain set at load time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_applicable_to: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub practice: Option<String>,
    /// Free-form Markdown system prompt. Required for assistant
    /// workflows; for tabular workflows it sets the overall posture
    /// while each `columns_config` entry carries the per-cell prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_md: Option<String>,
    /// Tabular column schema. Optional (assistant workflows omit it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns_config: Option<Vec<WorkflowColumn>>,
    /// Optional DocxTemplate id this workflow's output should be
    /// formatted with. When set on an `assistant`-type workflow, the
    /// chat handler:
    ///   1. Injects the template's authoring prompt (auto-generated
    ///      from its sidecar fields) into the system message.
    ///   2. Encourages the LLM to finish the conversation by calling
    ///      `generate_docx(template_id=..., body_md=..., metadata=...)`.
    /// Omitted → workflow produces plain chat output; the user can
    /// still ask for a docx after the fact via the bare
    /// `generate_docx` tool. The wiring is opt-in per template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_output_template: Option<String>,
    /// JSON Schema the model's answer must satisfy. Required for
    /// `extraction` presets, meaningless for the others.
    ///
    /// It lives here, beside the prompt, because the two are one
    /// contract: changing what you ask for without changing the shape
    /// you expect back is how a pipeline step starts returning
    /// something nobody parses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Value>,
}

impl WorkflowPreset {
    /// True when this preset should appear for `target`: matches the
    /// primary `domain` or any entry in `also_applicable_to`. With
    /// `target = None` (no domain filter) it always matches. Mirrors
    /// `DocxTemplate::matches_domain`.
    pub fn matches_domain(&self, target: Option<&str>) -> bool {
        match target {
            None => true,
            Some(d) => self.domain == d || self.also_applicable_to.iter().any(|x| x == d),
        }
    }

    /// Render the preset as the JSON shape the `/workflow` endpoint
    /// serves to the frontend. Adds the synthesised fields (`is_system`,
    /// `user_id`, `created_at`, `is_owner`) so consumers see the same
    /// schema as user-created rows from the DB.
    pub fn to_api_json(&self) -> Value {
        let columns = self
            .columns_config
            .as_ref()
            .map(|cols| serde_json::to_value(cols).unwrap_or(serde_json::json!([])))
            .unwrap_or(serde_json::json!([]));
        serde_json::json!({
            "id": self.id,
            "user_id": null,
            "title": self.title,
            "type": self.kind,
            "prompt_md": self.prompt_md,
            "columns_config": columns,
            "practice": self.practice,
            "domain": self.domain,
            "also_applicable_to": self.also_applicable_to,
            "default_output_template": self.default_output_template,
            "created_at": "",
            "is_system": true,
            "is_owner": false,
        })
    }
}

/// Walk every JSON file in `dir` (one level of subdirectory recursion
/// for domain folders), parse each as a `WorkflowPreset`, validate
/// minimal invariants. Broken files are skipped with a `tracing::warn`
/// — one bad preset doesn't take down the rest.
pub fn load_workflow_presets(dir: &Path) -> Result<Vec<WorkflowPreset>> {
    let mut out: Vec<WorkflowPreset> = Vec::new();
    let files = match super::collect_json_files(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "[workflow-presets] directory {} not found; no presets loaded",
                dir.display()
            );
            return Ok(out);
        }
        Err(e) => return Err(anyhow::anyhow!("read {}: {}", dir.display(), e)),
    };

    for path in files {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    "[workflow-presets] skip {} (read error): {}",
                    path.display(),
                    e
                );
                continue;
            }
        };
        match serde_json::from_slice::<WorkflowPreset>(&bytes) {
            Ok(mut p) => {
                if p.id.is_empty() || p.title.is_empty() {
                    tracing::warn!(
                        "[workflow-presets] skip {} (id/title empty)",
                        path.display()
                    );
                    continue;
                }
                if !matches!(p.kind.as_str(), "assistant" | "tabular" | "extraction") {
                    tracing::warn!(
                        "[workflow-presets] skip {} (type {} not in [assistant, tabular, extraction])",
                        path.display(),
                        p.kind
                    );
                    continue;
                }
                if !crate::domain::is_valid(&p.domain) {
                    tracing::warn!(
                        "[workflow-presets] skip {} (domain {} not in canonical set)",
                        path.display(),
                        p.domain
                    );
                    continue;
                }
                // Sanitise also_applicable_to: drop non-canonical
                // entries and any redundant listing of the primary
                // domain (warn, don't kill the preset). Keeps the
                // cross-domain surface honest without making one bad
                // entry take down an otherwise-valid workflow.
                {
                    let primary = p.domain.clone();
                    let before = p.also_applicable_to.len();
                    p.also_applicable_to.retain(|d| {
                        if d == &primary {
                            tracing::warn!(
                                "[workflow-presets] {}: also_applicable_to redundantly lists primary domain {} — dropping",
                                p.id, d
                            );
                            return false;
                        }
                        if !crate::domain::is_valid(d) {
                            tracing::warn!(
                                "[workflow-presets] {}: also_applicable_to entry {} not canonical — dropping",
                                p.id, d
                            );
                            return false;
                        }
                        true
                    });
                    let _ = before;
                }
                tracing::info!(
                    "[workflow-presets] loaded {} ({}, {}, domain={})",
                    p.id,
                    p.kind,
                    p.title,
                    p.domain
                );
                out.push(p);
            }
            Err(e) => {
                tracing::warn!(
                    "[workflow-presets] skip {} (parse error): {}",
                    path.display(),
                    e
                );
            }
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity-check every JSON file shipped under `config/workflow-presets/`:
    /// the strongly-typed loader must accept all of them, every id must
    /// be unique, every `domain` must be in the canonical set and every
    /// preset's `kind` must be one of `assistant` | `tabular`. Catches a
    /// typo in a new preset before it disappears silently at startup.
    #[test]
    fn shipped_workflow_presets_all_load_cleanly() {
        let dir = crate::presets::config_subdir("workflow-presets");
        let presets = load_workflow_presets(&dir).expect("load shipped workflow presets");
        assert!(
            !presets.is_empty(),
            "no shipped workflow presets found under {}",
            dir.display()
        );

        let mut seen = std::collections::HashSet::new();
        for p in &presets {
            assert!(
                seen.insert(p.id.clone()),
                "duplicate workflow-preset id: {}",
                p.id
            );
            assert!(
                crate::domain::is_valid(&p.domain),
                "preset {} has non-canonical domain {}",
                p.id,
                p.domain
            );
            assert!(
                matches!(p.kind.as_str(), "assistant" | "tabular" | "extraction"),
                "preset {} has unexpected kind {}",
                p.id,
                p.kind
            );
            // An extraction preset without a schema is a prompt whose
            // answer nobody can parse — the failure would only show up
            // at the first run, on a real document, after paying for it.
            if p.kind == "extraction" {
                let schema = p
                    .response_schema
                    .as_ref()
                    .unwrap_or_else(|| panic!("extraction preset {} has no response_schema", p.id));
                assert_eq!(
                    schema["type"], "object",
                    "{}: the top level of a response schema must be an object",
                    p.id
                );
                assert!(
                    schema["properties"].is_object(),
                    "{}: response schema has no properties",
                    p.id
                );
                assert!(
                    p.prompt_md.as_ref().is_some_and(|m| m.len() > 200),
                    "{}: an extraction preset needs a prompt that says what to extract",
                    p.id
                );
            }
        }
    }

    /// The three presets the insurance data-collection pipeline runs on.
    /// Named explicitly: a rename or a lost file would otherwise only
    /// surface when a broker runs the pipeline and gets nothing.
    #[test]
    fn shipped_insurance_qst_pipeline_is_complete() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config/workflow-presets");
        let presets = load_workflow_presets(&dir).expect("presets load");
        for id in [
            "builtin-insurance-qst-estrazione-domande",
            "builtin-insurance-qst-fusione-modulo",
            "builtin-insurance-qst-verifica-risposte",
        ] {
            let p = presets
                .iter()
                .find(|p| p.id == id)
                .unwrap_or_else(|| panic!("missing preset {id}"));
            assert_eq!(p.kind, "extraction", "{id}");
            assert_eq!(p.domain, "insurance", "{id}");
        }

        // The question types the extraction may emit must be the same
        // vocabulary the merged form uses, or the second step receives
        // types it cannot place.
        let types_of = |id: &str, path: &[&str]| -> Vec<String> {
            let p = presets.iter().find(|p| p.id == id).expect("preset");
            let mut node = p.response_schema.clone().expect("schema");
            for step in path {
                node = node[*step].clone();
            }
            node.as_array()
                .expect("enum array")
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .collect()
        };
        let extracted = types_of(
            "builtin-insurance-qst-estrazione-domande",
            &["properties", "questions", "items", "properties", "type", "enum"],
        );
        let merged = types_of(
            "builtin-insurance-qst-fusione-modulo",
            &[
                "properties", "sections", "items", "properties", "fields", "items",
                "properties", "type", "enum",
            ],
        );
        assert_eq!(extracted, merged, "the two steps disagree on field types");
        assert!(extracted.contains(&"currency".to_string()));
        assert!(extracted.contains(&"table".to_string()));
    }

    #[test]
    fn matches_domain_primary_also_and_none() {
        let mut p = WorkflowPreset {
            id: "x".into(),
            title: "X".into(),
            kind: "tabular".into(),
            domain: "fiscale".into(),
            also_applicable_to: vec!["finance".into()],
            practice: None,
            prompt_md: None,
            columns_config: None,
            default_output_template: None,
            response_schema: None,
        };
        assert!(p.matches_domain(None), "no filter ⇒ always matches");
        assert!(p.matches_domain(Some("fiscale")), "primary domain matches");
        assert!(p.matches_domain(Some("finance")), "also_applicable_to matches");
        assert!(!p.matches_domain(Some("legal")), "unrelated domain does not match");
        p.also_applicable_to.clear();
        assert!(!p.matches_domain(Some("finance")), "without also_applicable_to, secondary no longer matches");
    }

    /// The two cross-domain commercialista workflows (fixed-asset
    /// analysis + bookkeeping quadrature) must surface in BOTH the
    /// `fiscale` and `finance` pickers via `also_applicable_to`.
    /// Guards the cross-domain registration the user asked for in
    /// v0.7.3.
    #[test]
    fn shipped_cross_domain_commercialista_presets_in_both_pickers() {
        let dir = crate::presets::config_subdir("workflow-presets");
        let presets = load_workflow_presets(&dir).expect("load");
        for id in ["builtin-fiscale-analisi-cespiti", "builtin-finance-controlli-libri-contabili"] {
            let p = presets
                .iter()
                .find(|p| p.id == id)
                .unwrap_or_else(|| panic!("missing cross-domain preset {id}"));
            assert!(p.matches_domain(Some("fiscale")), "{id} must show under fiscale");
            assert!(p.matches_domain(Some("finance")), "{id} must show under finance");
        }
    }

    /// Anchor the Pubblica Amministrazione presets shipped with the
    /// `docs/pa-prompts.md` spec: the four Fase-1 priority workflows
    /// (delibera, 241-check, appalto-review, ptpct) plus determina,
    /// PNRR milestone and RUP checklist. Failure here means a
    /// contributor renamed or deleted one of them, or the `pa` domain
    /// itself disappeared from the canonical set.
    #[test]
    fn shipped_pa_workflows_present_and_typed() {
        let dir = crate::presets::config_subdir("workflow-presets");
        let presets = load_workflow_presets(&dir).expect("load");

        // Domain must be canonical or the loader silently dropped the
        // preset (validate_minimal rejects unknown domains).
        assert!(
            crate::domain::is_valid("pa"),
            "`pa` must be in the canonical domain set for these presets to load"
        );

        let expected: [(&str, &str); 7] = [
            ("builtin-pa-delibera", "assistant"),
            ("builtin-pa-determina", "assistant"),
            ("builtin-pa-appalto-review", "assistant"),
            ("builtin-pa-rup-checklist", "tabular"),
            ("builtin-pa-241-check", "assistant"),
            ("builtin-pa-pnrr-milestone", "tabular"),
            ("builtin-pa-ptpct", "assistant"),
        ];
        for (id, kind) in expected {
            let preset = presets
                .iter()
                .find(|p| p.id == id)
                .unwrap_or_else(|| panic!("PA preset must ship: {id}"));
            assert_eq!(preset.domain, "pa", "{id} must be domain=pa");
            assert_eq!(preset.kind, kind, "{id} must be type={kind}");
            // Every PA preset belongs to one of the spec's five blocks
            // and the practice string anchors that mapping in the UI.
            let practice = preset.practice.as_deref().unwrap_or("");
            assert!(
                practice.starts_with("PA — "),
                "{id} practice must start with 'PA — ': got {practice:?}"
            );
        }
        // Sanity: the tabular ones must carry columns_config; the
        // assistant ones must NOT (otherwise the picker mis-renders).
        for p in presets.iter().filter(|p| p.domain == "pa") {
            match p.kind.as_str() {
                "tabular" => assert!(
                    p.columns_config.as_ref().is_some_and(|c| !c.is_empty()),
                    "tabular preset {} must have non-empty columns_config",
                    p.id
                ),
                "assistant" => assert!(
                    p.columns_config.is_none(),
                    "assistant preset {} must NOT carry columns_config",
                    p.id
                ),
                _ => {}
            }
        }
    }

    /// Anchor the three NIS2 compliance presets shipped together with the
    /// `docs/nis2-prompts.md` spec — both an assistant workflow (linked
    /// to the docx template) and a tabular workflow for policy inventory.
    /// Failure here means a contributor renamed or deleted one of them.
    #[test]
    fn shipped_compliance_nis2_presets_present_and_wired() {
        let dir = crate::presets::config_subdir("workflow-presets");
        let presets = load_workflow_presets(&dir).expect("load");

        let assistant = presets
            .iter()
            .find(|p| p.id == "builtin-compliance-nis2-audit-readiness")
            .expect("NIS2 audit-readiness assistant workflow must ship");
        assert_eq!(assistant.kind, "assistant");
        assert_eq!(assistant.domain, "compliance");
        assert_eq!(
            assistant.default_output_template.as_deref(),
            Some("compliance/nis2-audit-readiness-report"),
            "assistant workflow must reference the docx template"
        );
        assert!(assistant.columns_config.is_none());

        let tabular = presets
            .iter()
            .find(|p| p.id == "builtin-compliance-nis2-policy-inventory")
            .expect("NIS2 policy-inventory tabular workflow must ship");
        assert_eq!(tabular.kind, "tabular");
        assert_eq!(tabular.domain, "compliance");
        let cols = tabular.columns_config.as_ref().expect("tabular columns");
        assert!(
            cols.len() >= 7,
            "expected ≥7 columns for the policy inventory, got {}",
            cols.len()
        );
        // Column 0 must be the human-readable title — drives the row
        // header in the review grid.
        assert!(
            cols[0].name.to_lowercase().contains("titolo"),
            "first column should be the document title, got {:?}",
            cols[0].name
        );
    }
}
