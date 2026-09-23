// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The collection form: one HTML page a client fills in.
//!
//! This is the artefact the whole pipeline exists to produce. Several
//! organisations each ask their own questions; the merge step turns them
//! into one schema; this turns that schema into a page a person can
//! actually complete, once, without an account and without installing
//! anything.
//!
//! The page is **self-contained**: no scripts, styles or fonts fetched
//! from anywhere. It is sent to people who will open it from an email, on
//! a machine nobody controls, sometimes offline.
//!
//! Several rules below look arbitrary and are not — they come from a
//! production standard written after two real forms went out:
//!
//! * **No native browser dialogs** — not for confirmation, not for
//!   printing. In a sandboxed frame they are silently blocked: no dialog,
//!   no error, nothing. The destructive action becomes a button that arms
//!   on the first click and acts on the second, so the confirmation lives
//!   in the page and cannot be suppressed.
//! * **The draft is saved in the browser**, never sent anywhere. The
//!   client may fill this in over several sittings, and nothing should
//!   leave their machine until they decide to send it.
//! * **Filling the form sends nothing.** The page says so, in a banner, at
//!   the point where somebody would assume otherwise: they generate a
//!   summary, copy it, and paste it into their reply. A form that looks
//!   like it submitted and did not is worse than a form with no button.

use serde::{Deserialize, Serialize};

/// The schema the merge step produces and this page renders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionForm {
    #[serde(default)]
    pub sections: Vec<FormSection>,
    /// Questions deliberately left out, with the reason. Not rendered:
    /// they are for the broker's review, not the client's page.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormSection {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Which organisations require something in this section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companies: Vec<String>,
    #[serde(default)]
    pub fields: Vec<FormField>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormField {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default)]
    pub has_detail: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub companies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_by: Vec<String>,
    /// A value we know for certain, shown as such and still editable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<Prefill>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefill {
    pub value: String,
    /// Why we believe it. Shown to the client, because a value they did
    /// not type needs to explain itself.
    #[serde(default)]
    pub because: String,
}

/// What surrounds the questions: who is asking, and who to send it back to.
#[derive(Debug, Clone)]
pub struct FormOptions {
    pub title: String,
    /// Shown at the top: what this is for, in one or two sentences.
    pub intro: String,
    /// The person the client replies to. Named, because "the broker" is
    /// not an address.
    pub contact: String,
    /// Key under which the draft is kept in the browser.
    pub storage_key: String,
}

impl Default for FormOptions {
    fn default() -> Self {
        Self {
            title: "Raccolta dati".to_string(),
            intro: String::new(),
            contact: String::new(),
            storage_key: "cove-form".to_string(),
        }
    }
}

