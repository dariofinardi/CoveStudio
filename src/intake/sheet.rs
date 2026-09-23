// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! The same form as a spreadsheet, and the answers back from one.
//!
//! The HTML page is what a client should get. The spreadsheet is for the
//! cases where that is not possible: a client who wants to circulate the
//! questions internally, a broker who fills them in during a phone call, a
//! file that has to be archived beside the policy.
//!
//! The two directions are one file: the sheet the client receives is the
//! sheet they send back, with the answers column filled. Nothing else
//! about it may be relied upon — people insert rows, reorder them, and
//! rename the sheet — so reading it back keys on the **field id** in the
//! first column, not on position.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use rust_xlsxwriter::{Format, Workbook};

use super::form::CollectionForm;

/// Column order of the sheet. The id comes first because it is the only
/// thing the reader trusts.
const HEADERS: [&str; 7] = [
    "id",
    "Sezione",
    "Domanda",
    "Dove si trova / note",
    "Tipo",
    "Richiesto da",
    "Risposta",
];

/// The answers column, 0-based. Named because a reader that counts to six
/// in three places will eventually count to five in one of them.
const ANSWER_COLUMN: u16 = 6;

/// Writes the form as a spreadsheet to fill in.
pub fn to_xlsx(form: &CollectionForm) -> Result<Vec<u8>> {
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Raccolta dati")?;

    let header = Format::new().set_bold().set_background_color("#EEF2F2");
    let section = Format::new().set_bold();
    let wrapped = Format::new().set_text_wrap();
    let answer = Format::new().set_background_color("#FFFDF5");

    for (i, title) in HEADERS.iter().enumerate() {
        sheet.write_string_with_format(0, i as u16, *title, &header)?;
    }
    sheet.set_column_width(0, 18)?;
    sheet.set_column_width(1, 20)?;
    sheet.set_column_width(2, 52)?;
    sheet.set_column_width(3, 40)?;
    sheet.set_column_width(4, 12)?;
    sheet.set_column_width(5, 16)?;
    sheet.set_column_width(6, 34)?;
    // The header stays visible: these sheets run to a hundred rows and
    // nobody remembers which column is which at row 80.
    sheet.set_freeze_panes(1, 0)?;

    let mut row = 1u32;
    for form_section in &form.sections {
        // A section heading row, so the sheet reads like the form rather
        // than like a database dump.
        sheet.write_string_with_format(row, 1, &form_section.title, &section)?;
        if let Some(note) = &form_section.note {
            sheet.write_string_with_format(row, 2, note, &wrapped)?;
        }
        row += 1;
        for field in &form_section.fields {
            sheet.write_string(row, 0, &field.id)?;
            sheet.write_string(row, 1, &form_section.title)?;
            sheet.write_string_with_format(row, 2, &field.label, &wrapped)?;
            if let Some(help) = &field.help {
                sheet.write_string_with_format(row, 3, help, &wrapped)?;
            }
            sheet.write_string(row, 4, &field.kind)?;
            if !field.companies.is_empty() {
                sheet.write_string(row, 5, field.companies.join(", "))?;
            }
            // A known value is written into the answer column, where the
            // client can correct it — not into a column of its own that
            // nobody would read.
            match &field.prefill {
                Some(prefill) => {
                    sheet.write_string_with_format(row, ANSWER_COLUMN, &prefill.value, &answer)?
                }
                None => sheet.write_blank(row, ANSWER_COLUMN, &answer)?,
            };
            row += 1;
        }
    }
    Ok(workbook.save_to_buffer()?)
}

