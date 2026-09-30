//! Sections: every heading opens one and closes the open sections it
//! outranks; a named heading (`## Packing :packing`) is a checklist over the
//! tasks under it.
use crate::blocks::{Heading, Named, Tree};
use crate::document::Document;

#[derive(Clone, Debug)]
pub struct Section {
    pub line: usize,
    pub end_line: usize,
    pub level: usize,
    pub title: String,
    pub named: Option<Named>,
}

pub(crate) fn recognize(_: &mut Tree, doc: &mut Document, heading: &Heading<'_>) {
    for section in &mut doc.sections {
        if section.end_line == usize::MAX && section.level >= heading.level {
            section.end_line = heading.row;
        }
    }
    doc.sections.push(Section {
        line: heading.row,
        end_line: usize::MAX,
        level: heading.level,
        title: heading.title.into(),
        named: heading.named.clone(),
    });
}

/// Close the sections still open at the end of the note.
pub(crate) fn close(_: &mut Tree, doc: &mut Document, lines: &[&str]) {
    for section in &mut doc.sections {
        if section.end_line == usize::MAX {
            section.end_line = lines.len();
        }
    }
}
