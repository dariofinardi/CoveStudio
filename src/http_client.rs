//! Shared `reqwest::Client` factories with sane timeout defaults.
//!
//! Most of Cove Studio's outbound HTTP — MCP probe / MCP dispatch, LLM
//! chat (non-streaming bits), corpus metadata fetches, EUR-Lex /
//! Italian-legal / DILA poke endpoints — has no natural watchdog
//! upstream of `reqwest`. A network partition or a slow remote
//! server can leave a `spawn_blocking` task wedged for minutes,
//! pegging the dispatcher and tying up an HTTP-route worker.
//!
//! The previous pattern (`reqwest::Client::builder().build()`) used
//! reqwest's *implicit* defaults — no overall timeout, only a 30 s
//! connect timeout. This module fixes that without inviting every
//! caller to reinvent the wheel:
//!
//!   - `default()` — 60 s overall, 10 s connect. The right answer
//!     for **transactional** calls (single round-trip, no streaming).
//!     Use it for: MCP probes, EUR-Lex metadata, presets fetch.
//!
//!   - `streaming()` — 10 s connect, **no overall timeout**. Right for
//!     LLM streaming and SSE consumption where the server legitimately
//!     trickles the response over minutes. Pair with a per-event
//!     `tokio::time::timeout` upstream when you want a deadline.
//!
//!   - `bulk_download(secs)` — caller-specified overall timeout for
//!     long-running file downloads (HuggingFace model weights, DILA
//!     archive .tar.gz, ONNX bundles). The single configurable knob
//!     keeps the per-caller `Client::builder()` boilerplate out.
//!
//! All three set `native-tls` (the project default) implicitly via
//! reqwest features. None pass `danger_accept_invalid_certs` —
//! certificate pinning stays the platform default.

use std::time::Duration;

/// Build a `reqwest::Client` for transactional HTTP calls (single
/// short-lived round-trip, no streaming). Overall timeout 60 s,
/// connect timeout 10 s. Panics only on TLS backend init failure,
/// which is also fatal for the rest of the binary.
pub fn default() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("reqwest default client init")
}

/// Build a `reqwest::Client` for streaming / SSE / long-poll calls.
/// 10 s connect timeout, NO overall timeout — the caller is expected
/// to drive a per-event deadline via `tokio::time::timeout`.
///
/// Use specifically when the remote endpoint legitimately holds the
/// connection open over minutes (LLM token streaming, EUR-Lex sync
/// progress feed). For everything else, prefer `default()`.
pub fn streaming() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest streaming client init")
}

/// Build a `reqwest::Client` for bulk file downloads with an explicit
/// overall ceiling. Used for: HuggingFace model weights (~ 30 min for
/// the 1 GB FP32 e5-base, ~ 5 min for the 265 MB INT8 variant), DILA
/// `.tar.gz` archive ingestion, ONNX runtime bundles.
///
/// Pass the timeout that matches the slowest realistic download for
/// the use case. Anything below 60 s defeats the purpose — call
/// `default()` instead.
pub fn bulk_download(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .expect("reqwest bulk-download client init")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_client_builds() {
        let _ = default();
    }

    #[test]
    fn streaming_client_builds() {
        let _ = streaming();
    }

    #[test]
    fn bulk_client_builds_with_explicit_timeout() {
        let _ = bulk_download(Duration::from_secs(30 * 60));
    }
}
