//! Functional tests of the real PII pipeline: GLiNER2 weights on disk,
//! ONNX Runtime sessions, chunking, redaction and the over-masking
//! pass.
//!
//! These load ~1.1 GB of weights and take seconds per pass, so they are
//! `#[ignore]`d and a vanilla `cargo test` stays fast. Run them
//! explicitly — and do, before touching anything in `src/ner`:
//!
//!     cargo test --features pdf,rag,ner-pii --test pii_redaction -- --ignored --nocapture
//!
//! The first run downloads the model if the cache is cold. Everything
//! after that is offline.
//!
//! What these tests are for: the unit tests in `src/ner/engine.rs`
//! cover the pure functions, but the questions that matter here cannot
//! be answered without the model — does the engine still load after an
//! `ort` upgrade, does the threshold actually change what gets masked,
//! does a name survive a chunk boundary, is anything left in clear
//! text. A green unit suite over a broken engine would be worse than
//! no suite at all.

#![cfg(all(feature = "ner-pii", feature = "rag"))]

use cove_studio::ner;

/// A short Italian document with one of each thing we promise to mask.
const SAMPLE: &str = "Il sottoscritto Mario Rossi, nato a Reggio Emilia, \
codice fiscale RSSMRA80A01H223X, residente in via Emilia 12, dichiara di \
aver ricevuto il pagamento sull'IBAN IT60X0542811101000000123456. \
Per comunicazioni: mario.rossi@example.com oppure 0522 123456.";

