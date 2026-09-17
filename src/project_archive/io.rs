//! `.coveprj` build / parse pipeline.
//!
//! Splits the work in two layers so the route handlers stay small:
//!
//!  - `build_payload(...)` queries the DB and storage, assembles a
//!    `Payload` struct (project + documents + reviews + workflows +
//!    optional chats), and serialises it as a ZIP.
//!  - `unpack_payload(...)` does the inverse: ZIP bytes → `Payload`.
//!
//! The actual encryption / file-format envelope lives in `crypto.rs`;
//! this module is format-agnostic about transport.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde_json::Value;
use sqlx::SqlitePool;
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

use super::manifest::{
    ChatRecord, DocumentRecord, Manifest, ManifestContents, ProjectRecord,
    TabularReviewRecord, WorkflowRecord, SCHEMA_VERSION,
};

#[derive(Debug)]
pub struct Payload {
    pub manifest: Manifest,
    pub project: ProjectRecord,
    pub documents: Vec<(DocumentRecord, Vec<u8>)>,
    pub tabular_reviews: Vec<TabularReviewRecord>,
    pub workflows: Vec<WorkflowRecord>,
    pub chats: Vec<ChatRecord>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExportOptions {
    pub include_chats: bool,
}

/// Read everything that belongs to `project_id` for `user_id` from the
/// DB + storage and assemble it into a `Payload`. The caller owns the
/// SqlitePool and a storage handle (passed as a closure so we don't
/// take a hard dep on the storage trait here — easier to mock in
/// tests, and lets the route handler decide between local / S3).
pub async fn build_payload(
    db: &SqlitePool,
    user_id: &str,
    project_id: &str,
    options: ExportOptions,
    read_storage: impl Fn(&str) -> futures_util::future::BoxFuture<'_, Result<Vec<u8>>>,
) -> Result<Payload> {
    // ---------- project ----------
    // domain + isolation_mode were silently dropped in pre-v0.5.4
    // exports (the latter was even SELECTed and discarded). Both
    // travel now so the recipient lands on the same domain filter
    // and retrieval-scope mode the source project had.
    let p_row: Option<(String, String, Option<String>, String, String, String)> = sqlx::query_as(
        "SELECT id, name, cm_number, created_at, isolation_mode, domain \
         FROM projects WHERE id = ? AND user_id = ?",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .context("read project")?;
    let (pid, name, cm_number, created_at, isolation_mode, project_domain) =
        p_row.ok_or_else(|| anyhow!("project not found"))?;
    let project = ProjectRecord {
        id: pid.clone(),
        name,
        cm_number,
        created_at,
        original_creator_email: None,
        domain: Some(project_domain.clone()),
        isolation_mode: Some(isolation_mode),
    };

    // ---------- documents ----------
    // The SELECT now carries every per-doc field that survives import
    // intact: domain (project-relative filter), project_folder_id
    // (sub-folder tree placement), and the accept/reject decision
    // tuple from migration 0029. None of these used to travel.
    let doc_rows: Vec<(
        String,
        String,
        String,
        i64,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT id, filename, file_type, size_bytes, storage_path, created_at, \
                domain, project_folder_id, \
                decision, decision_reason, decision_summary \
         FROM documents WHERE user_id = ? AND project_id = ?",
    )
    .bind(user_id)
    .bind(&pid)
    .fetch_all(db)
    .await
    .context("read documents")?;

    let mut documents: Vec<(DocumentRecord, Vec<u8>)> = Vec::with_capacity(doc_rows.len());
    for (
        id,
        filename,
        file_type,
        size_bytes,
        storage_path,
        created_at,
        domain,
        project_folder_id,
        decision,
        decision_reason,
        decision_summary,
    ) in doc_rows
    {
        let bytes = if let Some(key) = storage_path.as_deref() {
            read_storage(key).await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let sha = sha256_hex(&bytes);
        documents.push((
            DocumentRecord {
                id,
                filename,
                file_type: Some(file_type),
                mime_type: None,
                size_bytes: Some(size_bytes as u64),
                sha256: sha,
                created_at,
                domain: Some(domain),
                project_folder_id,
                decision,
                decision_reason,
                decision_summary,
            },
            bytes,
        ));
    }

    // ---------- tabular reviews (config only, no cells) ----------
    let tr_rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, title, columns_config, created_at, domain \
         FROM tabular_reviews WHERE user_id = ? AND project_id = ?",
    )
    .bind(user_id)
    .bind(&pid)
    .fetch_all(db)
    .await
    .context("read tabular_reviews")?;
    let tabular_reviews: Vec<TabularReviewRecord> = tr_rows
        .into_iter()
        .map(|(id, title, cfg, created_at, tr_domain)| TabularReviewRecord {
            id,
            title: Some(title),
            columns_config: serde_json::from_str(&cfg).unwrap_or(Value::Array(Vec::new())),
            document_ids: Vec::new(), // only configuration travels
            created_at,
            domain: Some(tr_domain),
        })
        .collect();

    // ---------- workflows (custom only — no built-ins, they're recreated by id) ----------
    // Pre-v0.5.4 export hard-coded `type = "assistant"` and dropped
    // `columns_config` / `practice` / `domain`. A custom tabular
    // workflow therefore arrived monchi: only the prompt body
    // travelled, the columns vanished, and it was re-typed as an
    // assistant on the recipient side. All four fields travel now.
    let wf_rows: Vec<(String, String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT id, title, prompt_md, type, columns_config, domain \
         FROM workflows WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
    .context("read workflows")?;
    let workflows: Vec<WorkflowRecord> = wf_rows
        .into_iter()
        .map(|(id, title, prompt_md, wf_type, cols_cfg, wf_domain)| {
            let columns_config = cols_cfg
                .as_deref()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .filter(|v| !v.is_null());
            WorkflowRecord {
                id,
                title,
                r#type: wf_type,
                prompt_md: Some(prompt_md),
                columns_config,
                practice: None,
                domain: Some(wf_domain),
            }
        })
        .collect();

    // ---------- chats (opt-in) ----------
    let chats = if options.include_chats {
        let chat_rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
            "SELECT id, title, created_at FROM chats \
             WHERE user_id = ? AND project_id = ?",
        )
        .bind(user_id)
        .bind(&pid)
        .fetch_all(db)
        .await
        .context("read chats")?;
        let mut out = Vec::with_capacity(chat_rows.len());
        for (cid, title, created_at) in chat_rows {
            let msg_rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT role, content, created_at FROM messages \
                 WHERE chat_id = ? ORDER BY created_at ASC",
            )
            .bind(&cid)
            .fetch_all(db)
            .await
            .unwrap_or_default();
            let messages = msg_rows
                .into_iter()
                .map(|(role, content, created_at)| {
                    serde_json::json!({
                        "role": role, "content": content, "created_at": created_at,
                    })
                })
                .collect();
            out.push(ChatRecord {
                id: cid,
                title,
                created_at,
                messages,
            });
        }
        out
    } else {
        Vec::new()
    };

    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        exporter: format!("{} {}", crate::product::NAME, env!("CARGO_PKG_VERSION")),
        exported_at: Utc::now().to_rfc3339(),
        exported_by_display_name: None,
        contents: ManifestContents {
            project: true,
            document_count: documents.len() as u32,
            tabular_review_count: tabular_reviews.len() as u32,
            workflow_count: workflows.len() as u32,
            chat_count: chats.len() as u32,
            includes_chats: options.include_chats,
        },
    };