/// Renders the form as one self-contained HTML page.
pub fn render_html(form: &CollectionForm, options: &FormOptions) -> String {
    let mut out = String::with_capacity(48 * 1024);
    out.push_str("<!doctype html>\n<html lang=\"it\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!("<title>{}</title>\n", escape(&options.title)));
    out.push_str("<style>\n");
    out.push_str(STYLE);
    out.push_str("</style>\n</head>\n<body>\n");

    out.push_str("<header class=\"top\">\n");
    out.push_str(&format!("<h1>{}</h1>\n", escape(&options.title)));
    if !options.intro.is_empty() {
        out.push_str(&format!("<p class=\"intro\">{}</p>\n", escape(&options.intro)));
    }
    out.push_str("<button type=\"button\" class=\"danger\" data-clear>Cancella tutti i dati inseriti</button>\n");
    out.push_str("</header>\n");

    out.push_str("<div class=\"layout\">\n<nav class=\"index\" aria-label=\"Sezioni\">\n<ol>\n");
    for (i, section) in form.sections.iter().enumerate() {
        out.push_str(&format!(
            "<li><a href=\"#{id}\"><span class=\"dot\" data-dot=\"{id}\"></span><span class=\"num\">{n:02}</span> {title}</a></li>\n",
            id = escape(&section.id),
            n = i + 1,
            title = escape(&section.title)
        ));
    }
    out.push_str("<li><a href=\"#riepilogo\"><span class=\"dot\"></span><span class=\"num\">99</span> Riepilogo e invio</a></li>\n");
    out.push_str("</ol>\n</nav>\n<main>\n");

    for (i, section) in form.sections.iter().enumerate() {
        out.push_str(&format!(
            "<section id=\"{}\" data-section>\n<div class=\"head\"><span class=\"num\">{:02}</span><h2>{}</h2>",
            escape(&section.id),
            i + 1,
            escape(&section.title)
        ));
        if !section.companies.is_empty() {
            out.push_str(&format!(
                "<span class=\"who\">richiesto da: {}</span>",
                escape(&section.companies.join(", "))
            ));
        }
        out.push_str("</div>\n");
        if let Some(note) = &section.note {
            out.push_str(&format!("<p class=\"note\">{}</p>\n", escape(note)));
        }
        out.push_str("<div class=\"fields\">\n");
        for field in &section.fields {
            out.push_str(&render_field(field));
        }
        out.push_str("</div>\n</section>\n");
    }

    out.push_str(&summary_section(options));
    out.push_str("</main>\n</div>\n<script>\n");
    out.push_str(&script(options));
    out.push_str("\n</script>\n</body>\n</html>\n");
    out
}

fn render_field(field: &FormField) -> String {
    let id = escape(&field.id);
    let mut out = String::new();
    out.push_str(&format!("<div class=\"field\" data-field-wrap=\"{id}\">\n"));
    out.push_str(&format!("<label for=\"{id}\">{}", escape(&field.label)));
    if !field.required_by.is_empty() {
        out.push_str(&format!(
            "<span class=\"req\" title=\"obbligatorio per {0}\">obbligatorio</span>",
            escape(&field.required_by.join(", "))
        ));
    }
    if field.prefill.is_some() {
        out.push_str("<span class=\"known\">dato noto</span>");
    }
    if let Some(help) = &field.help {
        out.push_str(&format!("<span class=\"help\">{}</span>", escape(help)));
    }
    out.push_str("</label>\n");

    let value = field
        .prefill
        .as_ref()
        .map(|p| escape(&p.value))
        .unwrap_or_default();
    let label_attr = escape(&field.label);

    match field.kind.as_str() {
        "bool" => {
            out.push_str(&format!(
                "<div class=\"choice\"><label><input type=\"radio\" name=\"{id}\" value=\"sì\" data-field=\"{id}\" data-label=\"{label_attr}\"> sì</label>\
                 <label><input type=\"radio\" name=\"{id}\" value=\"no\" data-field=\"{id}\" data-label=\"{label_attr}\"> no</label></div>\n"
            ));
            if field.has_detail {
                let detail_label = field
                    .detail_label
                    .clone()
                    .unwrap_or_else(|| "Specificare".to_string());
                out.push_str(&format!(
                    "<div class=\"detail\" data-detail-of=\"{id}\" hidden><label for=\"{id}__d\">{}</label>\
                     <textarea id=\"{id}__d\" rows=\"3\" data-field=\"{id}__d\" data-label=\"{} — dettaglio\"></textarea></div>\n",
                    escape(&detail_label),
                    label_attr
                ));
            }
        }
        "textarea" => out.push_str(&format!(
            "<textarea id=\"{id}\" rows=\"4\" data-field=\"{id}\" data-label=\"{label_attr}\">{value}</textarea>\n"
        )),
        "select" => {
            out.push_str(&format!(
                "<select id=\"{id}\" data-field=\"{id}\" data-label=\"{label_attr}\"><option value=\"\"></option>"
            ));
            for option in &field.options {
                let o = escape(option);
                let selected = if o == value { " selected" } else { "" };
                out.push_str(&format!("<option value=\"{o}\"{selected}>{o}</option>"));
            }
            out.push_str("</select>\n");
        }
        "checkgroup" => {
            out.push_str("<div class=\"choice column\">");
            for (i, option) in field.options.iter().enumerate() {
                let o = escape(option);
                out.push_str(&format!(
                    "<label><input type=\"checkbox\" value=\"{o}\" data-field=\"{id}__{i}\" data-label=\"{label_attr}: {o}\"> {o}</label>"
                ));
            }
            out.push_str("</div>\n");
        }
        "table" => {
            out.push_str(&format!("<table class=\"grid\" data-table=\"{id}\"><thead><tr>"));
            for column in &field.columns {
                out.push_str(&format!("<th>{}</th>", escape(column)));
            }
            out.push_str("</tr></thead><tbody>");
            let rows = if field.rows.is_empty() {
                vec![String::new(); 3]
            } else {
                field.rows.clone()
            };
            for (r, row_label) in rows.iter().enumerate() {
                out.push_str("<tr>");
                for (c, column) in field.columns.iter().enumerate() {
                    if c == 0 && !row_label.is_empty() {
                        out.push_str(&format!("<td class=\"rowhead\">{}</td>", escape(row_label)));
                        continue;
                    }
                    out.push_str(&format!(
                        "<td><input type=\"text\" data-field=\"{id}__{r}_{c}\" data-label=\"{label_attr} — {} {}\"></td>",
                        escape(row_label),
                        escape(column)
                    ));
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table>\n");
        }
        // `currency` is text, not a number input: the client types
        // "1.200,50" and a number input would refuse it or silently
        // reinterpret the separators.
        "currency" => out.push_str(&format!(
            "<div class=\"amount\"><span class=\"prefix\">{}</span>\
             <input type=\"text\" inputmode=\"decimal\" id=\"{id}\" value=\"{value}\" data-field=\"{id}\" data-label=\"{label_attr}\" data-money></div>\n",
            escape(field.unit.as_deref().unwrap_or("€"))
        )),
        "number" | "percentage" => out.push_str(&format!(
            "<div class=\"amount\"><input type=\"text\" inputmode=\"numeric\" id=\"{id}\" value=\"{value}\" data-field=\"{id}\" data-label=\"{label_attr}\">\
             <span class=\"suffix\">{}</span></div>\n",
            escape(field.unit.as_deref().unwrap_or(if field.kind == "percentage" { "%" } else { "" }))
        )),
        "date" => out.push_str(&format!(
            "<input type=\"date\" id=\"{id}\" value=\"{value}\" data-field=\"{id}\" data-label=\"{label_attr}\">\n"
        )),
        // `text` and anything a newer schema introduces: a field we do
        // not recognise still has to be answerable.
        _ => out.push_str(&format!(
            "<input type=\"text\" id=\"{id}\" value=\"{value}\" data-field=\"{id}\" data-label=\"{label_attr}\">\n"
        )),
    }

    if let Some(prefill) = &field.prefill {
        if !prefill.because.is_empty() {
            out.push_str(&format!(
                "<p class=\"because\">{}</p>\n",
                escape(&prefill.because)
            ));
        }
    }
    out.push_str("</div>\n");
    out
}

fn summary_section(options: &FormOptions) -> String {
    let contact = if options.contact.is_empty() {
        "al vostro referente".to_string()
    } else {
        escape(&options.contact)
    };
    format!(
        r#"<section id="riepilogo" data-section>
<div class="head"><span class="num">99</span><h2>Riepilogo e invio</h2></div>
<p class="warning"><strong>Importante.</strong> Compilare questo modulo non invia nulla automaticamente.
Per farci avere le risposte: generate il riepilogo, copiatelo e incollatelo nella mail di risposta a {contact}.</p>
<div class="actions">
  <button type="button" data-summary>1. Genera riepilogo</button>
  <button type="button" data-copy disabled>2. Copia testo</button>
  <span class="copied" data-copied hidden>copiato</span>
</div>
<pre id="summary" data-summary-out hidden></pre>
<button type="button" class="danger" data-clear>Cancella tutti i dati inseriti</button>
</section>
"#
    )
}

const STYLE: &str = r#"
:root { --ink:#1c2b2d; --muted:#5a6f70; --line:#d8e0e0; --bg:#fbf9f4; --accent:#0f766e; --danger:#b3261e; }
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--ink); font:16px/1.55 system-ui, -apple-system, "Segoe UI", sans-serif; }
.top { padding:24px 20px 16px; border-bottom:1px solid var(--line); background:#fff; }
.top h1 { margin:0 0 6px; font-size:1.5rem; }
.intro { margin:0 0 12px; color:var(--muted); max-width:70ch; }
.layout { display:flex; align-items:flex-start; gap:24px; padding:20px; }
.index { position:sticky; top:20px; flex:0 0 240px; }
.index ol { list-style:none; margin:0; padding:0; font-size:.9rem; }
.index a { display:flex; align-items:center; gap:8px; padding:6px 8px; color:var(--ink); text-decoration:none; border-radius:6px; }
.index a:hover { background:#fff; }
.dot { width:9px; height:9px; border-radius:50%; border:1.5px solid var(--line); flex:0 0 auto; }
.dot.done { background:var(--accent); border-color:var(--accent); }
.num { color:var(--muted); font-variant-numeric:tabular-nums; }
main { flex:1 1 auto; max-width:900px; }
section { background:#fff; border:1px solid var(--line); border-radius:10px; padding:18px 20px; margin:0 0 18px; }
.head { display:flex; align-items:baseline; gap:10px; flex-wrap:wrap; }
.head h2 { margin:0; font-size:1.15rem; }
.who { color:var(--muted); font-size:.8rem; }
.note { color:var(--muted); margin:6px 0 14px; }
.fields { display:grid; gap:14px; }
.field { display:grid; grid-template-columns:minmax(0,1fr) minmax(0,1fr); gap:6px 16px; align-items:start; }
.field > label { font-weight:500; }
.help { display:block; font-weight:400; color:var(--muted); font-size:.85rem; margin-top:2px; }
.req { margin-left:6px; font-size:.7rem; text-transform:uppercase; letter-spacing:.04em; color:var(--danger); }
.known { margin-left:6px; font-size:.7rem; text-transform:uppercase; letter-spacing:.04em; color:var(--accent); }
.because { grid-column:2; margin:0; color:var(--muted); font-size:.8rem; }
input[type=text], input[type=date], textarea, select { width:100%; padding:7px 9px; border:1px solid var(--line); border-radius:6px; font:inherit; background:#fff; color:inherit; }
textarea { resize:vertical; }
input[type=number]::-webkit-outer-spin-button, input[type=number]::-webkit-inner-spin-button { -webkit-appearance:none; margin:0; }
.choice { display:flex; gap:16px; align-items:center; }
.choice.column { flex-direction:column; align-items:flex-start; gap:4px; }
.choice label { font-weight:400; display:flex; align-items:center; gap:6px; }
.amount { display:flex; align-items:center; gap:6px; }
.prefix, .suffix { color:var(--muted); }
.detail { grid-column:1 / -1; }
.grid { grid-column:1 / -1; width:100%; border-collapse:collapse; font-size:.9rem; }
.grid th, .grid td { border:1px solid var(--line); padding:4px 6px; text-align:left; }
.grid th { background:#f3f6f6; font-weight:500; }
.rowhead { background:#fafcfc; color:var(--muted); }
.warning { background:#fff8e6; border:1px solid #f0d9a0; border-radius:8px; padding:10px 12px; }
.actions { display:flex; align-items:center; gap:10px; margin:14px 0; flex-wrap:wrap; }
button { font:inherit; padding:8px 14px; border-radius:8px; border:1px solid var(--accent); background:var(--accent); color:#fff; cursor:pointer; }
button[disabled] { opacity:.5; cursor:default; }
button.danger { background:var(--danger); border-color:var(--danger); }
.copied { color:var(--accent); font-size:.85rem; }
pre { white-space:pre-wrap; background:#f7faf9; border:1px solid var(--line); border-radius:8px; padding:12px; }
@media (max-width:860px) { .layout { flex-direction:column; } .index { position:static; flex:1 1 auto; } .field { grid-template-columns:1fr; } .because { grid-column:1; } }
"#;

fn script(options: &FormOptions) -> String {
    let key = escape_js(&options.storage_key);
    let title = escape_js(&options.title);
    format!(
        r#"(function () {{
  var KEY = "{key}";
  var TITLE = "{title}";
  var fields = function () {{ return Array.prototype.slice.call(document.querySelectorAll("[data-field]")); }};

  function valueOf(el) {{
    if (el.type === "radio") return el.checked ? el.value : "";
    if (el.type === "checkbox") return el.checked ? "sì" : "";
    return el.value || "";
  }}

  // The draft lives in this browser and nowhere else. A blocked or full
  // storage must not break the form, so every access is guarded.
  function save() {{
    try {{
      var data = {{}};
      fields().forEach(function (el) {{
        var v = valueOf(el);
        if (v) data[el.getAttribute("data-field") + (el.type === "radio" ? "" : "")] = v;
      }});
      localStorage.setItem(KEY, JSON.stringify(data));
    }} catch (e) {{ /* private window, blocked storage: the form still works */ }}
  }}

  function restore() {{
    var data = null;
    try {{ data = JSON.parse(localStorage.getItem(KEY) || "null"); }} catch (e) {{ data = null; }}
    if (!data) return;
    fields().forEach(function (el) {{
      var k = el.getAttribute("data-field");
      if (!(k in data)) return;
      if (el.type === "radio") el.checked = (el.value === data[k]);
      else if (el.type === "checkbox") el.checked = !!data[k];
      else el.value = data[k];
    }});
  }}

  function marks() {{
    document.querySelectorAll("[data-section]").forEach(function (section) {{
      var filled = Array.prototype.slice.call(section.querySelectorAll("[data-field]")).some(function (el) {{
        return valueOf(el) !== "";
      }});
      var dot = document.querySelector('[data-dot="' + section.id + '"]');
      if (dot) dot.classList.toggle("done", filled);
    }});
  }}

  function details() {{
    document.querySelectorAll("[data-detail-of]").forEach(function (box) {{
      var name = box.getAttribute("data-detail-of");
      var yes = document.querySelector('input[name="' + name + '"][value="sì"]');
      box.hidden = !(yes && yes.checked);
    }});
  }}

  document.addEventListener("input", function () {{ save(); marks(); details(); }});
  document.addEventListener("change", function () {{ save(); marks(); details(); }});

  function summary() {{
    var lines = [TITLE, new Array(TITLE.length + 1).join("="), ""];
    document.querySelectorAll("[data-section]").forEach(function (section) {{
      var heading = section.querySelector("h2");
      var rows = [];
      Array.prototype.slice.call(section.querySelectorAll("[data-field]")).forEach(function (el) {{
        var v = valueOf(el);
        if (!v) return;
        rows.push("  " + (el.getAttribute("data-label") || el.getAttribute("data-field")) + ": " + v);
      }});
      if (rows.length) {{
        lines.push(heading ? heading.textContent : section.id);
        lines.push.apply(lines, rows);
        lines.push("");
      }}
    }});
    return lines.join("\n");
  }}

  var out = document.querySelector("[data-summary-out]");
  var copyButton = document.querySelector("[data-copy]");
  document.querySelector("[data-summary]").addEventListener("click", function () {{
    out.textContent = summary();
    out.hidden = false;
    copyButton.disabled = false;
  }});

  copyButton.addEventListener("click", function () {{
    var text = out.textContent || summary();
    var done = function () {{
      var flag = document.querySelector("[data-copied]");
      flag.hidden = false;
      setTimeout(function () {{ flag.hidden = true; }}, 2500);
    }};
    // The modern API needs a secure context; the fallback is what makes
    // this work for someone who opened the page from a file.
    if (navigator.clipboard && window.isSecureContext) {{
      navigator.clipboard.writeText(text).then(done, fallback);
    }} else fallback();
    function fallback() {{
      var area = document.createElement("textarea");
      area.value = text;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      try {{ document.execCommand("copy"); done(); }} catch (e) {{ /* nothing to do */ }}
      document.body.removeChild(area);
    }}
  }});

  // Two taps rather than a browser dialog: the native confirmation is
  // silently blocked in a sandboxed frame — no dialog, no error — so the
  // page would clear everything with no warning at all.
  document.querySelectorAll("[data-clear]").forEach(function (button) {{
    var armed = false, timer = null, original = button.textContent;
    button.addEventListener("click", function () {{
      if (!armed) {{
        armed = true;
        button.textContent = "Sicuro? Clicca di nuovo per cancellare tutto";
        timer = setTimeout(function () {{ armed = false; button.textContent = original; }}, 4000);
        return;
      }}
      clearTimeout(timer);
      try {{ localStorage.removeItem(KEY); }} catch (e) {{ }}
      location.reload();
    }});
  }});

  restore();
  marks();
  details();
}})();"#
    )
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn escape_js(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: &str, label: &str, kind: &str) -> FormField {
        FormField {
            id: id.into(),
            label: label.into(),
            kind: kind.into(),
            ..Default::default()
        }
    }

    fn sample() -> CollectionForm {
        CollectionForm {
            sections: vec![
                FormSection {
                    id: "anagrafica".into(),
                    title: "Chi siete".into(),
                    note: Some("I dati della visura camerale.".into()),
                    companies: vec!["aig".into(), "axa".into()],
                    fields: vec![
                        FormField {
                            help: Some("come in visura".into()),
                            prefill: Some(Prefill {
                                value: "Prova S.r.l.".into(),
                                because: "già nota dalla pratica".into(),
                            }),
                            ..field("ragioneSociale", "Ragione sociale", "text")
                        },
                        FormField {
                            options: vec!["Srl".into(), "SpA".into()],
                            ..field("forma", "Forma giuridica", "select")
                        },
                        FormField {
                            unit: Some("€".into()),
                            required_by: vec!["axa".into()],
                            ..field("fatturato", "Fatturato ultimo esercizio", "currency")
                        },
                        field("costituzione", "Data di costituzione", "date"),
                        field("addetti", "Numero addetti", "number"),
                        FormField {
                            has_detail: true,
                            detail_label: Some("Quali paesi".into()),
                            ..field("estero", "Avete attività all'estero?", "bool")
                        },
                        field("descrizione", "Descrizione dell'attività", "textarea"),
                        FormField {
                            options: vec!["Italia".into(), "UE".into(), "USA".into()],
                            ..field("mercati", "Mercati serviti", "checkgroup")
                        },
                        FormField {
                            columns: vec!["Anno".into(), "Sinistri".into(), "Importo".into()],
                            rows: vec!["2024".into(), "2025".into()],
                            ..field("sinistri", "Storico sinistri", "table")
                        },
                    ],
                },
                FormSection {
                    id: "coperture".into(),
                    title: "Assicurazioni attuali".into(),
                    fields: vec![field("polizza", "Polizza in corso", "text")],
                    ..Default::default()
                },
            ],
            dropped: Vec::new(),
        }
    }

    fn html() -> String {
        render_html(
            &sample(),
            &FormOptions {
                title: "Raccolta dati RC".into(),
                intro: "Serve a quotare la vostra copertura.".into(),
                contact: "Alessandro Finardi".into(),
                storage_key: "prova".into(),
            },
        )
    }

    #[test]
    fn every_field_type_is_rendered() {
        let page = html();
        for id in [
            "ragioneSociale",
            "forma",
            "fatturato",
            "costituzione",
            "addetti",
            "estero",
            "descrizione",
            "mercati",
            "sinistri",
            "polizza",
        ] {
            // The wrapper is the one attribute every field has: a
            // check-group has one input per option, a table one per cell.
            assert!(
                page.contains(&format!("data-field-wrap=\"{id}\"")),
                "{id} is missing from the page"
            );
        }
    }

    #[test]
    fn the_page_is_self_contained() {
        // It is opened from an email, on a machine nobody controls,
        // sometimes with no network at all.
        let page = html();
        assert!(!page.contains("http://"), "external reference");
        assert!(
            !page.contains("https://"),
            "external reference: {:?}",
            page.match_indices("https://").next()
        );
        assert!(!page.contains("<script src"), "external script");
        assert!(!page.contains("@import"), "external stylesheet");
    }

    #[test]
    fn the_blocked_browser_dialogs_are_never_used() {
        // In a sandboxed frame these do nothing at all — no dialog, no
        // error — so a destructive action guarded by confirm() would just
        // happen.
        let page = html();
        for forbidden in ["confirm(", "alert(", "window.print", "window.claude"] {
            assert!(!page.contains(forbidden), "{forbidden} must not be used");
        }
    }

    #[test]
    fn clearing_everything_takes_two_deliberate_clicks() {
        let page = html();
        assert_eq!(
            page.matches("data-clear").count(),
            // Once in the header, once at the end, plus the handler.
            3,
            "the clear button belongs at the top and at the bottom"
        );
        assert!(page.contains("Clicca di nuovo"), "the second click must be asked for");
    }

    #[test]
    fn the_page_says_that_filling_it_sends_nothing() {
        // The failure this prevents: a client fills everything in, closes
        // the tab, and waits for an answer that is never coming.
        let page = html();
        assert!(page.contains("non invia nulla automaticamente"));
        assert!(page.contains("Alessandro Finardi"), "the reply goes to a person");
    }

    #[test]
    fn the_draft_is_kept_in_the_browser_and_survives_blocked_storage() {
        let page = html();
        assert!(page.contains("localStorage.setItem"));
        assert!(
            page.contains("catch (e)"),
            "a private window must not break the form"
        );
    }

    #[test]
    fn a_conditional_detail_is_hidden_until_the_answer_is_yes() {
        let page = html();
        assert!(page.contains("data-detail-of=\"estero\""));
        assert!(page.contains("Quali paesi"));
        assert!(page.contains("hidden>"), "it starts hidden");
    }

    #[test]
    fn a_known_value_is_shown_as_known_and_explains_itself() {
        let page = html();
        assert!(page.contains("dato noto"));
        assert!(page.contains("già nota dalla pratica"));
        assert!(page.contains("value=\"Prova S.r.l.\""), "and it is editable");
    }

    #[test]
    fn who_asks_for_what_is_visible() {
        // The reason the client answers once instead of three times.
        let page = html();
        assert!(page.contains("richiesto da: aig, axa"));
        assert!(page.contains("obbligatorio"));
    }

    #[test]
    fn a_money_field_is_not_a_number_input() {
        // A number input refuses "1.200,50" or silently reinterprets the
        // separators, which is worse.
        let page = html();
        assert!(page.contains("data-money"));
        assert!(!page.contains("type=\"number\""));
    }

    #[test]
    fn text_from_the_schema_cannot_inject_markup() {
        let mut form = sample();
        form.sections[0].fields[0].label = "<script>alert(1)</script>".into();
        form.sections[0].fields[0].help = Some("\" onmouseover=\"steal()".into());
        let page = render_html(&form, &FormOptions::default());
        assert!(!page.contains("<script>alert(1)"), "the label was not escaped");
        assert!(page.contains("&lt;script&gt;"));
        assert!(!page.contains("onmouseover=\"steal()"));
    }

    #[test]
    fn the_side_index_lists_every_section_and_the_summary() {
        let page = html();
        assert!(page.contains("href=\"#anagrafica\""));
        assert!(page.contains("href=\"#coperture\""));
        assert!(page.contains("href=\"#riepilogo\""));
        // Always expanded: an accordion hides half the questions from
        // someone who is trying to see how much is left.
        assert!(!page.contains("<details"));
    }

    #[test]
    fn an_unknown_field_type_is_still_answerable() {
        // A newer schema may introduce a type this build has never seen;
        // rendering nothing would silently drop a question.
        let form = CollectionForm {
            sections: vec![FormSection {
                id: "s".into(),
                title: "S".into(),
                fields: vec![field("nuovo", "Campo nuovo", "qualcosa_di_nuovo")],
                ..Default::default()
            }],
            dropped: Vec::new(),
        };
        let page = render_html(&form, &FormOptions::default());
        assert!(page.contains("data-field=\"nuovo\""));
        assert!(page.contains("Campo nuovo"));
    }
}
