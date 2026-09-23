// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! `styles.xml`: the named styles a template is built on.
//!
//! Word styles inherit through `w:basedOn`, sometimes several levels deep
//! (`Quotation` on `Body Text` on `Normal`). An editor that shows only the
//! properties written on a style shows the wrong thing, so the chain is
//! resolved into effective properties here.
//!
//! **The names survive.** Resolving inheritance is not flattening: each
//! style keeps its id, its display name and what it was based on, because
//! a document that loses its style names stops being the company's
//! document — the next person to open it in Word finds direct formatting
//! where the house style used to be.

use std::collections::BTreeMap;

use crate::model::{ParagraphProps, ParagraphStyle, RunProps, Styles, TableProps};
use crate::ooxml::Element;

use super::props;

/// Reads `styles.xml`, or returns empty styles when there is none.
pub fn parse_styles(root: Option<&Element>) -> Styles {
    let Some(root) = root else {
        return Styles::default();
    };
    let mut out = Styles::default();

    if let Some(defaults) = root.child("w:docDefaults") {
        if let Some(rpr) = defaults.child("w:rPrDefault") {
            out.defaults = props::run_props(rpr.child("w:rPr")).unwrap_or_default();
        }
    }

    // First pass: read what each style declares, keeping the chain.
    let mut declared: BTreeMap<String, (String, ParagraphStyle)> = BTreeMap::new();
    for style in root.children_named("w:style") {
        let Some(id) = style.attr("w:styleId") else {
            continue;
        };
        let kind = style.attr("w:type").unwrap_or("paragraph").to_string();
        let entry = ParagraphStyle {
            name: style.val("w:name").map(str::to_string),
            based_on: style.val("w:basedOn").map(str::to_string),
            next: style.val("w:next").map(str::to_string),
            paragraph: props::paragraph_props(style.child("w:pPr")),
            run: props::run_props(style.child("w:rPr")),
        };
        declared.insert(id.to_string(), (kind, entry));
    }

    // Second pass: resolve inheritance.
    let ids: Vec<String> = declared.keys().cloned().collect();
    for id in ids {
        let (kind, _) = declared[&id].clone();
        let resolved = resolve(&declared, &id, 0);
        match kind.as_str() {
            "character" => {
                if let Some(run) = resolved.run.clone() {
                    out.character.insert(id.clone(), run);
                }
            }
            "table" => {
                out.table.insert(id.clone(), TableProps::default());
                out.paragraph.insert(id.clone(), resolved);
            }
            // `paragraph`, and anything unexpected: a style we cannot
            // classify is still better shown than dropped.
            _ => {
                out.paragraph.insert(id.clone(), resolved);
            }
        }
    }
    out
}

/// Effective properties of a style, its ancestors merged in.
///
/// `depth` guards against a template whose `basedOn` chain loops — rare,
/// but a file that makes an editor hang is worse than one it reads
/// imperfectly.
fn resolve(
    declared: &BTreeMap<String, (String, ParagraphStyle)>,
    id: &str,
    depth: u32,
) -> ParagraphStyle {
    let Some((_, own)) = declared.get(id) else {
        return ParagraphStyle::default();
    };
    if depth > 16 {
        return own.clone();
    }
    let mut out = match &own.based_on {
        Some(parent) if parent != id => resolve(declared, parent, depth + 1),
        _ => ParagraphStyle::default(),
    };
    // The style's own declarations win over what it inherited.
    out.name = own.name.clone();
    out.based_on = own.based_on.clone();
    out.next = own.next.clone();
    out.paragraph = merge_paragraph(out.paragraph, own.paragraph.clone());
    out.run = merge_run(out.run, own.run.clone());
    out
}

fn merge_paragraph(
    base: Option<ParagraphProps>,
    over: Option<ParagraphProps>,
) -> Option<ParagraphProps> {
    match (base, over) {
        (None, o) => o,
        (b, None) => b,
        (Some(mut b), Some(o)) => {
            if o.align.is_some() {
                b.align = o.align;
            }
            if o.spacing_before.is_some() {
                b.spacing_before = o.spacing_before;
            }
            if o.spacing_after.is_some() {
                b.spacing_after = o.spacing_after;
            }
            if o.line_spacing.is_some() {
                b.line_spacing = o.line_spacing;
                b.line_rule = o.line_rule;
            }
            if o.indent_left.is_some() {
                b.indent_left = o.indent_left;
            }
            if o.indent_right.is_some() {
                b.indent_right = o.indent_right;
            }
            if o.first_line.is_some() {
                b.first_line = o.first_line;
            }
            if o.hanging.is_some() {
                b.hanging = o.hanging;
            }
            if o.shading.is_some() {
                b.shading = o.shading;
            }
            if o.numbering.is_some() {
                b.numbering = o.numbering;
            }
            if o.borders.is_some() {
                b.borders = o.borders;
            }
            // Toggles: only an explicit `true` overrides, because
            // `false` here means "not declared at this level".
            b.keep_next |= o.keep_next;
            b.keep_lines |= o.keep_lines;
            b.page_break_before |= o.page_break_before;
            Some(b)
        }
    }
}

fn merge_run(base: Option<RunProps>, over: Option<RunProps>) -> Option<RunProps> {
    match (base, over) {
        (None, o) => o,
        (b, None) => b,
        (Some(mut b), Some(o)) => {
            if o.font.is_some() {
                b.font = o.font;
            }
            if o.size.is_some() {
                b.size = o.size;
            }
            // Tri-state: `Some(false)` is a real value — a style that
            // turns bold off must beat a parent that turns it on.
            if o.bold.is_some() {
                b.bold = o.bold;
            }
            if o.italic.is_some() {
                b.italic = o.italic;
            }
            if o.underline.is_some() {
                b.underline = o.underline;
            }
            if o.strike.is_some() {
                b.strike = o.strike;
            }
            if o.caps.is_some() {
                b.caps = o.caps;
            }
            if o.color.is_some() {
                b.color = o.color;
            }
            if o.highlight.is_some() {
                b.highlight = o.highlight;
            }
            if o.vert_align.is_some() {
                b.vert_align = o.vert_align;
            }
            Some(b)
        }
    }
}

