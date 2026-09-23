//! Writes the three artefacts of the collection pipeline to disk, so a
//! person can open them and judge them.
//!
//! Ignored by default: it produces files rather than asserting things,
//! and a test suite should not leave output behind. Run it when you want
//! to *see* the result:
//!
//! ```powershell
//! cargo test --features pdf,rag,ner-pii --test intake_artifacts -- --ignored --nocapture
//! ```
//!
//! The schema below is synthetic. It is shaped like a real underwriting
//! form — every field type, conditional details, provenance per insurer —
//! and contains nobody's data.

use cove_studio::intake::form::{CollectionForm, FormField, FormOptions, FormSection, Prefill};
use cove_studio::intake::report::{self, Answers};
use cove_studio::intake::sheet;

fn field(id: &str, label: &str, kind: &str, companies: &[&str]) -> FormField {
    FormField {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        companies: companies.iter().map(|c| c.to_string()).collect(),
        ..Default::default()
    }
}

fn schema() -> CollectionForm {
    CollectionForm {
        sections: vec![
            FormSection {
                id: "anagrafica".into(),
                title: "Chi siete".into(),
                note: Some(
                    "Le informazioni di base: le trovate quasi tutte nella visura camerale."
                        .into(),
                ),
                companies: vec!["aig".into(), "axa".into(), "chubb".into()],
                fields: vec![
                    FormField {
                        help: Some("come risulta in visura".into()),
                        required_by: vec!["axa".into()],
                        prefill: Some(Prefill {
                            value: "Azienda Esempio S.r.l.".into(),
                            because: "già indicata nella richiesta di quotazione".into(),
                        }),
                        ..field("ragioneSociale", "Ragione sociale", "text", &["aig", "axa", "chubb"])
                    },
                    FormField {
                        options: vec!["Srl".into(), "SpA".into(), "Società di persone".into()],
                        ..field("forma", "Forma giuridica", "select", &["aig", "axa"])
                    },
                    FormField {
                        help: Some("il codice che classifica l'attività, in visura".into()),
                        ..field("ateco", "Codice ATECO", "text", &["axa"])
                    },
                    field("costituzione", "Data di costituzione", "date", &["aig", "axa"]),
                    FormField {
                        help: Some("la somma dei lordi annui, dal consulente del lavoro".into()),
                        unit: Some("€".into()),
                        required_by: vec!["axa".into()],
                        ..field("retribuzioni", "Retribuzioni lorde annue", "currency", &["axa"])
                    },
                    field("addetti", "Numero di addetti", "number", &["aig", "axa", "chubb"]),
                ],
            },
            FormSection {
                id: "produzione".into(),
                title: "Cosa producete".into(),
                note: Some("Serve a capire il rischio dei vostri prodotti.".into()),
                companies: vec!["axa".into(), "chubb".into()],
                fields: vec![
                    field("descrizione", "Descrizione dell'attività", "textarea", &["axa", "chubb"]),
                    FormField {
                        options: vec![
                            "Italia".into(),
                            "Unione Europea".into(),
                            "USA e Canada".into(),
                            "Resto del mondo".into(),
                        ],
                        ..field("mercati", "Mercati serviti", "checkgroup", &["chubb"])
                    },
                    FormField {
                        unit: Some("%".into()),
                        ..field("quotaExport", "Quota di fatturato esportata", "percentage", &["chubb"])
                    },
                    FormField {
                        has_detail: true,
                        detail_label: Some("Quali prodotti e in quali paesi".into()),
                        ..field("ritiri", "Avete mai ritirato un prodotto dal mercato?", "bool", &["axa", "chubb"])
                    },
                ],
            },
            FormSection {
                id: "sinistri".into(),
                title: "Assicurazioni attuali e danni passati".into(),
                note: None,
                companies: vec!["aig".into(), "axa".into(), "chubb".into()],
                fields: vec![
                    FormField {
                        columns: vec!["Anno".into(), "Numero sinistri".into(), "Importo pagato".into()],
                        rows: vec!["2023".into(), "2024".into(), "2025".into()],
                        ..field("storico", "Storico sinistri degli ultimi tre anni", "table", &["aig", "axa", "chubb"])
                    },
                    FormField {
                        has_detail: true,
                        detail_label: Some("Quale compagnia e per quale motivo".into()),
                        ..field("disdette", "Vi è mai stata disdetta una polizza?", "bool", &["aig"])
                    },
                ],
            },
        ],
        dropped: Vec::new(),
    }
}

fn answers() -> Answers {
    Answers::from([
        ("ragioneSociale".to_string(), "Azienda Esempio S.r.l.".to_string()),
        ("forma".to_string(), "Srl".to_string()),
        ("addetti".to_string(), "42".to_string()),
        ("descrizione".to_string(), "test".to_string()),
        ("mercati".to_string(), "Italia, Unione Europea".to_string()),
        ("ritiri".to_string(), "no".to_string()),
    ])
}

#[test]
#[ignore = "writes files for a person to look at"]
fn write_the_three_artefacts() {
    let out = std::path::Path::new(
        &std::env::var("INTAKE_OUT").unwrap_or_else(|_| "dist/esempi-modulo".to_string()),
    )
    .to_path_buf();
    std::fs::create_dir_all(&out).expect("create the output directory");

    let form = schema();
    let given = answers();

    let html = cove_studio::intake::form::render_html(
        &form,
        &FormOptions {
            title: "Raccolta dati — RC Terzi, Prodotti e Malattie professionali".into(),
            intro: "Queste domande coprono i questionari di tre compagnie. \
                    Rispondete una volta sola: al resto pensiamo noi."
                .into(),
            contact: "il vostro referente".into(),
            storage_key: "esempio-rc".into(),
        },
    );
    std::fs::write(out.join("modulo.html"), &html).expect("write the form");

    let xlsx = sheet::to_xlsx(&form).expect("write the sheet");
    std::fs::write(out.join("modulo.xlsx"), &xlsx).expect("save the sheet");

    for company in ["aig", "axa", "chubb"] {
        let doc = report::company_document(
            &form,
            &given,
            company,
            &format!("Questionario {} — risposte raccolte", company.to_uppercase()),
        );
        let bytes = report::to_docx(&doc).expect("write the document");
        std::fs::write(out.join(format!("questionario-{company}.docx")), &bytes)
            .expect("save the document");
    }

    let findings = report::check(&form, &given);
    let gaps = report::gaps_document(&findings, "Verifica prima dell'invio");
    std::fs::write(
        out.join("lacune.docx"),
        report::to_docx(&gaps).expect("write the gaps document"),
    )
    .expect("save the gaps document");

    println!("\nScritti in {}:", out.display());
    println!("  modulo.html            {} KB — la pagina che compila il cliente", html.len() / 1024);
    println!("  modulo.xlsx            {} KB — le stesse domande come foglio", xlsx.len() / 1024);
    println!("  questionario-*.docx    tre documenti, uno per compagnia, con le risposte");
    println!("  lacune.docx            cosa manca e cosa non torna, per il broker");
    println!("\n{} rilievi nella verifica:", findings.len());
    for finding in &findings {
        println!("  - {} ({})", finding.label, finding.detail);
    }
}
