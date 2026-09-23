// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Reading the property blocks: `w:rPr`, `w:pPr`, `w:tblPr`, borders.
//!
//! Every reader here returns `None` when the block says nothing, rather
//! than a struct full of defaults. The difference matters on the way out:
//! a document where every run carries an explicit `bold: false` no longer
//! inherits from its style, so changing the style stops changing the
//! document — which is the whole reason a company template has styles.

use crate::model::{
    Border, Borders, CellProps, NumberingRef, ParagraphProps, RowProps, RunProps, TableProps,
};
use crate::ooxml::Element;

/// Word writes alignment with several spellings for the same thing.
fn align_name(value: &str) -> String {
    match value {
        "left" | "start" => "left",
        "right" | "end" => "right",
        "center" => "center",
        "both" | "distribute" => "justify",
        other => other,
    }
    .to_string()
}

pub fn run_props(rpr: Option<&Element>) -> Option<RunProps> {
    let rpr = rpr?;
    let mut out = RunProps::default();

    if let Some(fonts) = rpr.child("w:rFonts") {
        // Word repeats the family per script; the ASCII one is what a
        // Latin-script document is actually set in.
        out.font = fonts
            .attr("w:ascii")
            .or_else(|| fonts.attr("w:hAnsi"))
            .or_else(|| fonts.attr("w:cs"))
            .map(str::to_string);
    }
    out.size = rpr.val_int("w:sz").and_then(|v| u32::try_from(v).ok());
    out.bold = rpr.toggle("w:b");
    out.italic = rpr.toggle("w:i");
    out.strike = rpr.toggle("w:strike");
    out.caps = rpr.toggle("w:caps");
    // Underline is not a toggle: it carries a style name, and "none" is
    // how Word turns it off.
    out.underline = rpr.val("w:u").map(|v| v != "none");
    out.color = rpr
        .val("w:color")
        .filter(|c| *c != "auto")
        .map(str::to_string);
    out.highlight = rpr
        .val("w:highlight")
        .filter(|h| *h != "none")
        .map(str::to_string);
    out.vert_align = rpr
        .val("w:vertAlign")
        .filter(|v| matches!(*v, "superscript" | "subscript"))
        .map(str::to_string);

    if out == RunProps::default() {
        None
    } else {
        Some(out)
    }
}

pub fn paragraph_props(ppr: Option<&Element>) -> Option<ParagraphProps> {
    let ppr = ppr?;
    let mut out = ParagraphProps::default();

    out.align = ppr.val("w:jc").map(align_name);

    if let Some(spacing) = ppr.child("w:spacing") {
        out.spacing_before = spacing.attr("w:before").and_then(|v| v.parse().ok());
        out.spacing_after = spacing.attr("w:after").and_then(|v| v.parse().ok());
        out.line_spacing = spacing.attr("w:line").and_then(|v| v.parse().ok());
        out.line_rule = spacing.attr("w:lineRule").map(str::to_string);
    }
    if let Some(ind) = ppr.child("w:ind") {
        // `start`/`end` are the newer spellings of `left`/`right`.
        out.indent_left = ind
            .attr("w:left")
            .or_else(|| ind.attr("w:start"))
            .and_then(|v| v.parse().ok());
        out.indent_right = ind
            .attr("w:right")
            .or_else(|| ind.attr("w:end"))
            .and_then(|v| v.parse().ok());
        out.first_line = ind.attr("w:firstLine").and_then(|v| v.parse().ok());
        out.hanging = ind.attr("w:hanging").and_then(|v| v.parse().ok());
    }
    out.keep_next = ppr.toggle("w:keepNext").unwrap_or(false);
    out.keep_lines = ppr.toggle("w:keepLines").unwrap_or(false);
    out.page_break_before = ppr.toggle("w:pageBreakBefore").unwrap_or(false);
    out.shading = ppr
        .child("w:shd")
        .and_then(|s| s.attr("w:fill"))
        .filter(|f| *f != "auto")
        .map(str::to_string);
    if let Some(num) = ppr.child("w:numPr") {
        if let Some(id) = num.val("w:numId") {
            out.numbering = Some(NumberingRef {
                id: id.to_string(),
                level: num.val_int("w:ilvl").unwrap_or(0).max(0) as u32,
            });
        }
    }
    out.borders = borders(ppr.child("w:pBdr"));

    if out == ParagraphProps::default() {
        None
    } else {
        Some(out)
    }
}

