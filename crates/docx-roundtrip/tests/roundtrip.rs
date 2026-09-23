//! Open → write → open again: what must survive the trip.
//!
//! The contract this crate can honestly promise is not "the same bytes".
//! Attribute order, relationship ids and namespace declarations become
//! ours the moment a document is written again. What must hold is:
//!
//! * everything the model represents comes back the same;
//! * everything it does not represent comes back as the XML it was;
//! * a template is still a template — placeholders that were not filled
//!   are still placeholders.
//!
//! Each test builds its own `.docx` in memory, so the suite runs anywhere
//! and a failure points at a specific piece of structure rather than at a
//! fixture nobody can open.

use std::io::Write;

use docx_roundtrip::model::{Block, Inline};

/// A minimal but valid `.docx` around the given body XML.
fn docx(body: &str) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        zip.start_file("[Content_Types].xml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#).unwrap();
        zip.start_file("word/document.xml", opts).unwrap();
        let doc = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:v="urn:schemas-microsoft-com:vml"><w:body>{body}</w:body></w:document>"#
        );
        zip.write_all(doc.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf.into_inner()
}

/// Opens, writes, opens again.
fn round_trip(body: &str) -> (docx_roundtrip::Opened, docx_roundtrip::Opened) {
    let first = docx_roundtrip::open(&docx(body)).expect("first open");
    let assets = docx_roundtrip::Assets::from_opened(&first);
    let bytes = docx_roundtrip::write(&first.document, &assets).expect("write");
    let second = docx_roundtrip::open(&bytes).expect("second open");
    (first, second)
}

#[test]
fn text_and_its_formatting_survive() {
    let (_, after) = round_trip(
        r#"<w:p><w:r><w:rPr><w:b/><w:sz w:val="28"/><w:color w:val="C00000"/></w:rPr><w:t>Contratto</w:t></w:r></w:p>"#,
    );
    let Block::Paragraph { runs, .. } = &after.document.sections[0].body[0] else {
        panic!("expected a paragraph");
    };
    let Inline::Text { text, props, .. } = &runs[0] else {
        panic!("expected text, got {:?}", runs[0]);
    };
    assert_eq!(text, "Contratto");
    let props = props.as_ref().expect("the formatting is part of the text");
    assert_eq!(props.bold, Some(true));
    assert_eq!(props.size, Some(28));
    assert_eq!(props.color.as_deref(), Some("C00000"));
}

#[test]
fn spaces_between_runs_are_not_swallowed() {
    // Word writes the space between two words as its own run; losing it
    // welds the words together, and nobody notices until a client does.
    let (_, after) = round_trip(
        r#"<w:p><w:r><w:t>Spett.le</w:t></w:r><w:r><w:t xml:space="preserve"> </w:t></w:r><w:r><w:t>Cliente</w:t></w:r></w:p>"#,
    );
    assert_eq!(after.document.text(), "Spett.le Cliente");
}

#[test]
fn what_the_model_does_not_understand_comes_back_verbatim() {
    // The promise the whole crate rests on.
    let original = r#"<w:p><w:r><w:pict><v:shape id="forma" style="width:120pt"><v:textbox><w:txbxContent><w:p><w:r><w:t>dentro la casella</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#;
    let (before, after) = round_trip(original);

    assert_eq!(before.opaque_count, 1, "the text box must be kept");
    assert_eq!(after.opaque_count, 1, "and still be there after writing");

    let kept_before = before.document.opaque.values().next().unwrap();
    let kept_after = after.document.opaque.values().next().unwrap();
    assert_eq!(kept_before.kind, "w:pict");
    assert_eq!(
        kept_before.xml, kept_after.xml,
        "the fragment must not be rewritten"
    );
    assert!(kept_after.xml.contains("dentro la casella"));
    assert!(
        kept_after.in_run,
        "it belongs inside a run, and must be put back in one"
    );
}

#[test]
fn an_unfilled_placeholder_is_still_a_placeholder() {
    // A template that consumes its own fields on the first save is worse
    // than no template at all.
    let (before, after) = round_trip(
        r#"<w:p><w:r><w:t>Spett.le {{cliente}}, oggetto: {{oggetto}}</w:t></w:r></w:p>"#,
    );
    assert_eq!(before.placeholders, ["cliente", "oggetto"]);
    assert_eq!(after.placeholders, ["cliente", "oggetto"]);
}

