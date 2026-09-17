//! Italian-legal-corpus connector.
//!
//! Bridges the `dossier-legal/italian-legal-corpus` HuggingFace
//! dataset (CC-BY-4.0, snapshot 2026-03-01) into Cove Studio's RAG store.
//!
//! Two phases:
//!
//! 1. **Bulk metadata import**. The user clicks "Scarica indice" once.
//!    We download the dataset's eight Parquet shards, project only
//!    the metadata columns (no `text` body), filter to
//!    `source ∈ {normattiva, corte_costituzionale}`, and INSERT the
//!    rows into a local SQLite table backed by an FTS5 virtual table.
//!    ~80 MB on disk, ~91k rows.
//!
//! 2. **Per-document fetch**. When the user picks a row from search
//!    results and clicks "Indicizza", we hit the HuggingFace
//!    `/rows?offset=N&length=1` endpoint to get just that row's full
//!    text, then run it through the same hash-keyed cache + embedding
//!    pipeline as EUR-Lex (see `docs/CACHE.md`).
//!
//! The dataset is a frozen snapshot. We pin its commit SHA so
//! re-imports don't drift; for newer acts the user can layer a live
//! Normattiva fetcher later.

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use std::sync::Arc;

/// HuggingFace dataset slug (`{owner}/{name}`).
pub const DATASET: &str = "dossier-legal/italian-legal-corpus";
/// Pinned commit SHA — the dataset author's only published snapshot.
/// Pinning keeps imports reproducible; bumping it on a future
/// snapshot is a one-line change here.
pub const DATASET_REVISION: &str =
    "e503a93f124d76b26aa420abcf4b7f3f9d87d793";

/// Shards we know exist for the `default` config / `train` split.
/// The dataset publishes them as `data/train-0000{N}-of-00008.parquet`.
pub const TOTAL_SHARDS: usize = 8;

/// Sources we ingest. EUR-Lex IT is excluded because the user already
/// has a direct EUR-Lex connector; OpenGA is excluded by default
/// (large + niche, opt-in via settings later).
pub const DEFAULT_SOURCES: &[&str] = &["normattiva", "corte_costituzionale"];

/// Public progress snapshot for the bulk-import job. Mirrors the
/// `italian_corpus_meta` row so the route layer can serialise it
/// directly without another DB hit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportProgress {
    pub job_state: String,
    pub current_shard: i64,
    pub total_shards: i64,
    pub rows_imported: i64,
    pub job_error: Option<String>,
    pub row_count: i64,
    pub last_import_at: Option<String>,
    pub dataset_revision: Option<String>,
}

pub async fn read_progress(db: &SqlitePool) -> Result<ImportProgress> {
    let row: (String, i64, i64, i64, Option<String>, i64, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT job_state, job_current_shard, job_total_shards, \
                    job_rows_imported, job_error, row_count, last_import_at, \
                    dataset_revision \
             FROM italian_corpus_meta WHERE id = 1",
        )
        .fetch_one(db)
        .await
        .context("italian_corpus_meta row missing")?;
    Ok(ImportProgress {
        job_state: row.0,
        current_shard: row.1,
        total_shards: row.2,
        rows_imported: row.3,
        job_error: row.4,
        row_count: row.5,
        last_import_at: row.6,
        dataset_revision: row.7,
    })
}

async fn set_state(db: &SqlitePool, state: &str) -> Result<()> {
    sqlx::query("UPDATE italian_corpus_meta SET job_state = ? WHERE id = 1")
        .bind(state)
        .execute(db)
        .await?;
    Ok(())
}

async fn set_progress(
    db: &SqlitePool,
    current_shard: i64,
    total_shards: i64,
    rows_imported: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE italian_corpus_meta SET \
           job_current_shard = ?, job_total_shards = ?, job_rows_imported = ? \
         WHERE id = 1",
    )
    .bind(current_shard)
    .bind(total_shards)
    .bind(rows_imported)
    .execute(db)
    .await?;
    Ok(())
}

async fn set_error(db: &SqlitePool, msg: &str) -> Result<()> {
    sqlx::query(
        "UPDATE italian_corpus_meta SET job_state = 'failed', job_error = ? WHERE id = 1",
    )
    .bind(msg)
    .execute(db)
    .await?;
    Ok(())
}