/// Reads `numbering.xml` into the lists a document uses.
pub fn parse_numbering(root: Option<&Element>) -> BTreeMap<String, crate::model::Numbering> {
    let Some(root) = root else {
        return BTreeMap::new();
    };
    // `w:num` points at an abstract definition; the paragraphs refer to
    // the concrete id, so that is what the map is keyed by.
    let mut abstracts: BTreeMap<String, crate::model::Numbering> = BTreeMap::new();
    for def in root.children_named("w:abstractNum") {
        let Some(id) = def.attr("w:abstractNumId") else {
            continue;
        };
        let mut levels = Vec::new();
        for lvl in def.children_named("w:lvl") {
            let ind = lvl.child("w:pPr").and_then(|p| p.child("w:ind"));
            levels.push(crate::model::NumberingLevel {
                format: lvl.val("w:numFmt").map(str::to_string),
                text: lvl.val("w:lvlText").map(str::to_string),
                indent_left: ind
                    .and_then(|i| i.attr("w:left").or_else(|| i.attr("w:start")))
                    .and_then(|v| v.parse().ok()),
                hanging: ind.and_then(|i| i.attr("w:hanging")).and_then(|v| v.parse().ok()),
            });
        }
        abstracts.insert(id.to_string(), crate::model::Numbering { levels });
    }

    let mut out = BTreeMap::new();
    for num in root.children_named("w:num") {
        let Some(id) = num.attr("w:numId") else {
            continue;
        };
        let target = num.val("w:abstractNumId").unwrap_or_default();
        if let Some(def) = abstracts.get(target) {
            out.insert(id.to_string(), def.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ooxml::parse_xml;

    fn styles(xml: &str) -> Styles {
        let root = parse_xml(xml.as_bytes()).unwrap();
        parse_styles(Some(&root))
    }

    #[test]
    fn a_style_inherits_from_the_one_it_is_based_on() {
        let s = styles(
            r#"<w:styles>
                <w:style w:type="paragraph" w:styleId="Normale"><w:name w:val="Normal"/><w:rPr><w:rFonts w:ascii="Garamond"/><w:sz w:val="22"/></w:rPr></w:style>
                <w:style w:type="paragraph" w:styleId="Citazione"><w:name w:val="Citazione"/><w:basedOn w:val="Normale"/><w:rPr><w:i/></w:rPr></w:style>
            </w:styles>"#,
        );
        let quote = &s.paragraph["Citazione"];
        let run = quote.run.as_ref().unwrap();
        assert_eq!(run.font.as_deref(), Some("Garamond"), "inherited");
        assert_eq!(run.size, Some(22), "inherited");
        assert_eq!(run.italic, Some(true), "its own");
        assert_eq!(quote.based_on.as_deref(), Some("Normale"), "the chain is kept");
    }

    #[test]
    fn a_child_style_can_turn_off_what_its_parent_turned_on() {
        let s = styles(
            r#"<w:styles>
                <w:style w:type="paragraph" w:styleId="Forte"><w:rPr><w:b/></w:rPr></w:style>
                <w:style w:type="paragraph" w:styleId="ForteMaNo"><w:basedOn w:val="Forte"/><w:rPr><w:b w:val="0"/></w:rPr></w:style>
            </w:styles>"#,
        );
        assert_eq!(s.paragraph["ForteMaNo"].run.as_ref().unwrap().bold, Some(false));
    }

    #[test]
    fn a_looping_chain_does_not_hang() {
        // Rare, but a file that makes the editor spin is worse than one
        // it reads imperfectly.
        let s = styles(
            r#"<w:styles>
                <w:style w:type="paragraph" w:styleId="A"><w:basedOn w:val="B"/></w:style>
                <w:style w:type="paragraph" w:styleId="B"><w:basedOn w:val="A"/></w:style>
            </w:styles>"#,
        );
        assert!(s.paragraph.contains_key("A") && s.paragraph.contains_key("B"));
    }

    #[test]
    fn document_defaults_are_read() {
        let s = styles(
            r#"<w:styles><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#,
        );
        assert_eq!(s.defaults.font.as_deref(), Some("Calibri"));
        assert_eq!(s.defaults.size, Some(22));
    }

    #[test]
    fn character_styles_land_in_their_own_map() {
        let s = styles(
            r#"<w:styles><w:style w:type="character" w:styleId="Enfasi"><w:rPr><w:i/></w:rPr></w:style></w:styles>"#,
        );
        assert!(s.character.contains_key("Enfasi"));
        assert!(!s.paragraph.contains_key("Enfasi"));
    }

    #[test]
    fn numbering_is_keyed_by_the_id_paragraphs_use() {
        // Paragraphs refer to `w:numId`, not to the abstract definition.
        let root = parse_xml(
            r#"<w:numbering>
                <w:abstractNum w:abstractNumId="3"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl></w:abstractNum>
                <w:num w:numId="7"><w:abstractNumId w:val="3"/></w:num>
            </w:numbering>"#
            .as_bytes(),
        )
        .unwrap();
        let n = parse_numbering(Some(&root));
        assert!(n.contains_key("7"), "keyed by numId: {:?}", n.keys().collect::<Vec<_>>());
        let level = &n["7"].levels[0];
        assert_eq!(level.format.as_deref(), Some("bullet"));
        assert_eq!(level.indent_left, Some(720));
    }
}
