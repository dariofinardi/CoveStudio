//! EUR-Lex adapter — V1 strategy: direct CELEX URL with HTML scraping.
//!
//! No registration required. The official SOAP Web Service (CWS) at
//! `https://eur-lex.europa.eu/EURLexWebService` requires an EU Login
//! account + a usage form whose approval takes 1-5 business days; for
//! V1 we fetch HTML directly from the public legal-content endpoint,
//! which serves all 24 official languages with no auth.
//!
//! See `docs/EURLEX_REGISTRATION.md` if/when V2 needs the SOAP CWS for
//! advanced full-text search (CELEX lookup is enough for V1).
//!
//! URL pattern:
//!     https://eur-lex.europa.eu/legal-content/{LANG}/TXT/HTML/?uri=CELEX:{celex}
//!
//! - 200 OK with the HTML body when the document exists in `LANG`.
//! - 302 redirect to a language-fallback page or document-family page
//!   when the requested language is missing — we treat this as
//!   "not-in-this-language" and switch to EN if `fallback_en` is true.
//! - 404 if the CELEX is invalid.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use scraper::{Html, Selector};

use super::{CorpusDocument, CorpusHit, LegalCorpusAdapter};

const BASE: &str = "https://eur-lex.europa.eu";
const ALL_LANGS: &[&str] = &[
    "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "ga", "hr",
    "hu", "it", "lt", "lv", "mt", "nl", "pl", "pt", "ro", "sk", "sl", "sv",
];

pub struct EurlexAdapter {
    client: reqwest::Client,
}

impl EurlexAdapter {
    pub fn new() -> Self {
        // EUR-Lex serves a fully-rendered HTML body to browser-like
        // requests but a near-empty stub when the User-Agent looks
        // like a generic crawler — confirmed empirically when our
        // earlier product UA produced sub-100-char bodies on
        // pages a browser renders fine. We pose as a real browser.
        // Per-request `Accept` and `Accept-Language` headers go on
        // each call so the adapter can request specific languages
        // via content negotiation when needed.
        let client = reqwest::Client::builder()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .expect("reqwest client init");
        Self { client }
    }

    /// CELEX validation — accept the official 11-character pattern
    /// without being strict about every sector code, because EUR-Lex
    /// adds new ones over time. Pattern (informal):
    ///     {sector}{year}{type}{number}
    /// e.g. `32016R0679` (Reg. 2016/679 = GDPR), `12012E101` (TFEU).
    /// We just reject obvious garbage.
    fn looks_like_celex(s: &str) -> bool {
        let s = s.trim();
        if s.len() < 7 || s.len() > 24 {
            return false;
        }
        // CELEX is purely alphanumeric — reject anything with slashes,
        // parens, spaces, etc. Natural references like "2014/24/UE"
        // get caught here and routed through the natural-ref parser.
        s.chars().all(|c| c.is_ascii_alphanumeric())
    }

    /// Build the list of CELEX candidates worth probing for a given
    /// user input. Strategy:
    ///
    /// - If the input is already a CELEX shape → just that one.
    /// - If it has a year/number pattern → produce one candidate per
    ///   common legislation act type (R/L/D/H) so the actual probe
    ///   discovers which exist. Court-of-Justice and treaty sectors
    ///   are out-of-scope for V1 since they need different sectors
    ///   (6, 1) — those can be added later.
    /// - Otherwise → empty list (caller surfaces an explanatory error).
    ///
    /// We do NOT hardcode "Regolamento" → R or "Direttiva" → L: that
    /// keyword→letter table proved fragile when the user's phrasing
    /// drifted. Instead we hand all type variants to the prober and
    /// let EUR-Lex itself say which exist. Type words in the input
    /// are still useful as a *display hint* in the UI but they no
    /// longer gate which CELEXes get probed.
    pub(crate) fn enumerate_celex_candidates(input: &str) -> Vec<String> {
        let raw = input.trim();
        if raw.is_empty() {
            return Vec::new();
        }

        // 1. Bare CELEX → single candidate.
        if Self::looks_like_celex(raw) {
            return vec![raw.to_string()];
        }

        // 2. ELI shortcut: eli/{reg|dir|dec}/{year}/{num}[/oj] → exact CELEX.
        if let Some(rest) = raw.to_ascii_lowercase().strip_prefix("eli/") {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() >= 3 {
                let typ = match parts[0] {
                    "reg" => Some('R'),
                    "dir" => Some('L'),
                    "dec" => Some('D'),
                    _ => None,
                };
                if let (Some(t), Ok(year), Ok(num)) =
                    (typ, parts[1].parse::<u32>(), parts[2].parse::<u32>())
                {
                    return vec![format!("3{:04}{}{:04}", year, t, num)];
                }
            }
        }

        // 3. Year/number reference: produce candidates across all common
        //    legislation act types, in a sensible probing order.
        if let Some((year, num)) = parse_year_slash_number(raw) {
            // R = Regulation (most common), L = Directive, D = Decision,
            // H = Recommendation, F = Framework decision (older Council
            // pillar-3 acts). The probe is fast (HTTP HEAD-equivalent
            // timing once cached locally) so we can afford 5 attempts.
            return ['R', 'L', 'D', 'H', 'F']
                .iter()
                .map(|t| format!("3{:04}{}{:04}", year, t, num))
                .collect();
        }

        Vec::new()
    }

    /// Convenience for callers that want a single CELEX synchronously
    /// (e.g. tests, or manual fetch of an unambiguous input). Returns
    /// the first candidate. For multi-candidate searches, use
    /// `enumerate_celex_candidates` directly.
    #[allow(dead_code)]
    pub(crate) fn resolve_input_to_celex(input: &str) -> Result<String> {
        let candidates = Self::enumerate_celex_candidates(input);
        candidates
            .into_iter()
            .next()
            .ok_or_else(|| {
                anyhow!(
                    "Non riconosco '{}' come CELEX, ELI o riferimento naturale. \
                     Esempi validi: '32016R0679', '2016/679', 'Direttiva 2014/24', \
                     'eli/reg/2016/679/oj'.",
                    input.trim()
                )
            })
    }

    fn html_url(celex: &str, lang_upper: &str) -> String {
        format!(
            "{BASE}/legal-content/{lang}/TXT/HTML/?uri=CELEX:{celex}",
            lang = lang_upper,
            celex = celex
        )
    }