fn is_masked(out: &str, needle: &str) -> bool {
    !out.contains(needle)
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn engine_loads_and_reports_ready() {
    // Loading is the single most fragile step after an ort or
    // onnxruntime bump: a version mismatch used to deadlock, and now
    // fails with BadVersion. Assert the happy path explicitly.
    let out = ner::mask_pii(SAMPLE, None, 0.5, None)
        .await
        .expect("first pass should load the engine and return");
    assert!(!out.is_empty());
    assert!(
        matches!(ner::status().await, ner::NerStatus::Ready),
        "status must be Ready after a successful pass"
    );
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn masks_the_obvious_identifiers() {
    let out = ner::mask_pii(SAMPLE, None, 0.5, None)
        .await
        .expect("mask_pii");
    println!("--- redacted ---\n{out}\n----------------");

    // The name is the one thing that must never survive.
    assert!(is_masked(&out, "Mario Rossi"), "name left in clear: {out}");
    // The rest are structured identifiers the model is trained on.
    assert!(
        is_masked(&out, "mario.rossi@example.com"),
        "email left in clear: {out}"
    );
    assert!(
        is_masked(&out, "IT60X0542811101000000123456"),
        "IBAN left in clear: {out}"
    );
    // Placeholders are uppercase label names in square brackets.
    assert!(out.contains('['), "no placeholder at all: {out}");
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn over_masking_catches_every_repetition() {
    // This is the guarantee we chose over the library's offset-only
    // redaction: the model tags a name where it is confident, and the
    // second pass removes the rest. A document that names someone four
    // times must not come back naming them once.
    let text = format!(
        "{SAMPLE}\n\nSi conferma che Mario Rossi ha firmato. \
         Successivamente Mario Rossi ha revocato. \
         Copia trasmessa a Mario Rossi."
    );
    let out = ner::mask_pii(&text, None, 0.5, None)
        .await
        .expect("mask_pii");
    assert!(
        is_masked(&out, "Mario Rossi"),
        "a repeated name survived redaction: {out}"
    );
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn a_higher_threshold_masks_no_more_than_a_lower_one() {
    // The slider must actually do something, and in the right
    // direction: raising the threshold can only keep or reduce the set
    // of masked spans, never enlarge it.
    let low = ner::extract_entities(SAMPLE, None, 0.2)
        .await
        .expect("extract at 0.2");
    let high = ner::extract_entities(SAMPLE, None, 0.9)
        .await
        .expect("extract at 0.9");
    println!("spans: 0.2 → {}, 0.9 → {}", low.len(), high.len());
    assert!(
        high.len() <= low.len(),
        "raising the threshold enlarged the span set ({} → {})",
        low.len(),
        high.len()
    );
    // And every span the strict pass kept must clear its own bar.
    for e in &high {
        assert!(e.score >= 0.9, "span below the threshold: {e:?}");
    }
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn offsets_point_at_the_text_that_was_passed_in() {
    // The new engine reports byte offsets; if they were relative to a
    // chunk instead of the document, redaction would cut in the wrong
    // place — silently, and only on long inputs.
    let spans = ner::extract_entities(SAMPLE, None, 0.5)
        .await
        .expect("extract");
    assert!(!spans.is_empty(), "no spans at all on the sample");
    for e in &spans {
        assert!(
            e.end <= SAMPLE.len() && e.start < e.end,
            "offsets out of range: {e:?}"
        );
        assert_eq!(
            &SAMPLE[e.start..e.end],
            e.text,
            "offsets do not point at the reported text"
        );
    }
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights, several chunks"]
async fn entities_survive_a_chunk_boundary() {
    // Padding is counted in words because the chunker is; 256 words per
    // window means ~400 words of filler puts the second name deep into
    // the second chunk, with one name before the seam and one after.
    let filler = "Il presente documento prosegue con clausole di rito. ".repeat(60);
    let text = format!(
        "Premessa: Mario Rossi dichiara quanto segue. {filler} \
         In fede, Giulia Bianchi, giulia.bianchi@example.org."
    );
    let ticks = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(usize, usize)>::new()));
    let sink = ticks.clone();
    let progress: ner::ProgressFn = std::sync::Arc::new(move |current, total| {
        if let Ok(mut g) = sink.lock() {
            g.push((current, total));
        }
    });

    let out = ner::mask_pii(&text, None, 0.5, Some(progress))
        .await
        .expect("mask_pii on a long document");

    assert!(
        is_masked(&out, "Mario Rossi"),
        "name before the seam survived: {out}"
    );
    assert!(
        is_masked(&out, "Giulia Bianchi"),
        "name after the seam survived: {out}"
    );
    assert!(
        is_masked(&out, "giulia.bianchi@example.org"),
        "email after the seam survived: {out}"
    );

    // The progress callback drives the UI's n/N counter; a silent pass
    // would leave the user staring at a stalled bar.
    let ticks = ticks.lock().unwrap().clone();
    assert!(!ticks.is_empty(), "no progress ticks were emitted");
    let total = ticks[0].1;
    assert!(total > 1, "this document should have been chunked");
    assert_eq!(
        ticks.last().unwrap().0,
        total,
        "the last tick must reach the total"
    );
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn multibyte_text_does_not_panic_or_corrupt() {
    // Offsets are bytes; a naive slice on accented or emoji text
    // panics. The document deliberately mixes both around the name.
    let text = "Verbale — 30 m² 🚀 — il perito Niccolò Dall'Acqua (niccolo@esempio.it) \
                attesta: «tutto è conforme», €1.200,50.";
    let out = ner::mask_pii(text, None, 0.5, None)
        .await
        .expect("mask_pii on multibyte text");
    assert!(out.contains("30 m²"), "unrelated text was damaged: {out}");
    assert!(out.contains('€'), "unrelated text was damaged: {out}");
    assert!(
        is_masked(&out, "niccolo@esempio.it"),
        "email left in clear: {out}"
    );
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn a_document_without_personal_data_comes_back_unchanged() {
    // False positives are the cost of over-masking, so the floor case
    // matters: text with nothing personal in it must not be shredded.
    let text = "L'appaltatore consegna l'opera entro il termine pattuito. \
                Le penali sono calcolate per ogni giorno di ritardo.";
    let out = ner::mask_pii(text, None, 0.5, None)
        .await
        .expect("mask_pii");
    assert_eq!(out, text, "clean text was modified: {out}");
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn scoped_labels_narrow_the_pass() {
    // A caller can ask for one thing only (a banking-redaction
    // workflow); the rest must then stay untouched.
    let out = ner::mask_pii(SAMPLE, Some(&["iban"]), 0.5, None)
        .await
        .expect("mask_pii with scoped labels");
    assert!(
        is_masked(&out, "IT60X0542811101000000123456"),
        "the requested label was not masked: {out}"
    );
    assert!(
        out.contains("Mario Rossi"),
        "a label that was not requested got masked anyway: {out}"
    );
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn the_engine_is_reused_across_calls() {
    // The engine thread is created once. A second pass that paid the
    // load again would mean the handle is not being cached — visible
    // here as a multi-second second call.
    let first = std::time::Instant::now();
    ner::mask_pii(SAMPLE, None, 0.5, None).await.expect("first");
    let first = first.elapsed();

    let second = std::time::Instant::now();
    ner::mask_pii(SAMPLE, None, 0.5, None).await.expect("second");
    let second = second.elapsed();

    println!("first {first:?}, second {second:?}");
    assert!(
        second < first,
        "the second pass was not faster, so the engine was probably reloaded \
         (first {first:?}, second {second:?})"
    );
}

#[tokio::test]
#[ignore = "loads ~1.1 GB of GLiNER2 weights"]
async fn empty_input_is_not_an_error() {
    let out = ner::mask_pii("", None, 0.5, None)
        .await
        .expect("empty input must not fail");
    assert_eq!(out, "");
}