/// Reads the answers back out of a filled-in sheet.
///
/// Keys on the id column, so a sheet whose rows were reordered, or which
/// grew a column somebody added for their own notes, still reads. A row
/// with no id is somebody's heading or comment and is skipped.
pub fn answers_from_xlsx(bytes: &[u8]) -> Result<BTreeMap<String, String>> {
    use calamine::{Data, Reader};

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut workbook = calamine::Xlsx::new(cursor)
        .map_err(|e| anyhow!("this file is not a readable spreadsheet: {e}"))?;
    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("the spreadsheet has no sheets"))?;
    let range = workbook
        .worksheet_range(&name)
        .map_err(|e| anyhow!("reading the sheet {name}: {e}"))?;

    // Find the answer column by its heading rather than trusting its
    // position: a column inserted for internal notes must not silently
    // shift what we read as answers.
    let mut answer_column = ANSWER_COLUMN as usize;
    if let Some(first) = range.rows().next() {
        if let Some(found) = first.iter().position(|c| match c {
            Data::String(s) => s.trim().eq_ignore_ascii_case("risposta"),
            _ => false,
        }) {
            answer_column = found;
        }
    }

    let mut out = BTreeMap::new();
    for row in range.rows().skip(1) {
        let Some(Data::String(id)) = row.first() else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        let Some(cell) = row.get(answer_column) else {
            continue;
        };
        let value = match cell {
            Data::Empty => String::new(),
            Data::String(s) => s.trim().to_string(),
            Data::Float(f) => {
                // A whole number typed into a spreadsheet arrives as a
                // float; writing "1200" back as "1200.0" would look like
                // a different answer to whoever reads it.
                if (f.fract()).abs() < f64::EPSILON {
                    format!("{}", *f as i64)
                } else {
                    f.to_string()
                }
            }
            Data::Int(i) => i.to_string(),
            Data::Bool(b) => if *b { "sì" } else { "no" }.to_string(),
            Data::DateTime(d) => d.to_string(),
            other => other.to_string(),
        };
        if !value.is_empty() {
            out.insert(id.to_string(), value);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intake::form::{FormField, FormSection, Prefill};

    fn form() -> CollectionForm {
        CollectionForm {
            sections: vec![FormSection {
                id: "anagrafica".into(),
                title: "Chi siete".into(),
                note: Some("Dalla visura camerale.".into()),
                companies: vec!["aig".into()],
                fields: vec![
                    FormField {
                        id: "ragioneSociale".into(),
                        label: "Ragione sociale".into(),
                        kind: "text".into(),
                        help: Some("come in visura".into()),
                        companies: vec!["aig".into(), "axa".into()],
                        prefill: Some(Prefill {
                            value: "Prova S.r.l.".into(),
                            because: "nota".into(),
                        }),
                        ..Default::default()
                    },
                    FormField {
                        id: "addetti".into(),
                        label: "Numero addetti".into(),
                        kind: "number".into(),
                        companies: vec!["axa".into()],
                        ..Default::default()
                    },
                ],
            }],
            dropped: Vec::new(),
        }
    }

    #[test]
    fn the_sheet_carries_the_questions_and_who_asks_them() {
        let bytes = to_xlsx(&form()).unwrap();
        let answers = answers_from_xlsx(&bytes).unwrap();
        // Only the known value is filled in; the rest is for the client.
        assert_eq!(answers.get("ragioneSociale").map(String::as_str), Some("Prova S.r.l."));
        assert_eq!(answers.get("addetti"), None);
    }

    #[test]
    fn answers_come_back_keyed_by_field_id() {
        // Simulates the client filling the sheet in and sending it back.
        let filled = {
            let mut workbook = Workbook::new();
            let sheet = workbook.add_worksheet();
            for (i, title) in HEADERS.iter().enumerate() {
                sheet.write_string(0, i as u16, *title).unwrap();
            }
            sheet.write_string(1, 0, "ragioneSociale").unwrap();
            sheet.write_string(1, ANSWER_COLUMN, "Oxygen S.r.l.").unwrap();
            sheet.write_string(2, 0, "addetti").unwrap();
            sheet.write_number(2, ANSWER_COLUMN, 12.0).unwrap();
            workbook.save_to_buffer().unwrap()
        };
        let answers = answers_from_xlsx(&filled).unwrap();
        assert_eq!(answers["ragioneSociale"], "Oxygen S.r.l.");
        assert_eq!(answers["addetti"], "12", "a whole number is not 12.0");
    }

    #[test]
    fn a_reordered_sheet_with_an_extra_column_still_reads() {
        // People reorder rows and add columns for their own notes. What
        // must survive is the pairing of id and answer.
        let filled = {
            let mut workbook = Workbook::new();
            let sheet = workbook.add_worksheet();
            sheet.write_string(0, 0, "id").unwrap();
            sheet.write_string(0, 1, "Nota interna").unwrap();
            sheet.write_string(0, 2, "Domanda").unwrap();
            sheet.write_string(0, 3, "Risposta").unwrap();
            sheet.write_string(1, 0, "addetti").unwrap();
            sheet.write_string(1, 3, "12").unwrap();
            sheet.write_string(2, 0, "ragioneSociale").unwrap();
            sheet.write_string(2, 3, "Oxygen S.r.l.").unwrap();
            workbook.save_to_buffer().unwrap()
        };
        let answers = answers_from_xlsx(&filled).unwrap();
        assert_eq!(answers["addetti"], "12");
        assert_eq!(answers["ragioneSociale"], "Oxygen S.r.l.");
    }

    #[test]
    fn rows_without_an_id_are_not_answers() {
        // Section headings, and whatever somebody types at the bottom.
        let bytes = to_xlsx(&form()).unwrap();
        let answers = answers_from_xlsx(&bytes).unwrap();
        assert!(
            !answers.keys().any(|k| k == "Chi siete"),
            "a section heading is not a field: {answers:?}"
        );
    }

    #[test]
    fn a_file_that_is_not_a_spreadsheet_is_refused_with_a_reason() {
        let err = answers_from_xlsx(b"not a spreadsheet").unwrap_err().to_string();
        assert!(err.contains("spreadsheet"), "{err}");
    }
}
