// Copyright (c) 2026 Dario Finardi. Licensed under AGPL-3.0-only.
//! Open a `.docx`, edit it, write it back — without losing what this
//! library does not understand.
//!
//! A company's Word template carries more than an editor can model: text
//! boxes, charts, fields, drawings, bookmarks, content controls. Three
//! things can be done with that content, and only one of them is honest:
//!
//! * drop it — every save impoverishes the document;
//! * refuse the file — the editor is useless on the documents people have;
//! * **keep it verbatim and put it back where it was** — the document
//!   survives an editor that understands half of it.
//!
//! This crate does the third. What it models becomes an editable tree
//! ([`model::Document`]); what it does not becomes an *opaque* node
//! holding the original XML and the relationships that XML refers to.
//!
//! ```no_run
//! # fn main() -> anyhow::Result<()> {
//! let bytes = std::fs::read("offerta.docx")?;
//! let opened = docx_roundtrip::open(&bytes)?;
//! println!("{} paragraphs, {} kept verbatim",
//!     opened.document.sections[0].body.len(), opened.document.opaque.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## What it is not
//!
//! It is not a renderer: it says nothing about how a document *looks*, and
//! it does not lay out pages. It is not a Word-compatible writer in the
//! sense of reproducing a file byte for byte — see [`open`] for what is
//! actually promised.

pub mod diff;
pub mod edit;
pub mod model;
pub mod parse;
pub mod render;
pub mod ooxml;

pub use diff::{compare, Comparison};
pub use edit::{apply_text_edits, Edit};
pub use model::Document;
pub use parse::{open, Opened};
pub use render::{write, Assets};