pub fn borders(node: Option<&Element>) -> Option<Borders> {
    let node = node?;
    let read = |side: &str, alt: Option<&str>| -> Option<Border> {
        let el = node
            .child(&format!("w:{side}"))
            .or_else(|| alt.and_then(|a| node.child(&format!("w:{a}"))))?;
        let b = Border {
            style: el.attr("w:val").map(str::to_string),
            size: el.attr("w:sz").and_then(|v| v.parse().ok()),
            color: el
                .attr("w:color")
                .filter(|c| *c != "auto")
                .map(str::to_string),
        };
        Some(b)
    };
    let out = Borders {
        top: read("top", None),
        left: read("left", Some("start")),
        bottom: read("bottom", None),
        right: read("right", Some("end")),
        inside_h: read("insideH", None),
        inside_v: read("insideV", None),
    };
    if out == Borders::default() {
        None
    } else {
        Some(out)
    }
}

pub fn table_props(tblpr: Option<&Element>, grid: Option<&Element>) -> TableProps {
    let mut out = TableProps::default();
    if let Some(tblpr) = tblpr {
        if let Some(w) = tblpr.child("w:tblW") {
            let kind = w.attr("w:type").unwrap_or("dxa");
            // `auto` means "let Word decide": storing the nominal zero
            // would freeze the table at no width.
            if kind != "auto" {
                out.width = w.attr("w:w").and_then(|v| v.parse().ok());
                out.layout = Some(if kind == "pct" { "pct" } else { "fixed" }.to_string());
            }
        }
        if let Some(layout) = tblpr.child("w:tblLayout").and_then(|l| l.attr("w:type")) {
            out.layout = Some(layout.to_string());
        }
        out.borders = borders(tblpr.child("w:tblBorders"));
        out.align = tblpr.val("w:jc").map(align_name);
    }
    if let Some(grid) = grid {
        out.grid = grid
            .children_named("w:gridCol")
            .filter_map(|c| c.attr("w:w").and_then(|v| v.parse().ok()))
            .collect();
    }
    out
}

pub fn row_props(trpr: Option<&Element>) -> Option<RowProps> {
    let trpr = trpr?;
    let out = RowProps {
        header: trpr.toggle("w:tblHeader").unwrap_or(false),
        cant_split: trpr.toggle("w:cantSplit").unwrap_or(false),
        height: trpr
            .child("w:trHeight")
            .and_then(|h| h.attr("w:val"))
            .and_then(|v| v.parse().ok()),
    };
    if out == RowProps::default() {
        None
    } else {
        Some(out)
    }
}

/// Cell properties. The vertical merge is returned separately because it
/// only makes sense while walking the rows: a `continue` cell is not a
/// cell of its own, it is the tail of one above.
pub fn cell_props(tcpr: Option<&Element>) -> (Option<CellProps>, VMerge) {
    let Some(tcpr) = tcpr else {
        return (None, VMerge::None);
    };
    let mut out = CellProps::default();
    if let Some(w) = tcpr.child("w:tcW") {
        if w.attr("w:type").unwrap_or("dxa") != "auto" {
            out.width = w.attr("w:w").and_then(|v| v.parse().ok());
        }
    }
    out.colspan = tcpr
        .val_int("w:gridSpan")
        .filter(|v| *v > 1)
        .map(|v| v as u32);
    out.shading = tcpr
        .child("w:shd")
        .and_then(|s| s.attr("w:fill"))
        .filter(|f| *f != "auto")
        .map(str::to_string);
    out.borders = borders(tcpr.child("w:tcBorders"));
    out.v_align = tcpr.val("w:vAlign").map(str::to_string);

    let merge = match tcpr.child("w:vMerge") {
        None => VMerge::None,
        // `<w:vMerge/>` with no value means "continue", not "restart" —
        // the default here is the opposite of what it looks like.
        Some(m) => match m.attr("w:val") {
            Some("restart") => VMerge::Restart,
            _ => VMerge::Continue,
        },
    };
    let props = if out == CellProps::default() {
        None
    } else {
        Some(out)
    };
    (props, merge)
}