    fn txt_url(celex: &str, lang_upper: &str) -> String {
        format!(
            "{BASE}/legal-content/{lang}/TXT/?uri=CELEX:{celex}",
            lang = lang_upper,
            celex = celex
        )
    }

    fn all_url(celex: &str, lang_upper: &str) -> String {
        format!(
            "{BASE}/legal-content/{lang}/ALL/?uri=CELEX:{celex}",
            lang = lang_upper,
            celex = celex
        )
    }

    /// Cellar REST endpoint for the same act. Lives on a different
    /// host (`publications.europa.eu`) than the legal-content pages,
    /// so when EUR-Lex's CloudFront-WAF fires an anti-bot challenge
    /// at `eur-lex.europa.eu` we still have a chance of getting the
    /// real content. Cellar 303-redirects to the right HTML
    /// representation when given Accept-Language; reqwest follows
    /// the redirect for us.
    fn cellar_url(celex: &str) -> String {
        format!(
            "https://publications.europa.eu/resource/celex/{celex}",
            celex = celex
        )
    }

    fn canonical_url(celex: &str, lang_upper: &str) -> String {
        format!(
            "{BASE}/legal-content/{lang}/TXT/?uri=CELEX:{celex}",
            lang = lang_upper,
            celex = celex
        )
    }

    /// One language attempt. Tries multiple URL variants because EUR-Lex
    /// serves a different body shape depending on the rendering path —
    /// `/TXT/HTML/` is sometimes near-empty for browser-checks while
    /// `/TXT/` (without HTML segment) returns the full body, and
    /// `/ALL/` is the consolidated rendering with metadata + body.
    /// First variant whose extracted body passes the stub check wins.
    ///
    /// Returns `Ok(Some(...))` on success, `Ok(None)` if all variants
    /// returned a stub or 404, `Err` only on transport/parse failure.
    async fn try_fetch_lang(
        &self,
        celex: &str,
        lang_iso: &str,
    ) -> Result<Option<EurlexFetched>> {
        let lang_upper = lang_iso.to_ascii_uppercase();
        // URL fallback chain. The first three live on
        // `eur-lex.europa.eu` and are sometimes WAF-challenged
        // (CloudFront Bot Control); the fourth (Cellar REST) lives on
        // `publications.europa.eu` and serves as a different-CDN
        // escape hatch. With four chances, a single transient WAF
        // hit on EUR-Lex doesn't kill the whole fetch.
        let urls = [
            Self::txt_url(celex, &lang_upper),
            Self::html_url(celex, &lang_upper),
            Self::all_url(celex, &lang_upper),
            Self::cellar_url(celex),
        ];

        let mut last_status: Option<u16> = None;
        let mut saw_waf_challenge = false;
        for url in &urls {
            tracing::info!("[eurlex] GET {url}");
            let resp = self
                .client
                .get(url)
                .header(
                    reqwest::header::ACCEPT,
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                )
                .header(
                    reqwest::header::ACCEPT_LANGUAGE,
                    format!("{lang_iso},en;q=0.7"),
                )
                .send()
                .await
                .with_context(|| format!("EUR-Lex GET {url}"))?;

            let status = resp.status();
            let final_url = resp.url().clone();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
                .unwrap_or_default();
            tracing::info!(
                "[eurlex] {celex} ({lang_upper}) variant {}: status={} final={} ct={}",
                url.split('/').nth(5).unwrap_or("?"),
                status,
                final_url,
                content_type
            );
            last_status = Some(status.as_u16());

            if status.as_u16() == 404 {
                continue;
            }
            if !status.is_success() {
                tracing::warn!(
                    "[eurlex] {celex} ({lang_upper}): unexpected status {status}, skipping variant"
                );
                continue;
            }

            let html = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("[eurlex] body decode failed for {url}: {e}");
                    continue;
                }
            };
            tracing::info!(
                "[eurlex] {celex} ({lang_upper}): downloaded {} bytes of HTML",
                html.len()
            );

            let extracted = extract_html_body(&html);
            tracing::info!(
                "[eurlex] {celex} ({lang_upper}): extracted text={} chars, title={:?}",
                extracted.text.len(),
                extracted
                    .title
                    .as_deref()
                    .map(|t| &t[..t.len().min(80)])
            );

            // AWS WAF / CloudFront Bot Control challenge detection.
            // The challenge response is a small HTML wrapper around a
            // JS payload that sets a session cookie via crypto-PoW.
            // We can't solve it (no JS engine), but we can recognise
            // it from these inline markers and short-circuit so the
            // user gets a useful error instead of "body too small".
            let html_lower = html.to_ascii_lowercase();
            if html_lower.contains("awswafcookiedomainlist")
                || html_lower.contains("gokuprops")
                || html_lower.contains("aws-waf-token")
            {
                tracing::warn!(
                    "[eurlex] {celex} ({lang_upper}): AWS WAF challenge detected on this variant — \
                     EUR-Lex's CDN flagged us as a bot. Trying next variant (different host = different rule set)."
                );
                saw_waf_challenge = true;
                continue;
            }

            // Stub-page detection by signature phrase across EU languages.
            let lower_text = extracted.text.to_ascii_lowercase();
            let stub_markers = [
                "not available in",
                "non disponibile in",
                "n'est pas disponible",
                "no está disponible",
                "nicht in der gewählten sprache",
                "the requested document does not exist",
                "no documents found",
            ];
            if stub_markers.iter().any(|m| lower_text.contains(m)) {
                tracing::info!(
                    "[eurlex] {celex} ({lang_upper}): stub page on this variant, trying next"
                );
                continue;
            }

            // Threshold tightened from 400 → 2000 chars. A 400-char
            // body sneaks past as a "hit" in search but is then
            // rejected downstream by fetch_celex's 1024-byte guard,
            // surfacing as a misleading "found, then error" UX. Real
            // legal acts in any language run to thousands of chars
            // even for short Court orders; anything below 2000 is
            // either a stub EUR-Lex serves while warming its CDN
            // cache, or a "not available in this language" page that
            // dodged the signature-phrase check.
            if extracted.text.trim().len() < 2000 {
                tracing::info!(
                    "[eurlex] {celex} ({lang_upper}): body too short ({} chars) on this variant, trying next",
                    extracted.text.trim().len()
                );
                continue;
            }