    Ok(Payload {
        manifest,
        project,
        documents,
        tabular_reviews,
        workflows,
        chats,
    })
}

/// Serialise a `Payload` as a ZIP archive (the bytes that go into
/// `crypto::seal`). Layout matches the spec in `project_archive/mod.rs`.
pub fn zip_payload(payload: &Payload) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::with_capacity(64 * 1024));
    {
        let mut z = ZipWriter::new(&mut buf);
        // Compression: deflate is good for JSON; documents are mostly
        // already-compressed (PDF/DOCX) so deflate is mostly a no-op
        // there but doesn't hurt.
        let opts = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        write_json(&mut z, "manifest.json", &payload.manifest, opts)?;
        write_json(&mut z, "project.json", &payload.project, opts)?;

        for (doc, bytes) in &payload.documents {
            let dir = format!("documents/{}/", doc.id);
            write_json(&mut z, &format!("{dir}meta.json"), doc, opts)?;
            z.start_file(format!("{dir}content.bin"), opts)?;
            z.write_all(bytes)?;
        }
        for tr in &payload.tabular_reviews {
            write_json(
                &mut z,
                &format!("tabular_reviews/{}.json", tr.id),
                tr,
                opts,
            )?;
        }
        for wf in &payload.workflows {
            write_json(&mut z, &format!("workflows/{}.json", wf.id), wf, opts)?;
        }
        for c in &payload.chats {
            write_json(&mut z, &format!("chats/{}.json", c.id), c, opts)?;
        }

        // Friendly README so the file isn't completely opaque to anyone
        // who unzips it manually (e.g. forensic recovery).
        z.start_file("README.txt", opts)?;
        let name = crate::product::NAME;
        let ext = crate::product::PROJECT_FILE_EXTENSION;
        z.write_all(format!("This is a {name} project archive (.{ext}).\n").as_bytes())?;
        z.write_all(format!("It is meant to be imported via the {name} UI.\n").as_bytes())?;
        z.write_all(b"Manual extraction is supported but you'll lose the citation links.\n")?;

        z.finish()?;
    }
    Ok(buf.into_inner())
}