/// Where a cell sits in a vertical merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VMerge {
    None,
    Restart,
    Continue,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::parse_xml;

    fn el(xml: &str) -> Element {
        parse_xml(xml.as_bytes()).unwrap()
    }

    #[test]
    fn an_empty_property_block_says_nothing() {
        // Not "everything is off": a run with no properties inherits.
        assert_eq!(run_props(Some(&el("<w:rPr/>"))), None);
        assert_eq!(paragraph_props(Some(&el("<w:pPr/>"))), None);
        assert_eq!(run_props(None), None);
    }

    #[test]
    fn bold_off_is_not_the_same_as_bold_unset() {
        let off = run_props(Some(&el(r#"<w:rPr><w:b w:val="0"/></w:rPr>"#))).unwrap();
        assert_eq!(off.bold, Some(false));
        let on = run_props(Some(&el("<w:rPr><w:b/></w:rPr>"))).unwrap();
        assert_eq!(on.bold, Some(true));
        let unset = run_props(Some(&el("<w:rPr><w:i/></w:rPr>"))).unwrap();
        assert_eq!(unset.bold, None);
    }

    #[test]
    fn underline_none_turns_it_off() {
        let off = run_props(Some(&el(r#"<w:rPr><w:u w:val="none"/></w:rPr>"#))).unwrap();
        assert_eq!(off.underline, Some(false));
        let on = run_props(Some(&el(r#"<w:rPr><w:u w:val="single"/></w:rPr>"#))).unwrap();
        assert_eq!(on.underline, Some(true));
    }

    #[test]
    fn automatic_colour_is_not_a_colour() {
        // `auto` means "whatever the theme says"; storing it would pin
        // black into a document that should follow its template.
        let auto = run_props(Some(&el(r#"<w:rPr><w:color w:val="auto"/><w:b/></w:rPr>"#))).unwrap();
        assert_eq!(auto.color, None);
        let red = run_props(Some(&el(r#"<w:rPr><w:color w:val="FF0000"/></w:rPr>"#))).unwrap();
        assert_eq!(red.color.as_deref(), Some("FF0000"));
    }

    #[test]
    fn the_font_comes_from_the_latin_script_slot() {
        let p = run_props(Some(&el(
            r#"<w:rPr><w:rFonts w:ascii="Garamond" w:hAnsi="Garamond" w:cs="Arial"/></w:rPr>"#,
        )))
        .unwrap();
        assert_eq!(p.font.as_deref(), Some("Garamond"));
    }

    #[test]
    fn alignment_spellings_collapse_to_four_names() {
        let names = |xml: &str| paragraph_props(Some(&el(xml))).unwrap().align.unwrap();
        assert_eq!(names(r#"<w:pPr><w:jc w:val="start"/></w:pPr>"#), "left");
        assert_eq!(names(r#"<w:pPr><w:jc w:val="end"/></w:pPr>"#), "right");
        assert_eq!(names(r#"<w:pPr><w:jc w:val="both"/></w:pPr>"#), "justify");
        assert_eq!(names(r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#), "center");
    }

    #[test]
    fn indentation_accepts_both_spellings() {
        let a = paragraph_props(Some(&el(
            r#"<w:pPr><w:ind w:left="720" w:right="360"/></w:pPr>"#,
        )))
        .unwrap();
        let b = paragraph_props(Some(&el(
            r#"<w:pPr><w:ind w:start="720" w:end="360"/></w:pPr>"#,
        )))
        .unwrap();
        assert_eq!(a.indent_left, Some(720));
        assert_eq!(b.indent_left, Some(720));
        assert_eq!(a.indent_right, b.indent_right);
    }

    #[test]
    fn a_numbered_paragraph_keeps_its_list_and_level() {
        let p = paragraph_props(Some(&el(
            r#"<w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="7"/></w:numPr></w:pPr>"#,
        )))
        .unwrap();
        let n = p.numbering.unwrap();
        assert_eq!((n.id.as_str(), n.level), ("7", 2));
    }

    #[test]
    fn a_vertical_merge_without_a_value_continues() {
        // The easy mistake: `<w:vMerge/>` looks like a start and is not.
        let (_, m) = cell_props(Some(&el("<w:tcPr><w:vMerge/></w:tcPr>")));
        assert_eq!(m, VMerge::Continue);
        let (_, m) = cell_props(Some(&el(r#"<w:tcPr><w:vMerge w:val="restart"/></w:tcPr>"#)));
        assert_eq!(m, VMerge::Restart);
        let (_, m) = cell_props(Some(&el("<w:tcPr/>")));
        assert_eq!(m, VMerge::None);
    }

    #[test]
    fn a_grid_span_of_one_is_not_a_span() {
        let (p, _) = cell_props(Some(&el(
            r#"<w:tcPr><w:gridSpan w:val="1"/><w:shd w:fill="EEEEEE"/></w:tcPr>"#,
        )));
        let p = p.unwrap();
        assert_eq!(p.colspan, None);
        assert_eq!(p.shading.as_deref(), Some("EEEEEE"));
    }

    #[test]
    fn an_automatic_table_width_is_left_to_word() {
        let t = table_props(
            Some(&el(r#"<w:tblPr><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#)),
            None,
        );
        assert_eq!(t.width, None, "0 twips would freeze the table at no width");
    }

    #[test]
    fn the_grid_gives_the_column_widths() {
        let t = table_props(
            None,
            Some(&el(
                r#"<w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="3000"/></w:tblGrid>"#,
            )),
        );
        assert_eq!(t.grid, vec![2000, 3000]);
    }
}