            return Ok(Some(EurlexFetched {
                html,
                text: extracted.text,
                title: extracted.title,
                lang_iso: lang_iso.to_string(),
                source_url: Self::canonical_url(celex, &lang_upper),
            }));
        }

        if saw_waf_challenge {
            tracing::warn!(
                "[eurlex] {celex} ({lang_upper}): every variant served an AWS WAF \
                 anti-bot challenge instead of the act body. The challenge requires \
                 a JS engine to solve, which we don't have. This is a CDN-side \
                 decision and not necessarily about call frequency — it can fire \
                 on a single request."
            );
        } else {
            tracing::info!(
                "[eurlex] {celex} ({lang_upper}): all variants exhausted (last status: {:?})",
                last_status
            );
        }
        Ok(None)
    }

    /// HTML keyword path. Hits EUR-Lex's public `/search.html` and
    /// scrapes the result list. Returns hits or empty (no error) so
    /// the caller can fall through to SPARQL.
    async fn keyword_search_via_html(
        &self,
        query: &str,
        lang: &str,
        lang_upper: &str,
        limit: usize,
    ) -> Result<Vec<CorpusHit>> {
        let url = format!(
            "{BASE}/search.html?lang={lang}&text={q}&scope=EURLEX&type=quick",
            lang = lang,
            q = urlencoding_encode(query),
        );
        tracing::info!("[eurlex] HTML keyword search GET {url}");

        let resp = self
            .client
            .get(&url)
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header(
                reqwest::header::ACCEPT_LANGUAGE,
                format!("{lang},en;q=0.7"),
            )
            .send()
            .await
            .with_context(|| format!("EUR-Lex HTML search GET {url}"))?;

        let status = resp.status();
        let html = resp
            .text()
            .await
            .with_context(|| format!("EUR-Lex HTML search body decode {url}"))?;
        tracing::info!(
            "[eurlex] HTML keyword search status={} {} bytes",
            status,
            html.len()
        );

        if !status.is_success() {
            return Err(anyhow!("EUR-Lex search status {status}"));
        }

        let hits = parse_search_results(&html, lang_upper, limit);
        tracing::info!("[eurlex] HTML keyword search produced {} hits", hits.len());

        // If we got nothing back, log a snippet of the actual response
        // so we can diagnose layout drift without round-trips through
        // the user. Trim to a single line and cap at 1KB.
        if hits.is_empty() {
            let snippet: String = html
                .chars()
                .take(1024)
                .collect::<String>()
                .replace(['\n', '\r', '\t'], " ");
            tracing::info!(
                "[eurlex] HTML response snippet (first 1KB, whitespace collapsed): {}",
                snippet
            );
        }

        Ok(hits)
    }

    /// SPARQL keyword path against Cellar's public endpoint. Used as a
    /// fallback when the HTML search returned 0 hits — EUR-Lex's
    /// search page sometimes serves an empty stub to non-browser
    /// clients, while the SPARQL endpoint serves structured JSON to
    /// anyone with no User-Agent gating.
    ///
    /// Endpoint docs: https://op.europa.eu/en/web/eu-vocabularies/sparql-endpoint
    async fn keyword_search_via_sparql(
        &self,
        query: &str,
        lang: &str,
        lang_upper: &str,
        limit: usize,
    ) -> Result<Vec<CorpusHit>> {
        // Cellar uses Virtuoso so we can use `bif:contains` for
        // full-text search on indexed predicates. If `bif:contains`
        // misses (some predicates aren't indexed for FTS), the query
        // falls through to a REGEX filter on the same title — slower
        // but more permissive.
        //
        // The Italian title is keyed by lang URI; we map ISO-2 → ISO-3
        // so the filter matches Cellar's authority codes.
        let lang_iso3 = match lang.to_ascii_lowercase().as_str() {
            "bg" => "BUL", "cs" => "CES", "da" => "DAN", "de" => "DEU",
            "el" => "ELL", "en" => "ENG", "es" => "SPA", "et" => "EST",
            "fi" => "FIN", "fr" => "FRA", "ga" => "GLE", "hr" => "HRV",
            "hu" => "HUN", "it" => "ITA", "lt" => "LIT", "lv" => "LAV",
            "mt" => "MLT", "nl" => "NLD", "pl" => "POL", "pt" => "POR",
            "ro" => "RON", "sk" => "SLK", "sl" => "SLV", "sv" => "SWE",
            _ => "ENG",
        };

        // Escape any quotes in the query so we don't break the SPARQL.
        let safe_query = query.replace('\\', "").replace('"', "");

        let sparql = format!(
            r#"PREFIX cdm: <http://publications.europa.eu/ontology/cdm#>
PREFIX skos: <http://www.w3.org/2004/02/skos/core#>
PREFIX dcterms: <http://purl.org/dc/terms/>

SELECT DISTINCT ?celex ?title WHERE {{
  ?expr cdm:expression_title ?title .
  FILTER(REGEX(STR(?title), "{q}", "i"))
  ?expr cdm:expression_uses_language <http://publications.europa.eu/resource/authority/language/{lang_iso3}> .
  ?expr cdm:expression_belongs_to_work ?work .
  ?work cdm:resource_legal_id_celex ?celex .
}}
LIMIT {limit}"#,
            q = safe_query,
            lang_iso3 = lang_iso3,
            limit = limit,
        );

        let url = "https://publications.europa.eu/webapi/rdf/sparql";
        tracing::info!(
            "[eurlex] SPARQL search lang={} q={:?} (limit={})",
            lang_iso3,
            safe_query,
            limit
        );

        let resp = self
            .client
            .post(url)
            .header(reqwest::header::ACCEPT, "application/sparql-results+json")
            .form(&[("query", sparql.as_str()), ("format", "application/sparql-results+json")])
            .send()
            .await
            .with_context(|| format!("Cellar SPARQL POST {url}"))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .with_context(|| "Cellar SPARQL body decode")?;
        tracing::info!(
            "[eurlex] SPARQL status={} {} bytes",
            status,
            body.len()
        );

        if !status.is_success() {
            // Log the body so we can diagnose Virtuoso syntax errors etc.
            let snippet: String = body.chars().take(512).collect();
            tracing::warn!("[eurlex] SPARQL error body: {snippet}");
            return Err(anyhow!("Cellar SPARQL status {status}"));
        }

        let hits = parse_sparql_json(&body, lang_upper, limit);
        tracing::info!("[eurlex] SPARQL produced {} hits", hits.len());
        Ok(hits)
    }

    /// Mutate `hits` in place to replace each title with the one we
    /// extract from the CELEX's actual page in the user's preferred
    /// language. Done in parallel — `try_fetch_lang` for N hits in
    /// flight at once. We saw an earlier SPARQL-based localisation
    /// approach hit zero matches due to Cellar ontology drift; this
    /// path doesn't depend on the SPARQL ontology being right.
    ///
    /// Best-effort: fetch failures and stub pages leave the original
    /// (possibly mixed-language) title intact.
    async fn localize_titles(&self, hits: &mut [CorpusHit], lang: &str) {
        if hits.is_empty() {
            return;
        }
        let total = hits.len();
        tracing::info!(
            "[eurlex] localising {} titles via parallel page-probe ({})",
            total,
            lang
        );

        let probes = hits.iter().map(|h| {
            let celex = h.identifier.clone();
            let lang = lang.to_string();
            async move {
                let r = self.try_fetch_lang(&celex, &lang).await;
                (celex, r)
            }
        });
        let results = futures_util::future::join_all(probes).await;

        let mut map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (celex, res) in results {
            if let Ok(Some(fetched)) = res {
                if let Some(t) = fetched.title {
                    if !t.is_empty() {
                        map.insert(celex, t);
                    }
                }
            }
        }

        let mut localised = 0usize;
        for h in hits.iter_mut() {
            if let Some(t) = map.get(&h.identifier) {
                h.title = t.clone();
                localised += 1;
            }
        }
        tracing::info!(
            "[eurlex] localised {}/{} titles to {}",
            localised,
            total,
            lang
        );
    }
}

