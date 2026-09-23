// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Reading and writing the XML inside a `.docx`, at the level where order
//! and whitespace still matter.
//!
//! A paragraph *is* an ordered sequence, so this keeps a small ordered tree
//! rather than a map: children in document order, attributes in the order
//! they were written. Anything that reorders them turns a re-save into a
//! diff full of changes nobody made.
//!
//! Two OOXML rules are easy to get wrong and are handled here once:
//!
//! * **`xml:space="preserve"`.** Word writes significant spaces in
//!   `<w:t xml:space="preserve"> </w:t>`. Trimming text on the way in
//!   silently welds words together.
//! * **Toggle properties are tri-state.** `<w:b/>` present means on,
//!   `<w:b w:val="0"/>` means explicitly off, and *absent* means "inherit
//!   from the style" — which is not the same as off. Collapsing the last
//!   two is how bold disappears from a whole document.

use std::io::{Cursor, Read};

use anyhow::{anyhow, Context, Result};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

/// One XML node: an element or a piece of text.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

/// An XML element, with its attributes and children in source order.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// Qualified name as written, prefix included: `w:p`, `r:embed`.
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attrs: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn with_attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attrs.push((name.into(), value.into()));
        self
    }

    pub fn with_child(mut self, child: Element) -> Self {
        self.children.push(Node::Element(child));
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.children.push(Node::Text(text.into()));
        self
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// First direct child with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// All direct children with this name, in order.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// All direct element children, in order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The `w:val` attribute of a named child property.
    pub fn val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("w:val"))
    }

    /// The `w:val` of a named child, as an integer.
    pub fn val_int(&self, name: &str) -> Option<i32> {
        self.val(name).and_then(|v| v.trim().parse().ok())
    }

    /// OOXML toggle semantics: `None` = inherit, `Some(true)` = on,
    /// `Some(false)` = explicitly off.
    pub fn toggle(&self, name: &str) -> Option<bool> {
        let child = self.child(name)?;
        Some(match child.attr("w:val") {
            None => true,
            Some(v) => !matches!(v, "0" | "false" | "off"),
        })
    }

    /// Concatenated text of this element and its descendants.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.write_text(&mut out);
        out
    }

    fn write_text(&self, out: &mut String) {
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.write_text(out),
            }
        }
    }

    /// Serialises this element back to XML.
    ///
    /// This is what makes opaque content possible: a fragment nobody
    /// understood goes back out the way it came in, attributes in the same
    /// order, whitespace untouched.
    pub fn to_xml(&self) -> String {
        let mut writer = Writer::new(Cursor::new(Vec::new()));
        self.write_to(&mut writer)
            .expect("writing to an in-memory buffer cannot fail");
        String::from_utf8(writer.into_inner().into_inner())
            .expect("the tree came from valid UTF-8 XML")
    }

    fn write_to<W: std::io::Write>(&self, writer: &mut Writer<W>) -> Result<()> {
        let mut start = BytesStart::new(self.name.as_str());
        for (k, v) in &self.attrs {
            start.push_attribute((k.as_str(), v.as_str()));
        }
        if self.children.is_empty() {
            writer.write_event(Event::Empty(start))?;
            return Ok(());
        }
        writer.write_event(Event::Start(start))?;
        for child in &self.children {
            match child {
                Node::Text(t) => {
                    writer.write_event(Event::Text(BytesText::new(t)))?;
                }
                Node::Element(e) => e.write_to(writer)?,
            }
        }
        writer.write_event(Event::End(BytesEnd::new(self.name.as_str())))?;
        Ok(())
    }
}