#[test]
fn a_filled_placeholder_becomes_its_value() {
    let bytes = docx(r#"<w:p><w:r><w:t>Spett.le {{cliente}}</w:t></w:r></w:p>"#);
    let mut opened = docx_roundtrip::open(&bytes).unwrap();
    let id = opened
        .document
        .fields
        .iter()
        .find(|(_, f)| f.key.as_deref() == Some("cliente"))
        .map(|(id, _)| id.clone())
        .unwrap();
    opened.document.fields.get_mut(&id).unwrap().value = "Oxygen S.r.l.".into();

    let assets = docx_roundtrip::Assets::from_opened(&opened);
    let written = docx_roundtrip::write(&opened.document, &assets).unwrap();
    let after = docx_roundtrip::open(&written).unwrap();
    assert_eq!(after.document.text(), "Spett.le Oxygen S.r.l.");
    assert!(
        after.placeholders.is_empty(),
        "a filled field is no longer a placeholder: {:?}",
        after.placeholders
    );
}

#[test]
fn a_table_keeps_its_shape() {
    let (_, after) = round_trip(
        r#"<w:tbl>
            <w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/></w:tblGrid>
            <w:tr><w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:tcPr><w:shd w:fill="EEEEEE"/></w:tcPr><w:p><w:r><w:t>Voce</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>Importo</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr>
                <w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>Canone</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>1.200</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr>
                <w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>
                <w:tc><w:p><w:r><w:t>1.250</w:t></w:r></w:p></w:tc></w:tr>
        </w:tbl>"#,
    );
    let Block::Table { rows, props, .. } = &after.document.sections[0].body[0] else {
        panic!("expected a table, got {:?}", after.document.sections[0].body[0]);
    };
    assert_eq!(props.grid, vec![2400, 2400]);
    assert_eq!(rows.len(), 3);
    assert!(rows[0].props.as_ref().unwrap().header, "the header row repeats");
    assert_eq!(
        rows[0].cells[0].props.as_ref().unwrap().shading.as_deref(),
        Some("EEEEEE")
    );
    assert_eq!(
        rows[1].cells[0].props.as_ref().unwrap().rowspan,
        Some(2),
        "the vertical merge survives"
    );
}

#[test]
fn the_page_setup_survives() {
    let (_, after) = round_trip(
        r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1000" w:right="800" w:bottom="1000" w:left="800" w:header="400" w:footer="400"/></w:sectPr>"#,
    );
    let page = &after.document.page;
    assert_eq!((page.width, page.height), (16838, 11906));
    assert_eq!(page.orientation, docx_roundtrip::model::Orientation::Landscape);
    assert_eq!(page.margins.left, 800);
    assert_eq!(page.margins.header, 400);
}

#[test]
fn a_page_break_stays_one_page_break() {
    // Re-saving must not add an empty paragraph each time.
    let (_, after) = round_trip(
        r#"<w:p><w:r><w:t>prima</w:t></w:r></w:p><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:r><w:t>dopo</w:t></w:r></w:p>"#,
    );
    let body = &after.document.sections[0].body;
    let breaks = body.iter().filter(|b| matches!(b, Block::PageBreak)).count();
    assert_eq!(breaks, 1, "{body:?}");
    assert_eq!(body.len(), 3, "no empty paragraph was added: {body:?}");
}

#[test]
fn a_tracked_deletion_is_not_quietly_accepted() {
    let (_, after) = round_trip(
        r#"<w:p><w:r><w:t>resta </w:t></w:r><w:del w:author="Rossi" w:date="2026-09-01T10:00:00Z"><w:r><w:delText>tolto</w:delText></w:r></w:del></w:p>"#,
    );
    let Block::Paragraph { runs, .. } = &after.document.sections[0].body[0] else {
        panic!("expected a paragraph");
    };
    let deleted = runs.iter().find_map(|r| match r {
        Inline::Text { text, props, .. } if props.as_ref().is_some_and(|p| p.del.is_some()) => {
            Some((text.clone(), props.as_ref().unwrap().del.clone().unwrap()))
        }
        _ => None,
    });
    let (text, revision) = deleted.expect("the deletion must still be there to accept or reject");
    assert_eq!(text, "tolto");
    assert_eq!(revision.author, "Rossi");
}

#[test]
fn styles_keep_their_names_and_their_inheritance() {
    let bytes = {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("word/document.xml", opts).unwrap();
            zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Citazione"/></w:pPr><w:r><w:t>testo</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
            zip.start_file("word/styles.xml", opts).unwrap();
            zip.write_all(br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Normale"><w:name w:val="Normal"/><w:rPr><w:rFonts w:ascii="Garamond"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Citazione"><w:name w:val="Citazione"/><w:basedOn w:val="Normale"/><w:rPr><w:i/></w:rPr></w:style></w:styles>"#).unwrap();
            zip.finish().unwrap();
        }
        buf.into_inner()
    };
    let first = docx_roundtrip::open(&bytes).unwrap();
    let written =
        docx_roundtrip::write(&first.document, &docx_roundtrip::Assets::from_opened(&first)).unwrap();
    let after = docx_roundtrip::open(&written).unwrap();

    let quote = after
        .document
        .styles
        .paragraph
        .get("Citazione")
        .expect("the style must survive by name");
    assert_eq!(quote.based_on.as_deref(), Some("Normale"));
    let run = quote.run.as_ref().unwrap();
    assert_eq!(run.italic, Some(true));
    assert_eq!(run.font.as_deref(), Some("Garamond"), "inherited, and kept");

    let Block::Paragraph { style, .. } = &after.document.sections[0].body[0] else {
        panic!("expected a paragraph");
    };
    assert_eq!(style.as_deref(), Some("Citazione"));
}

#[test]
fn the_written_package_has_the_parts_word_requires() {
    let (first, _) = round_trip(r#"<w:p><w:r><w:t>ciao</w:t></w:r></w:p>"#);
    let bytes =
        docx_roundtrip::write(&first.document, &docx_roundtrip::Assets::from_opened(&first)).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    for required in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "word/styles.xml",
    ] {
        assert!(names.contains(&required.to_string()), "missing {required} in {names:?}");
    }
}

#[test]
fn writing_the_same_document_twice_gives_the_same_bytes() {
    // Otherwise every save would look like a change in version control,
    // and a version comparison would be unreadable.
    let (first, _) = round_trip(r#"<w:p><w:r><w:t>stabile</w:t></w:r></w:p>"#);
    let assets = docx_roundtrip::Assets::from_opened(&first);
    let a = docx_roundtrip::write(&first.document, &assets).unwrap();
    let b = docx_roundtrip::write(&first.document, &assets).unwrap();
    assert_eq!(a, b);
}