impl Default for EurlexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

struct EurlexFetched {
    html: String,
    text: String,
    title: Option<String>,
    lang_iso: String,
    source_url: String,
}

/// Result of running the body-extraction selector chain on a CELEX
/// HTML page.
pub(crate) struct ExtractedBody {
    pub text: String,
    pub title: Option<String>,
}

/// Pull the legal-content body out of an EUR-Lex HTML page. EUR-Lex
/// has shipped several layouts over the years; we try a chain of
/// selectors and pick the LONGEST match (longest = most likely to be
/// the real act body, not a sidebar). If all specific selectors strike
/// out, we fall back to whole-body text.
///
/// Public(crate) so tests in `mod tests` below exercise it without
/// hitting the network.
pub(crate) fn extract_html_body(html: &str) -> ExtractedBody {
    let doc = Html::parse_document(html);

    // We try many selectors and keep the longest result. EUR-Lex uses
    // different containers depending on document type and rendering
    // path: ELI-aware acts get `eli-main-content`, older acts use
    // `TexteOnly`, court decisions use yet another layout.
    let selectors = [
        // Modern ELI-aware layout (post-2020).
        "div.eli-main-content",
        // Common act-body wrappers.
        "div#text",
        "div#TexteOnly",
        "div#document-content",
        // ELI subdivisions / preamble.
        "div.eli-container",
        "article.eli-document",
        // Some legal-content pages wrap everything in #document_div.
        "div#document_div",
        // Court of Justice decisions.
        "div.documentDescriptive",
        // Last-resort: whole <main>; will include nav crumbs but
        // gives us something rather than nothing.
        "main",
        "body",
    ];

    let mut best = String::new();
    for sel in selectors {
        let parsed = match Selector::parse(sel) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for el in doc.select(&parsed) {
            let collected: String = el.text().collect::<Vec<_>>().join(" ");
            let trimmed = collapse_whitespace(&collected);
            if trimmed.len() > best.len() {
                best = trimmed;
            }
        }
        // If we got a substantial hit from a specific selector, stop —
        // anything we'd find in `body` would be a superset polluted
        // with navigation, and we already have what we need.
        if !best.is_empty() && sel != "main" && sel != "body" && best.len() > 500 {
            break;
        }
    }

    // Title: prefer the OJ-style "TITLE OF THE ACT" element, fall back
    // to the document <title>.
    let title = [
        "p.eli-main-title",
        "p.oj-doc-ti",
        "h1.eli-main-title",
        "p.title-doc",
        "h1",
        "title",
    ]
    .iter()
    .filter_map(|s| Selector::parse(s).ok())
    .find_map(|sel| {
        doc.select(&sel)
            .next()
            .map(|el| collapse_whitespace(&el.text().collect::<String>()))
            .filter(|s| !s.is_empty() && s.len() < 500)
    });

    ExtractedBody { text: best, title }
}