/// Parses one XML part into its root element.
pub fn parse_xml(bytes: &[u8]) -> Result<Element> {
    let text = std::str::from_utf8(bytes).context("the XML part is not UTF-8")?;
    let mut reader = Reader::from_str(text);
    let config = reader.config_mut();
    // Word's significant spaces live in text nodes; trimming them here
    // would weld words together at run boundaries.
    config.trim_text(false);
    config.expand_empty_elements = false;

    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    loop {
        match reader.read_event()? {
            Event::Start(e) => stack.push(element_from(&e)?),
            Event::Empty(e) => {
                let el = element_from(&e)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Element(el)),
                    None => root = Some(el),
                }
            }
            Event::End(_) => {
                let done = stack
                    .pop()
                    .ok_or_else(|| anyhow!("closing tag without an opening one"))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Element(done)),
                    None => root = Some(done),
                }
            }
            Event::Text(t) => {
                if let Some(parent) = stack.last_mut() {
                    let s = t.unescape()?.into_owned();
                    if !s.is_empty() {
                        parent.children.push(Node::Text(s));
                    }
                }
            }
            Event::CData(t) => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .children
                        .push(Node::Text(String::from_utf8_lossy(&t).into_owned()));
                }
            }
            Event::Eof => break,
            // Declarations, comments and processing instructions carry
            // nothing we read and nothing we must reproduce.
            _ => {}
        }
    }
    root.ok_or_else(|| anyhow!("the XML part has no root element"))
}

fn element_from(start: &BytesStart) -> Result<Element> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = attr.unescape_value()?.into_owned();
        attrs.push((key, value));
    }
    Ok(Element {
        name,
        attrs,
        children: Vec::new(),
    })
}

/// A `.docx` opened for reading: the parts, by name.
pub struct Package {
    parts: std::collections::BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// Reads every part into memory. A `.docx` is a few hundred kilobytes
    /// of XML plus its images; streaming would complicate the parser for
    /// no gain at this size.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .context("this file is not a zip container, so it is not a .docx")?;
        let mut parts = std::collections::BTreeMap::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            parts.insert(name, buf);
        }
        Ok(Self { parts })
    }

    pub fn raw(&self, name: &str) -> Option<&[u8]> {
        self.parts.get(name).map(|v| v.as_slice())
    }

    /// Parses a part, or `None` when it is not in the package. A missing
    /// part is normal — most documents have no `footnotes.xml` — so it is
    /// not an error.
    pub fn part(&self, name: &str) -> Result<Option<Element>> {
        match self.raw(name) {
            None => Ok(None),
            Some(bytes) => Ok(Some(
                parse_xml(bytes).with_context(|| format!("parsing {name}"))?,
            )),
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.parts.keys().map(|s| s.as_str())
    }
}

/// One entry of a `_rels` part.
#[derive(Debug, Clone, PartialEq)]
pub struct Rel {
    pub id: String,
    /// Full relationship type URI.
    pub kind: String,
    pub target: String,
    /// `Internal` (the default) or `External`.
    pub mode: String,
}

impl Rel {
    /// The last segment of the type URI: `image`, `hyperlink`, `header`.
    pub fn short_kind(&self) -> &str {
        self.kind.rsplit('/').next().unwrap_or(&self.kind)
    }
}

/// Reads the relationships of a part, plus the directory they resolve
/// against.
pub fn read_rels(pkg: &Package, part_name: &str) -> Result<(Vec<Rel>, String)> {
    let (dir, base) = match part_name.rfind('/') {
        Some(i) => (&part_name[..i], &part_name[i + 1..]),
        None => ("", part_name),
    };
    let rels_name = if dir.is_empty() {
        format!("_rels/{base}.rels")
    } else {
        format!("{dir}/_rels/{base}.rels")
    };
    let Some(root) = pkg.part(&rels_name)? else {
        return Ok((Vec::new(), dir.to_string()));
    };
    let mut out = Vec::new();
    for r in root.children_named("Relationship") {
        let (Some(id), Some(kind), Some(target)) =
            (r.attr("Id"), r.attr("Type"), r.attr("Target"))
        else {
            continue;
        };
        out.push(Rel {
            id: id.to_string(),
            kind: kind.to_string(),
            target: target.to_string(),
            mode: r.attr("TargetMode").unwrap_or("Internal").to_string(),
        });
    }
    Ok((out, dir.to_string()))
}