async fn finish_import(
    db: &SqlitePool,
    row_count: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE italian_corpus_meta SET \
           job_state = 'ready', last_import_at = datetime('now'), \
           row_count = ?, dataset_revision = ?, job_error = NULL \
         WHERE id = 1",
    )
    .bind(row_count)
    .bind(DATASET_REVISION)
    .execute(db)
    .await?;
    Ok(())
}

fn shard_url(shard: usize) -> String {
    // The auto-converted Parquet branch always exists for valid
    // datasets and is row-identical to the source files — but pinned
    // to the source revision via the dataset_revision query param.
    format!(
        "https://huggingface.co/datasets/{DATASET}/resolve/{rev}/data/train-{n:05}-of-{total:05}.parquet",
        rev = DATASET_REVISION,
        n = shard,
        total = TOTAL_SHARDS,
    )
}

/// Run the full bulk import, end to end. Designed to be called from
/// a `tokio::spawn`'d task — updates the meta row as it progresses
/// so the UI can poll. On any error it sets `job_state = 'failed'`
/// and stores the message; otherwise flips to `'ready'`.
pub async fn run_import(db: Arc<SqlitePool>) -> Result<()> {
    set_state(&db, "downloading").await?;
    set_progress(&db, 0, TOTAL_SHARDS as i64, 0).await?;

    // Wipe prior contents — re-imports are full replacements (the
    // dataset has no row-level versioning that would let us diff).
    sqlx::query("DELETE FROM italian_corpus")
        .execute(&*db)
        .await?;
    sqlx::query("DELETE FROM italian_corpus_fts")
        .execute(&*db)
        .await?;

    let client = reqwest::Client::builder()
        .user_agent(crate::product::user_agent(Some("italian-legal-corpus importer")))
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    // Hard cap on shard size, applied BEFORE the bytes ever reach the
    // Parquet decoder. Mitigates the unfixed-upstream Thrift footer
    // advisory (parquet → thrift 0.17.0): a malicious shard could
    // declare an oversized allocation in its footer that the parser
    // would honour on a 16 GB workstation. Threshold is configurable
    // via `config/corpora.json` (default 500 MB; live Cassazione shards
    // run ~50-80 MB).
    let limits = crate::corpora::limits::load_limits();
    let max_shard_bytes = limits.max_parquet_file_size_bytes();

    let mut total_rows: i64 = 0;
    let mut row_offset_running: i64 = 0;
    for shard in 0..TOTAL_SHARDS {
        set_progress(
            &db,
            shard as i64,
            TOTAL_SHARDS as i64,
            total_rows,
        )
        .await?;
        set_state(&db, "downloading").await?;

        let url = shard_url(shard);
        tracing::info!("[italian-legal] downloading shard {shard}/{TOTAL_SHARDS}: {url}");
        let bytes = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("HTTP error {url}"))?
            .bytes()
            .await
            .with_context(|| format!("body {url}"))?;
        tracing::info!(
            "[italian-legal] shard {shard} downloaded ({:.1} MB)",
            bytes.len() as f64 / 1_000_000.0
        );
        if (bytes.len() as u64) > max_shard_bytes {
            anyhow::bail!(
                "[italian-legal] shard {shard} from {url} is {:.1} MB, above the \
                 max_parquet_file_size_mb cap ({} MB) configured in \
                 config/corpora.json — refusing to decode it (thrift advisory \
                 mitigation). Raise the cap only if you trust the source.",
                bytes.len() as f64 / 1_000_000.0,
                limits.max_parquet_file_size_mb
            );
        }

        set_state(&db, "importing").await?;
        let imported = import_shard_into_db(
            &db,
            bytes.to_vec(),
            row_offset_running,
        )
        .await?;
        // The row_offset_running reflects ALL rows in the shard
        // (filtered or not) so that subsequent /rows fetches map to
        // the right offset in the original split.
        let shard_rows = parquet_row_count_estimate(bytes.len());
        row_offset_running += shard_rows as i64;
        total_rows += imported as i64;

        set_progress(
            &db,
            (shard + 1) as i64,
            TOTAL_SHARDS as i64,
            total_rows,
        )
        .await?;
    }

    finish_import(&db, total_rows).await?;
    tracing::info!("[italian-legal] import complete: {total_rows} rows");
    Ok(())
}