fn write_json<W: Write + std::io::Seek, T: serde::Serialize>(
    z: &mut ZipWriter<W>,
    name: &str,
    value: &T,
    opts: SimpleFileOptions,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    z.start_file(name, opts)?;
    z.write_all(&bytes)?;
    Ok(())
}

/// Parse a ZIP payload (already decrypted by `crypto::open`) back into
/// a `Payload`. Used by the import endpoint.
pub fn unzip_payload(zip_bytes: &[u8]) -> Result<Payload> {
    let mut zip = ZipArchive::new(Cursor::new(zip_bytes))?;

    let manifest: Manifest = read_json(&mut zip, "manifest.json")?;
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported project file schema_version {}; this build expects {}",
            manifest.schema_version,
            SCHEMA_VERSION
        ));
    }
    let project: ProjectRecord = read_json(&mut zip, "project.json")?;

    let mut documents: Vec<(DocumentRecord, Vec<u8>)> = Vec::new();
    let mut tabular_reviews: Vec<TabularReviewRecord> = Vec::new();
    let mut workflows: Vec<WorkflowRecord> = Vec::new();
    let mut chats: Vec<ChatRecord> = Vec::new();

    // First pass: list filenames so we can iterate index-by-name without
    // borrowing `zip` while iterating (zip's API doesn't expose a
    // borrow-friendly iterator for both name + content).
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    for name in &names {
        if let Some(rest) = name.strip_prefix("documents/") {
            // pattern: documents/<doc_id>/meta.json   or  content.bin
            if let Some((id, tail)) = rest.split_once('/') {
                if tail == "meta.json" {
                    let meta: DocumentRecord = read_json(&mut zip, name)?;
                    let content_path = format!("documents/{id}/content.bin");
                    let bytes = read_bytes(&mut zip, &content_path).unwrap_or_default();
                    documents.push((meta, bytes));
                }
            }
        } else if let Some(_) = name.strip_prefix("tabular_reviews/") {
            if name.ends_with(".json") {
                let tr: TabularReviewRecord = read_json(&mut zip, name)?;
                tabular_reviews.push(tr);
            }
        } else if let Some(_) = name.strip_prefix("workflows/") {
            if name.ends_with(".json") {
                let wf: WorkflowRecord = read_json(&mut zip, name)?;
                workflows.push(wf);
            }
        } else if let Some(_) = name.strip_prefix("chats/") {
            if name.ends_with(".json") {
                let c: ChatRecord = read_json(&mut zip, name)?;
                chats.push(c);
            }
        }
    }

    Ok(Payload {
        manifest,
        project,
        documents,
        tabular_reviews,
        workflows,
        chats,
    })
}

fn read_json<R: Read + std::io::Seek, T: serde::de::DeserializeOwned>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<T> {
    let mut f = zip.by_name(name).context(format!("missing entry: {name}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    serde_json::from_slice(&buf).context(format!("parse {name}"))
}

fn read_bytes<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut f = zip.by_name(name).context(format!("missing entry: {name}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}