/// Resolves a relationship target to a package part name.
///
/// A leading slash means package-absolute; anything else is relative to the
/// part's own directory, `..` included — `../media/logo.png` from
/// `word/header1.xml` is `media/logo.png`, not `word/../media/logo.png`,
/// which no zip contains.
pub fn resolve_target(dir: &str, target: &str) -> String {
    if let Some(stripped) = target.strip_prefix('/') {
        return stripped.to_string();
    }
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_children_and_attributes_in_source_order() {
        // A paragraph is a sequence: reordering it rewrites the document.
        let root = parse_xml(br#"<w:p><w:r><w:t>uno</w:t></w:r><w:r><w:t>due</w:t></w:r></w:p>"#)
            .unwrap();
        assert_eq!(root.name, "w:p");
        let texts: Vec<String> = root.elements().map(|r| r.text()).collect();
        assert_eq!(texts, ["uno", "due"]);
    }

    #[test]
    fn significant_whitespace_survives() {
        // `<w:t xml:space="preserve"> </w:t>` is how Word writes the space
        // between two words that sit in different runs.
        let root =
            parse_xml(br#"<w:p><w:r><w:t>uno</w:t></w:r><w:r><w:t xml:space="preserve"> </w:t></w:r><w:r><w:t>due</w:t></w:r></w:p>"#)
                .unwrap();
        assert_eq!(root.text(), "uno due");
    }

    #[test]
    fn toggles_are_tri_state() {
        let on = parse_xml(br#"<w:rPr><w:b/></w:rPr>"#).unwrap();
        assert_eq!(on.toggle("w:b"), Some(true));

        let off = parse_xml(br#"<w:rPr><w:b w:val="0"/></w:rPr>"#).unwrap();
        assert_eq!(off.toggle("w:b"), Some(false), "explicitly off");

        let inherit = parse_xml(br#"<w:rPr><w:i/></w:rPr>"#).unwrap();
        assert_eq!(
            inherit.toggle("w:b"),
            None,
            "absent means inherit from the style, not off"
        );
    }

    #[test]
    fn a_fragment_serialises_back_as_it_came() {
        // The whole opaque strategy rests on this.
        let xml = r#"<w:pict><v:shape id="s1" style="width:10pt"><v:textbox><w:txbxContent><w:p><w:r><w:t>ciao</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict>"#;
        let root = parse_xml(xml.as_bytes()).unwrap();
        assert_eq!(root.to_xml(), xml);
    }

    #[test]
    fn empty_elements_stay_empty() {
        let root = parse_xml(br#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr></w:p>"#).unwrap();
        assert_eq!(
            root.to_xml(),
            r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr></w:p>"#
        );
    }

    #[test]
    fn entities_are_read_and_written_back() {
        let root = parse_xml(br#"<w:t>Rossi &amp; Bianchi &lt;spa&gt;</w:t>"#).unwrap();
        assert_eq!(root.text(), "Rossi & Bianchi <spa>");
        assert_eq!(root.to_xml(), r#"<w:t>Rossi &amp; Bianchi &lt;spa&gt;</w:t>"#);
    }

    #[test]
    fn targets_resolve_against_the_part_directory() {
        assert_eq!(resolve_target("word", "media/logo.png"), "word/media/logo.png");
        assert_eq!(resolve_target("word", "../customXml/item1.xml"), "customXml/item1.xml");
        assert_eq!(resolve_target("word", "/word/media/a.png"), "word/media/a.png");
        assert_eq!(resolve_target("", "document.xml"), "document.xml");
    }

    #[test]
    fn the_short_relationship_kind_is_the_last_segment() {
        let rel = Rel {
            id: "rId5".into(),
            kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image".into(),
            target: "media/logo.png".into(),
            mode: "Internal".into(),
        };
        assert_eq!(rel.short_kind(), "image");
    }
}