/// Best-effort row-count estimate for offset bookkeeping when we
/// don't want to walk the file twice. Each shard is row-balanced at
/// build time so an estimate based on file size is close enough; the
/// only consequence of being off is `/rows` calls hitting the wrong
/// offset, which we mitigate at fetch time by looking the row up by
/// its `id` field as a fallback.
fn parquet_row_count_estimate(_bytes: usize) -> usize {
    // Dataset is published as 347252 rows / 8 shards ≈ 43406 rows/shard.
    347_252 / TOTAL_SHARDS
}

/// Parse one shard's bytes, project the metadata columns, filter to
/// our source allow-list, and INSERT the matching rows + their FTS5
/// counterparts. Returns the number of rows inserted.
async fn import_shard_into_db(
    db: &SqlitePool,
    bytes: Vec<u8>,
    base_offset: i64,
) -> Result<usize> {
    // Parquet decoding is CPU-bound and synchronous — run it on a
    // blocking thread so the tokio runtime stays free.
    let parsed: Vec<ParsedRow> = tokio::task::spawn_blocking(move || {
        parse_parquet_metadata(&bytes, base_offset)
    })
    .await
    .context("spawn_blocking failed")??;

    let mut tx = db.begin().await?;
    for row in &parsed {
        sqlx::query(
            "INSERT OR REPLACE INTO italian_corpus \
             (hf_id, row_offset, source, doc_type, title, authority, number, \
              year, date, ecli, text_length) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&row.hf_id)
        .bind(row.row_offset)
        .bind(&row.source)
        .bind(&row.doc_type)
        .bind(&row.title)
        .bind(&row.authority)
        .bind(&row.number)
        .bind(row.year)
        .bind(&row.date)
        .bind(&row.ecli)
        .bind(row.text_length)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO italian_corpus_fts (hf_id, title, authority, number) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&row.hf_id)
        .bind(row.title.as_deref().unwrap_or(""))
        .bind(row.authority.as_deref().unwrap_or(""))
        .bind(row.number.as_deref().unwrap_or(""))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(parsed.len())
}

#[derive(Debug)]
struct ParsedRow {
    hf_id: String,
    row_offset: i64,
    source: String,
    doc_type: Option<String>,
    title: Option<String>,
    authority: Option<String>,
    number: Option<String>,
    year: Option<i64>,
    date: Option<String>,
    ecli: Option<String>,
    text_length: i64,
}

/// Synchronous parquet decoding. Reads the entire shard into memory
/// (each shard ~210 MB), projects the metadata columns, returns the
/// rows whose `source` is in `DEFAULT_SOURCES`.
fn parse_parquet_metadata(
    bytes: &[u8],
    base_offset: i64,
) -> Result<Vec<ParsedRow>> {
    use bytes::Bytes;
    use parquet::arrow::arrow_reader::{
        ArrowReaderOptions, ParquetRecordBatchReaderBuilder,
    };
    use parquet::arrow::ProjectionMask;

    let bytes = Bytes::copy_from_slice(bytes);
    let opts = ArrowReaderOptions::new();
    let builder = ParquetRecordBatchReaderBuilder::try_new_with_options(
        bytes, opts,
    )?;

    // Project only the metadata columns by name. The schema gives us
    // 15 fields; we want 10 of them.
    let schema = builder.parquet_schema().clone();
    let want_names: &[&str] = &[
        "id",
        "source",
        "doc_type",
        "title",
        "authority",
        "number",
        "year",
        "date",
        "ecli",
        "text_length",
    ];
    let want_indices: Vec<usize> = schema
        .columns()
        .iter()
        .enumerate()
        .filter(|(_, c)| want_names.iter().any(|n| c.name() == *n))
        .map(|(i, _)| i)
        .collect();
    let mask = ProjectionMask::leaves(&schema, want_indices);

    let mut reader = builder.with_projection(mask).build()?;

    let allow: std::collections::HashSet<&str> =
        DEFAULT_SOURCES.iter().copied().collect();

    let mut out: Vec<ParsedRow> = Vec::with_capacity(50_000);
    let mut running_offset = base_offset;
    while let Some(batch_result) = reader.next() {
        let batch = batch_result?;
        let n = batch.num_rows();
        let arrow_schema = batch.schema();
        let col_idx = |name: &str| -> Option<usize> {
            arrow_schema.fields().iter().position(|f| f.name() == name)
        };
        let id_idx = col_idx("id");
        let source_idx = col_idx("source");
        let doc_type_idx = col_idx("doc_type");
        let title_idx = col_idx("title");
        let authority_idx = col_idx("authority");
        let number_idx = col_idx("number");
        let year_idx = col_idx("year");
        let date_idx = col_idx("date");
        let ecli_idx = col_idx("ecli");
        let text_length_idx = col_idx("text_length");

        for row_in_batch in 0..n {
            let row_offset = running_offset + row_in_batch as i64;
            let source = string_at(&batch, source_idx, row_in_batch).unwrap_or_default();
            if !allow.contains(source.as_str()) {
                continue;
            }
            let hf_id = match string_at(&batch, id_idx, row_in_batch) {
                Some(v) if !v.is_empty() => v,
                _ => continue,
            };
            out.push(ParsedRow {
                hf_id,
                row_offset,
                source,
                doc_type: string_at(&batch, doc_type_idx, row_in_batch),
                title: string_at(&batch, title_idx, row_in_batch),
                authority: string_at(&batch, authority_idx, row_in_batch),
                number: string_at(&batch, number_idx, row_in_batch),
                year: i64_at(&batch, year_idx, row_in_batch),
                date: string_at(&batch, date_idx, row_in_batch),
                ecli: string_at(&batch, ecli_idx, row_in_batch),
                text_length: i64_at(&batch, text_length_idx, row_in_batch).unwrap_or(0),
            });
        }
        running_offset += n as i64;
    }
    Ok(out)
}

fn string_at(
    batch: &arrow_array::RecordBatch,
    col_idx: Option<usize>,
    row: usize,
) -> Option<String> {
    use arrow_array::cast::AsArray;
    use arrow_array::Array;
    let idx = col_idx?;
    let col = batch.column(idx);
    if let Some(arr) = col.as_string_opt::<i32>() {
        if arr.is_null(row) {
            return None;
        }
        return Some(arr.value(row).to_string());
    }
    if let Some(arr) = col.as_string_opt::<i64>() {
        if arr.is_null(row) {
            return None;
        }
        return Some(arr.value(row).to_string());
    }
    None
}

fn i64_at(
    batch: &arrow_array::RecordBatch,
    col_idx: Option<usize>,
    row: usize,
) -> Option<i64> {
    use arrow_array::cast::AsArray;
    use arrow_array::types::{Int32Type, Int64Type};
    use arrow_array::Array;
    let idx = col_idx?;
    let col = batch.column(idx);
    if let Some(arr) = col.as_primitive_opt::<Int64Type>() {
        if arr.is_null(row) {
            return None;
        }
        return Some(arr.value(row));
    }
    if let Some(arr) = col.as_primitive_opt::<Int32Type>() {
        if arr.is_null(row) {
            return None;
        }
        return Some(arr.value(row) as i64);
    }
    None
}

/// Fetch a single document's full text from HuggingFace by row offset.
/// Returns `(title, text)` — the title is included for cross-checking
/// against our local metadata in case the dataset row order shifts.
pub async fn fetch_full_text(
    client: &reqwest::Client,
    row_offset: i64,
) -> Result<(String, String)> {
    let url = format!(
        "https://datasets-server.huggingface.co/rows?dataset={ds}&config=default&split=train&offset={off}&length=1",
        ds = urlencoding_encode(DATASET),
        off = row_offset
    );
    tracing::info!("[italian-legal] GET {url}");
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("HF rows GET {url}"))?
        .error_for_status()
        .with_context(|| format!("HF rows status {url}"))?;
    let body: serde_json::Value = resp.json().await?;
    let row = body
        .get("rows")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|wrap| wrap.get("row"))
        .ok_or_else(|| anyhow::anyhow!("no rows in HF response"))?;
    let title = row
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let text = row
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("row has no text"))?
        .to_string();
    Ok((title, text))
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b'/' => out.push_str("%2F"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
