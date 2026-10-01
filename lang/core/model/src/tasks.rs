//! Checklist items: a list item with a checkbox, `- [ ] title :name`,
//! nested under the nearest open item indented less than it, until a
//! heading starts over. They are the language's checklists: a named item is
//! a Boolean, whether it is done, and a named heading is the checklist of the
//! items under it. What else an item means, its attributes and what a person
//! does with it, is the `tasks` module's to say.
use crate::blocks::{Attribute, HighlightKind, Line, Named, Tree, trailing_name};
use crate::document::Document;
use common::Span;
use std::collections::BTreeMap;

/// What an item's checkbox says: `[ ]`, `[-]` or `[x]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskState {
    #[default]
    Open,
    /// `[-]`: partly done, still open.
    InProgress,
    Done,
}
impl TaskState {
    fn from_mark(mark: u8) -> Self {
        match mark {
            b' ' => Self::Open,
            b'-' => Self::InProgress,
            _ => Self::Done,
        }
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Task {
    pub line: usize,
    pub indent: usize,
    pub state: TaskState,
    pub checkbox: Span,
    pub title: String,
    pub named: Option<Named>,
    pub parent: Option<usize>,
    pub attributes: BTreeMap<String, Attribute>,
}

pub(crate) fn recognize(tree: &mut Tree, doc: &mut Document, line: &Line<'_>) {
    let Some(checkbox) = line.checkbox else {
        return;
    };
    let Line {
        text, row, start, ..
    } = *line;
    let attrs = &line.attributes.map;
    let at = checkbox.at;
    let named = trailing_name(text, row);
    // The open tasks are the last task and its ancestors, unless a heading
    // came after it; the parent is the nearest one indented less.
    let mut parent = doc
        .tasks
        .len()
        .checked_sub(1)
        .filter(|last| tree.heading.is_none_or(|h| h < doc.tasks[*last].line));
    while let Some(open) = parent.filter(|open| doc.tasks[*open].indent >= start) {
        parent = doc.tasks[open].parent;
    }
    let title_end = attrs
        .values()
        .map(|a| a.span.start)
        .chain(named.iter().map(|n| n.span.start - 1))
        .min()
        .unwrap_or(text.len());
    doc.tasks.push(Task {
        line: row,
        indent: start,
        state: TaskState::from_mark(checkbox.mark),
        checkbox: Span::new(row, at, at + 3),
        title: text[at + 3..title_end].trim().into(),
        named: named.clone(),
        parent,
        attributes: attrs.clone(),
    });
    tree.mark(row, at, at + 3, HighlightKind::Keyword);
    if let Some(n) = named {
        tree.mark(row, n.span.start, n.span.end, HighlightKind::Variable);
    }
}