/// Find a `YYYY/N` pattern anywhere in the string, optionally followed
/// by `/UE` or `/EU`. Returns the parsed `(year, number)` of the first
/// match. Used to extract the document number from natural references
/// like "Regolamento (UE) 2016/679".
pub(crate) fn parse_year_slash_number(s: &str) -> Option<(u32, u32)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Find the next 4-digit sequence.
        if i + 4 <= bytes.len() && bytes[i..i + 4].iter().all(|c| c.is_ascii_digit()) {
            // Make sure it's not part of a longer digit run (e.g. "20160" → not a year).
            let prev_is_digit = i > 0 && bytes[i - 1].is_ascii_digit();
            let next_after_4 = i + 4;
            let after_is_digit = next_after_4 < bytes.len()
                && bytes[next_after_4].is_ascii_digit();
            if !prev_is_digit && !after_is_digit {
                let year_str = &s[i..i + 4];
                if let Ok(year) = year_str.parse::<u32>() {
                    if (1900..=2100).contains(&year) {
                        // Look for "/<digits>" right after.
                        if next_after_4 < bytes.len() && bytes[next_after_4] == b'/' {
                            let mut j = next_after_4 + 1;
                            let num_start = j;
                            while j < bytes.len() && bytes[j].is_ascii_digit() {
                                j += 1;
                            }
                            if j > num_start {
                                if let Ok(num) = s[num_start..j].parse::<u32>() {
                                    return Some((year, num));
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Minimal URL-encoder for the query-string value of a search query.
/// We only have a single non-ASCII payload (the user's query), so a
/// hand-written percent-encoder is cheaper than pulling `urlencoding`
/// or `percent-encoding` as a dep.
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            // RFC 3986 unreserved + a few safe chars commonly left bare
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Extract search hits from an EUR-Lex public-search HTML page.
///
/// EUR-Lex's search result list nests each result inside a `<div>`
/// with the result link pointing to `legal-content/{LANG}/...?uri=CELEX:{id}`.
/// We pull every CELEX-bearing link, dedupe, and pair each with the
/// nearest title/heading text we can find for that result block.
///
/// Robust to layout drift: instead of relying on a single class name
/// (which EUR-Lex changes from time to time), we scrape every `<a>`
/// whose href contains `CELEX:` — the URL pattern is stable.
pub(crate) fn parse_search_results(
    html: &str,
    lang_upper: &str,
    limit: usize,
) -> Vec<CorpusHit> {
    let doc = Html::parse_document(html);
    let link_sel = match Selector::parse("a") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut hits: Vec<CorpusHit> = Vec::new();

    for a in doc.select(&link_sel) {
        let href = match a.value().attr("href") {
            Some(h) => h,
            None => continue,
        };
        // Pull CELEX out of href. Patterns we expect:
        //   /legal-content/IT/TXT/?uri=CELEX:32016R0679
        //   ../legal-content/IT/TXT/?uri=CELEX:32016R0679&from=...
        let celex = match extract_celex_from_href(href) {
            Some(c) => c,
            None => continue,
        };
        if !seen.insert(celex.clone()) {
            continue;
        }

        // Title: prefer the link's own text. Fall back to a parent
        // heading or the first non-empty text-bearing ancestor block.
        let link_text =
            collapse_whitespace(&a.text().collect::<String>());
        let title = if !link_text.is_empty() && link_text.len() < 400 {
            link_text
        } else {
            format!("CELEX {celex}")
        };

        hits.push(CorpusHit {
            identifier: celex.clone(),
            title,
            date: None,
            url: format!(
                "{BASE}/legal-content/{lang}/TXT/?uri=CELEX:{celex}",
                lang = lang_upper,
                celex = celex
            ),
            languages_available: vec![],
        });
        if hits.len() >= limit {
            break;
        }
    }

    hits
}

/// Parse the SPARQL-results JSON envelope returned by Cellar.
///
/// Shape (W3C SPARQL 1.1 Results JSON):
/// ```json
/// {
///   "head": {"vars": ["celex","title"]},
///   "results": {"bindings": [
///     {"celex": {"type":"literal","value":"32016R0679"},
///      "title": {"type":"literal","xml:lang":"it","value":"Regolamento..."}}
///   ]}
/// }
/// ```
pub(crate) fn parse_sparql_json(
    body: &str,
    lang_upper: &str,
    limit: usize,
) -> Vec<CorpusHit> {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[eurlex] SPARQL JSON parse failed: {e}");
            return Vec::new();
        }
    };
    let bindings = match parsed
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array())
    {
        Some(b) => b,
        None => return Vec::new(),
    };

    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut hits: Vec<CorpusHit> = Vec::new();
    for b in bindings {
        let celex = b
            .get("celex")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let title = b
            .get("title")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let Some(celex) = celex else { continue };
        if !seen.insert(celex.clone()) {
            continue;
        }
        hits.push(CorpusHit {
            identifier: celex.clone(),
            title: title.unwrap_or_else(|| format!("CELEX {celex}")),
            date: None,
            url: format!(
                "{BASE}/legal-content/{lang}/TXT/?uri=CELEX:{celex}",
                lang = lang_upper,
                celex = celex
            ),
            languages_available: vec![],
        });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

/// Pull the CELEX value out of a EUR-Lex result-link URL. Handles
/// three patterns we've seen in production:
///   - `?uri=CELEX:32016R0679` (legacy `legal-content` URLs)
///   - `/eli/reg/2016/679/oj` (modern ELI deep-links — these don't
///     contain "CELEX" literally; we convert from ELI segments)
///   - bare CELEX-shaped substring anywhere in the URL — covers any
///     other layout where EUR-Lex includes the CELEX as a path or
///     query segment we don't recognise.
/// Returns `None` if the href doesn't carry an act identifier.
pub(crate) fn extract_celex_from_href(href: &str) -> Option<String> {
    // Pattern 1: explicit CELEX in query string.
    if let Some(i) = href.find("CELEX:") {
        let rest = &href[i + "CELEX:".len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        let celex = &rest[..end];
        if celex.len() >= 7 {
            return Some(celex.to_string());
        }
    }
    // Pattern 2: ELI URL like `/eli/{type}/{year}/{num}/oj` or
    // `/eli/{type}/{year}/{num}`.
    if let Some(idx) = href.find("/eli/") {
        let rest = &href[idx + "/eli/".len()..];
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 3 {
            let typ = match parts[0].to_ascii_lowercase().as_str() {
                "reg" => Some('R'),
                "dir" => Some('L'),
                "dec" => Some('D'),
                "rec" => Some('H'),
                _ => None,
            };
            if let (Some(t), Ok(year), Ok(num)) =
                (typ, parts[1].parse::<u32>(), parts[2].parse::<u32>())
            {
                return Some(format!("3{:04}{}{:04}", year, t, num));
            }
        }
    }
    // Pattern 3: raw CELEX shape. Scan the URL for any 9-12 char
    // alphanumeric run matching {sector}{year:4}{type:1}{number:4+}.
    // We constrain to legislation/court sectors (1, 3, 6, 8) so
    // common false positives (numeric IDs, hashes) don't qualify.
    extract_celex_pattern(href)
}

/// Look for a CELEX-shaped substring inside `s`. The recognised shape
/// is one digit (sector) + 4-digit year + one uppercase letter + at
/// least 3 digits (number — treaty CELEX like `12012E101` has 3,
/// legislation acts like `32016R0679` have 4). Sector restricted to
/// 1, 3, 6, 8 to keep false positives down.
fn extract_celex_pattern(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    // Minimum legal CELEX is 1+4+1+3 = 9 chars; the loop guard uses
    // that as its short-circuit, but we also bounds-check each access
    // since later positions extend dynamically.
    if bytes.len() < 9 {
        return None;
    }
    for i in 0..=bytes.len() - 9 {
        let sector = bytes[i];
        if !matches!(sector, b'1' | b'3' | b'6' | b'8') {
            continue;
        }
        // Require 4-digit year (positions i+1..i+5).
        if !bytes[i + 1..i + 5].iter().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // Require an uppercase ASCII letter at i+5.
        if !bytes[i + 5].is_ascii_uppercase() {
            continue;
        }
        // Walk the trailing digit run.
        let mut j = i + 6;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j - (i + 6) < 3 {
            continue;
        }
        // Reject if preceded by an alphanumeric — we want a clean
        // word boundary so we don't catch tail of a longer token.
        if i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
            continue;
        }
        return Some(s[i..j].to_string());
    }
    None
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim().to_string()
}

#[async_trait]
impl LegalCorpusAdapter for EurlexAdapter {
    fn id(&self) -> &'static str {
        "eurlex"
    }

    fn languages(&self) -> &[&'static str] {
        ALL_LANGS
    }

    async fn search_by_id(
        &self,
        identifier: &str,
        language: Option<&str>,
    ) -> Result<Vec<CorpusHit>> {
        // Enumerate candidate CELEXes from the user's input — for an
        // ambiguous "2014/24" we probe Regulation, Directive, Decision,
        // Recommendation, and Framework variants and return whichever
        // EUR-Lex actually serves. This replaces the old hardcoded
        // keyword→letter mapping with a "ask EUR-Lex" approach.
        let candidates = Self::enumerate_celex_candidates(identifier);
        if candidates.is_empty() {
            return Err(anyhow!(
                "Non riconosco '{}' come CELEX, ELI o riferimento naturale. \
                 Esempi validi: '32016R0679', '2016/679', 'Direttiva 2014/24', \
                 'eli/reg/2016/679/oj'.",
                identifier.trim()
            ));
        }

        let lang = language.unwrap_or("en").to_ascii_lowercase();
        let lang_upper = lang.to_ascii_uppercase();

        // Probe in parallel — for the typical 5-candidate case this
        // turns a 5×network-RTT serial probe into one RTT.
        let probes = candidates.iter().map(|celex| {
            let celex = celex.clone();
            let lang = lang.clone();
            async move {
                let r = self.try_fetch_lang(&celex, &lang).await;
                (celex, r)
            }
        });
        let results = futures_util::future::join_all(probes).await;

        let mut hits: Vec<CorpusHit> = Vec::new();
        for (celex, res) in results {
            match res {
                Ok(Some(fetched)) => {
                    hits.push(CorpusHit {
                        identifier: celex.clone(),
                        title: fetched
                            .title
                            .clone()
                            .unwrap_or_else(|| format!("CELEX {celex}")),
                        date: None,
                        url: Self::canonical_url(&celex, &lang_upper),
                        languages_available: vec![fetched.lang_iso.clone()],
                    });
                }
                Ok(None) => {
                    tracing::debug!(
                        "[eurlex] candidate {celex} not available in {lang}"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[eurlex] probe error for {celex}: {e}"
                    );
                }
            }
        }

        Ok(hits)
    }

    async fn search_by_keyword(
        &self,
        query: &str,
        language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CorpusHit>> {
        // V1 keyword search uses two paths in series:
        //   1. EUR-Lex public search HTML (fast, returns titles)
        //   2. Cellar SPARQL endpoint (structured JSON, no auth) when
        //      HTML returns nothing — handles the case where EUR-Lex
        //      serves a JS-rendered or anti-bot stub to our request.
        // The SOAP CWS would give us richer filters but needs EU Login
        // registration; documented for V2 in EURLEX_REGISTRATION.md.
        let lang = language.unwrap_or("en").to_ascii_lowercase();
        let lang_upper = lang.to_ascii_uppercase();

        let mut html_hits = self
            .keyword_search_via_html(query, &lang, &lang_upper, limit)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("[eurlex] HTML search failed: {e}");
                Vec::new()
            });
        if !html_hits.is_empty() {
            // EUR-Lex's public search page returns titles in mixed
            // languages (English-by-default for many doc types). Run
            // a single SPARQL VALUES query to fetch the localised
            // title for each CELEX in the user's preferred language;
            // fall back to whatever HTML gave us if SPARQL doesn't
            // return one.
            self.localize_titles(&mut html_hits, &lang).await;
            return Ok(html_hits);
        }

        tracing::info!(
            "[eurlex] HTML search returned 0 hits, falling back to SPARQL"
        );
        let sparql_hits = self
            .keyword_search_via_sparql(query, &lang, &lang_upper, limit)
            .await?;
        Ok(sparql_hits)
    }

    async fn fetch(
        &self,
        identifier: &str,
        language: Option<&str>,
        fallback_en: bool,
    ) -> Result<CorpusDocument> {
        // For fetch we expect a concrete identifier: either a clean
        // CELEX (which is what the search-then-pick UI flow gives us)
        // or an unambiguous natural reference. If the user passed
        // something ambiguous they should go through search first.
        let candidates = Self::enumerate_celex_candidates(identifier);
        if candidates.is_empty() {
            return Err(anyhow!(
                "Non riconosco '{}' come CELEX o riferimento valido.",
                identifier.trim()
            ));
        }
        if candidates.len() > 1 {
            return Err(anyhow!(
                "'{}' è ambiguo: corrisponde a più atti possibili ({}). \
                 Usa la ricerca per vedere la lista e scegliere quello giusto.",
                identifier.trim(),
                candidates.join(", ")
            ));
        }
        let celex = candidates.into_iter().next().unwrap();
        let celex = celex.as_str();
        let primary = language
            .map(|l| l.to_ascii_lowercase())
            .unwrap_or_else(|| "en".to_string());

        // Retry-with-backoff wrapper around try_fetch_lang. EUR-Lex
        // sometimes serves a tiny stub on the first hit (CDN cache
        // miss, soft rate limit) and the real content on a follow-up
        // request a few seconds later. Giving up after a single try
        // makes the user click Indicizza-fail-Indicizza-success in
        // a loop. Three attempts at 0/2/5 s caps the worst case at
        // ~7 s of wait without retries running away.
        async fn fetch_with_retry(
            this: &EurlexAdapter,
            celex: &str,
            lang: &str,
        ) -> Result<Option<EurlexFetched>> {
            for (attempt, delay_ms) in [(1u32, 0u64), (2, 2000), (3, 5000)] {
                if delay_ms > 0 {
                    tracing::info!(
                        "[eurlex] {celex} ({}): retry attempt {} in {}ms",
                        lang.to_ascii_uppercase(),
                        attempt,
                        delay_ms
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                if let Some(f) = this.try_fetch_lang(celex, lang).await? {
                    if attempt > 1 {
                        tracing::info!(
                            "[eurlex] {celex} ({}): attempt {} succeeded ({} chars)",
                            lang.to_ascii_uppercase(),
                            attempt,
                            f.text.len()
                        );
                    }
                    return Ok(Some(f));
                }
            }
            Ok(None)
        }

        let try_primary = fetch_with_retry(self, celex, &primary).await?;
        let (fetched, used_fallback) = match try_primary {
            Some(f) => (f, false),
            None => {
                if !fallback_en || primary == "en" {
                    return Err(anyhow!(
                        "EUR-Lex non ha restituito il testo dell'atto {celex} ({primary}) \
                         dopo 3 tentativi su 4 endpoint diversi (legal-content TXT/HTML/ALL \
                         + Cellar REST). I log del backend riportano la natura precisa di \
                         ogni risposta (challenge anti-bot, stub, body troppo corto, 4xx). \
                         L'atto potrebbe non essere disponibile in questa lingua, oppure \
                         il CDN sta servendo un challenge JS che non possiamo risolvere."
                    ));
                }
                tracing::info!(
                    "[eurlex] {celex}: missing in {primary} after 3 attempts, falling back to en"
                );
                let en = fetch_with_retry(self, celex, "en")
                    .await?
                    .ok_or_else(|| {
                        anyhow!(
                            "EUR-Lex non ha restituito il testo dell'atto {celex} né in \
                             {primary} né in English dopo 3 tentativi per lingua su 4 \
                             endpoint diversi. I log riportano la natura precisa di ogni \
                             risposta (challenge anti-bot, stub, body troppo corto, 4xx)."
                        )
                    })?;
                (en, true)
            }
        };

        let title = fetched
            .title
            .clone()
            .unwrap_or_else(|| format!("CELEX {celex}"));

        // We persist the *plain text* — the HTML is only useful for
        // viewer parity with native uploads, and for V1 we'd rather
        // keep storage tight. The chat handler reads `extracted_text_path`
        // anyway, so the in-app viewer for corpus docs is V2 work.
        Ok(CorpusDocument {
            identifier: celex.to_string(),
            title,
            date: None,
            language: fetched.lang_iso.clone(),
            fetched_with_fallback: used_fallback,
            bytes: fetched.text.into_bytes(),
            mime: "text/plain; charset=utf-8",
            source_url: fetched.source_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML: &str = r#"<!doctype html>
        <html><head><title>Regulation (EU) 2016/679</title></head>
        <body>
            <main>
                <h1 class="eli-main-title">Regolamento (UE) 2016/679</h1>
                <div id="text">
                    <p>Articolo 1.&nbsp;
                        Il presente regolamento stabilisce norme...</p>
                    <p>Articolo 2. Ambito di applicazione materiale...</p>
                </div>
            </main>
        </body></html>"#;

    #[test]
    fn extract_picks_text_div() {
        let body = extract_html_body(SAMPLE_HTML);
        assert!(body.text.contains("Articolo 1"));
        assert!(body.text.contains("ambito di applicazione".to_lowercase().as_str())
            || body.text.contains("Ambito di applicazione"));
        assert!(!body.text.is_empty());
    }

    #[test]
    fn extract_picks_title() {
        let body = extract_html_body(SAMPLE_HTML);
        assert_eq!(
            body.title.as_deref(),
            Some("Regolamento (UE) 2016/679")
        );
    }

    #[test]
    fn celex_shape_validation() {
        assert!(EurlexAdapter::looks_like_celex("32016R0679"));
        assert!(EurlexAdapter::looks_like_celex("12012E101"));
        assert!(EurlexAdapter::looks_like_celex("32024R0903"));
        assert!(!EurlexAdapter::looks_like_celex(""));
        assert!(!EurlexAdapter::looks_like_celex("nope"));
        assert!(!EurlexAdapter::looks_like_celex("https://eur-lex.europa.eu/blah"));
        // Natural references should NOT pass the CELEX shape check —
        // they're handled by resolve_input_to_celex instead.
        assert!(!EurlexAdapter::looks_like_celex("2014/24/UE"));
        assert!(!EurlexAdapter::looks_like_celex("Regolamento (UE) 2016/679"));
    }

    #[test]
    fn enumerate_passes_through_celex() {
        assert_eq!(
            EurlexAdapter::enumerate_celex_candidates("32016R0679"),
            vec!["32016R0679".to_string()]
        );
        assert_eq!(
            EurlexAdapter::enumerate_celex_candidates("  32014L0024  "),
            vec!["32014L0024".to_string()]
        );
    }

    #[test]
    fn enumerate_year_number_produces_all_act_types() {
        // The whole point of removing the hardcoded keyword→letter
        // table: a bare year/number now produces every act-type
        // candidate so the prober can ask EUR-Lex which exist.
        let cands = EurlexAdapter::enumerate_celex_candidates("2014/24");
        assert!(cands.contains(&"32014R0024".to_string()));
        assert!(cands.contains(&"32014L0024".to_string()));
        assert!(cands.contains(&"32014D0024".to_string()));
        assert!(cands.contains(&"32014H0024".to_string()));
        assert!(cands.len() >= 4);

        // Same for Italian-natural-language input — type words are
        // ignored at the enumeration stage; the prober is the one
        // that decides which candidates exist.
        let cands_it = EurlexAdapter::enumerate_celex_candidates("Direttiva 2014/24/UE");
        assert!(cands_it.contains(&"32014L0024".to_string()));
        // Includes other types too — the search UI shows only the
        // ones the prober confirms exist.
        assert!(cands_it.len() >= 4);
    }

    #[test]
    fn enumerate_eli_shortcuts() {
        assert_eq!(
            EurlexAdapter::enumerate_celex_candidates("eli/reg/2016/679/oj"),
            vec!["32016R0679".to_string()]
        );
        assert_eq!(
            EurlexAdapter::enumerate_celex_candidates("eli/dir/2014/24/oj"),
            vec!["32014L0024".to_string()]
        );
    }

    #[test]
    fn enumerate_rejects_garbage() {
        assert!(EurlexAdapter::enumerate_celex_candidates("").is_empty());
        assert!(EurlexAdapter::enumerate_celex_candidates("hello world").is_empty());
        assert!(EurlexAdapter::enumerate_celex_candidates("https://eur-lex.europa.eu").is_empty());
    }

    #[test]
    fn extract_celex_from_href_handles_query_string() {
        assert_eq!(
            extract_celex_from_href(
                "/legal-content/IT/TXT/?uri=CELEX:32016R0679"
            ),
            Some("32016R0679".to_string())
        );
        assert_eq!(
            extract_celex_from_href(
                "../legal-content/IT/TXT/?uri=CELEX:32014L0024&from=EN"
            ),
            Some("32014L0024".to_string())
        );
        assert_eq!(
            extract_celex_from_href(
                "https://eur-lex.europa.eu/legal-content/EN/TXT/HTML/?uri=CELEX:32024R0903"
            ),
            Some("32024R0903".to_string())
        );
        // No CELEX → None
        assert_eq!(
            extract_celex_from_href("/about/site-policies.html"),
            None
        );
    }

    #[test]
    fn extract_celex_pattern_finds_raw_celex() {
        // CELEX-shaped substring anywhere in the URL is recognised.
        assert_eq!(
            extract_celex_pattern("/anything/32016R0679-or-other"),
            Some("32016R0679".to_string())
        );
        assert_eq!(
            extract_celex_pattern("https://example.com/path/to/12012E101"),
            Some("12012E101".to_string())
        );
        // No CELEX in the URL → None.
        assert_eq!(extract_celex_pattern("/no/match/here"), None);
        // Sector 4 is not in the allow-list (avoids common false
        // positives from numeric IDs).
        assert_eq!(extract_celex_pattern("path/42016R0679"), None);
    }

    #[test]
    fn parse_sparql_json_extracts_celex_and_title() {
        let body = r#"{
            "head": {"vars": ["celex","title"]},
            "results": {"bindings": [
                {"celex": {"type":"literal","value":"32016R0679"},
                 "title": {"type":"literal","xml:lang":"it","value":"Regolamento (UE) 2016/679"}},
                {"celex": {"type":"literal","value":"32024R0903"},
                 "title": {"type":"literal","xml:lang":"it","value":"Regolamento sull'IA"}}
            ]}
        }"#;
        let hits = parse_sparql_json(body, "IT", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].identifier, "32016R0679");
        assert_eq!(hits[0].title, "Regolamento (UE) 2016/679");
        assert_eq!(hits[1].identifier, "32024R0903");
    }

    #[test]
    fn parse_sparql_json_handles_empty_bindings() {
        let body = r#"{"head":{"vars":[]},"results":{"bindings":[]}}"#;
        assert_eq!(parse_sparql_json(body, "IT", 10).len(), 0);
    }

    #[test]
    fn extract_celex_from_href_handles_eli_pattern() {
        // Modern EUR-Lex links use ELI URLs: /eli/{type}/{year}/{num}/oj
        assert_eq!(
            extract_celex_from_href("/eli/reg/2016/679/oj"),
            Some("32016R0679".to_string())
        );
        assert_eq!(
            extract_celex_from_href("/eli/dir/2014/24/oj"),
            Some("32014L0024".to_string())
        );
        assert_eq!(
            extract_celex_from_href("https://eur-lex.europa.eu/eli/dec/2014/24"),
            Some("32014D0024".to_string())
        );
        // Without /oj is fine, with /it/oj is fine
        assert_eq!(
            extract_celex_from_href("/eli/reg/2016/679/oj/it"),
            Some("32016R0679".to_string())
        );
    }

    #[test]
    fn url_encoder_handles_spaces_and_unicode() {
        assert_eq!(urlencoding_encode("hello"), "hello");
        assert_eq!(urlencoding_encode("hello world"), "hello+world");
        assert_eq!(urlencoding_encode("dati personali"), "dati+personali");
        // UTF-8 byte-by-byte percent encoding
        assert_eq!(urlencoding_encode("città"), "citt%C3%A0");
    }

    #[test]
    fn parse_search_results_extracts_celex_and_title() {
        let html = r#"<!doctype html>
            <html><body>
                <div class="result">
                    <a href="legal-content/IT/TXT/?uri=CELEX:32016R0679">
                        Regolamento (UE) 2016/679 del Parlamento europeo
                    </a>
                </div>
                <div class="result">
                    <a href="../legal-content/IT/TXT/?uri=CELEX:32014L0024&amp;from=IT">
                        Direttiva 2014/24/UE — appalti pubblici
                    </a>
                </div>
                <a href="/about/policies">unrelated link</a>
            </body></html>"#;
        let hits = parse_search_results(html, "IT", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].identifier, "32016R0679");
        assert!(hits[0].title.contains("Regolamento"));
        assert_eq!(hits[1].identifier, "32014L0024");
        assert!(hits[1].title.contains("Direttiva"));
    }

    #[test]
    fn parse_search_results_dedupes_repeated_celex() {
        let html = r#"<a href="?uri=CELEX:32016R0679">first</a>
                      <a href="?uri=CELEX:32016R0679#articolo-1">deep link</a>
                      <a href="?uri=CELEX:32014L0024">other</a>"#;
        let hits = parse_search_results(html, "IT", 10);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().any(|h| h.identifier == "32016R0679"));
        assert!(hits.iter().any(|h| h.identifier == "32014L0024"));
    }

    #[test]
    fn parse_year_number_finds_pattern() {
        assert_eq!(parse_year_slash_number("2016/679"), Some((2016, 679)));
        assert_eq!(
            parse_year_slash_number("Regolamento (UE) 2016/679"),
            Some((2016, 679))
        );
        assert_eq!(
            parse_year_slash_number("Direttiva 2014/24/UE"),
            Some((2014, 24))
        );
        assert_eq!(parse_year_slash_number("nothing here"), None);
        // A standalone 5-digit run shouldn't match as a year.
        assert_eq!(parse_year_slash_number("20161/679"), None);
    }

    #[test]
    fn collapse_whitespace_squeezes_runs() {
        assert_eq!(collapse_whitespace("a   b\n\nc"), "a b c");
        assert_eq!(collapse_whitespace("  leading and trailing  "), "leading and trailing");
        assert_eq!(collapse_whitespace(""), "");
    }

    #[test]
    fn url_builders() {
        assert_eq!(
            EurlexAdapter::html_url("32016R0679", "IT"),
            "https://eur-lex.europa.eu/legal-content/IT/TXT/HTML/?uri=CELEX:32016R0679"
        );
        assert_eq!(
            EurlexAdapter::canonical_url("32016R0679", "EN"),
            "https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX:32016R0679"
        );
    }
}
