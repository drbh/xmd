//! The registry of feature recognizers: the one list of what reads a note's
//! features out of its generic blocks. The generic layer (`blocks`) knows the
//! document's structure and the language's inline forms; each recognizer
//! below reads those blocks and fills its `Document` fields, highlights and
//! problems, so the generic parser never names a feature and a new feature is
//! one row. Order is meaningful: recognizers run in list order at every hook,
//! so what one adds is there for the next.
use crate::attributes::Declaration;
use crate::blocks::{
    Block, BlockState, Checkbox, Heading, HighlightKind, Line, Tree, classify, trailing_name,
};
use crate::declared::On;
use crate::document::Document;
use crate::{attributes, calculations, plans_impl, sections, tables_impl, tasks};
use std::sync::Arc;

/// A heading, after its own marks are read.
type OnHeading = fn(&mut Tree, &mut Document, &Heading<'_>);
/// A line of prose, a list item or a table row: before its inline forms are
/// read (as `line`), or after (as `inline`).
type OnLine = fn(&mut Tree, &mut Document, &Line<'_>);
/// The rows under a line, once its inline forms and any continued expression
/// are read: how many rows below `row` it takes, when it claims them. Rows
/// taken are never classified.
type OnBlock = fn(&mut Tree, &mut Document, &[&str], usize) -> Option<usize>;
/// The whole note, after every line is read.
type OnDocument = fn(&mut Tree, &mut Document, &[&str]);

struct Recognizer {
    heading: Option<OnHeading>,
    line: Option<OnLine>,
    inline: Option<OnLine>,
    block: Option<OnBlock>,
    document: Option<OnDocument>,
}
impl Recognizer {
    const NONE: Self = Self {
        heading: None,
        line: None,
        inline: None,
        block: None,
        document: None,
    };
}

static RECOGNIZERS: &[Recognizer] = &[
    // What `@key(value)` means: painting each value, the unknown, repeated
    // and unclosed attribute problems, and every line that writes any.
    Recognizer {
        line: Some(attributes::recognize),
        ..Recognizer::NONE
    },
    // Sections: every heading, closed by the next that outranks it.
    Recognizer {
        heading: Some(sections::recognize),
        document: Some(sections::close),
        ..Recognizer::NONE
    },
    // Tasks: a list item with a checkbox.
    Recognizer {
        line: Some(tasks::recognize),
        ..Recognizer::NONE
    },
    // A line of math with bracketed variables.
    Recognizer {
        inline: Some(calculations::recognize),
        ..Recognizer::NONE
    },
    // `maximize`/`minimize` over the constraint table under it.
    Recognizer {
        block: Some(plans_impl::recognize),
        ..Recognizer::NONE
    },
    // `name := table` over the table under it.
    Recognizer {
        block: Some(tables_impl::recognize),
        ..Recognizer::NONE
    },
];

/// What the active modules declare that a note is read with: the recognizers
/// run over it once it is parsed, and the attributes it is parsed with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recognizers {
    pub rules: Vec<Arc<crate::declared::Rule>>,
    pub attributes: Vec<Arc<Declaration>>,
}

impl Document {
    /// Read the note with what modules declare, replacing what an earlier
    /// set found: parsed again when it writes an attribute no native feature
    /// owns and it was parsed with other declared attributes, then run the
    /// declared `rules` over.
    pub fn recognize(&mut self, recognizers: &Recognizers) {
        if self.foreign && self.declarations != recognizers.attributes {
            let text = std::mem::take(&mut self.text);
            *self = Self::parse_with(text, &recognizers.attributes);
        }
        self.match_declared(&recognizers.rules);
    }
    /// Parse a note knowing only the attributes native features own; a
    /// workspace parses it again with its modules' when it needs them.
    pub fn parse(text: String) -> Self {
        Self::parse_with(text, &[])
    }
    /// Parse a note knowing the attributes `declarations` declare too.
    pub fn parse_with(text: String, declarations: &[Arc<Declaration>]) -> Self {
        let mut tree = Tree::new(text.clone());
        let mut doc = Self::default();
        doc.declarations = declarations.to_vec();
        let lines: Vec<_> = text.lines().collect();
        let mut state = BlockState::default();
        let mut blocks = vec![None; lines.len()];
        let mut row = 0;
        while row < lines.len() {
            let line = lines[row];
            let mut consumed = 0;
            // A line of text: its indentation, any checkbox, and which block
            // it is, from where its text starts.
            let text_block = match classify(line, row, &mut state) {
                Block::Fence { start } => {
                    tree.mark(row, start, line.len(), HighlightKind::String);
                    None
                }
                Block::Comment { start } => {
                    tree.mark(row, start, line.len(), HighlightKind::Comment);
                    None
                }
                Block::Blank => None,
                Block::Heading(heading) => {
                    tree.heading(line, &heading);
                    let title = text_from(line, heading.start + heading.level);
                    blocks[row] = Some((On::Heading, title, heading.title_end));
                    for on in RECOGNIZERS.iter().filter_map(|r| r.heading) {
                        on(&mut tree, &mut doc, &heading);
                    }
                    None
                }
                Block::Item { start, checkbox } => {
                    let marker = checkbox.map_or(start + 1, |c| c.at + 3);
                    Some((start, checkbox, (On::Item, text_from(line, marker))))
                }
                Block::Row { start } => Some((start, None, (On::Row, start))),
                Block::Prose { start } => Some((start, None, (On::Prose, start))),
            };
            if let Some((start, checkbox, (on, from))) = text_block {
                let title_end;
                (consumed, title_end) = text_line(
                    &mut tree,
                    &mut doc,
                    &text,
                    &lines,
                    row,
                    start,
                    checkbox,
                    (on, from),
                );
                blocks[row] = Some((on, from, title_end));
            }
            // The rows a table or plan claims are still rows; the ones a
            // continued expression claims are part of it.
            for taken in row + 1..=row + consumed {
                let line = lines[taken];
                let start = line.len() - line.trim_start().len();
                if line[start..].starts_with('|') {
                    blocks[taken] = Some((On::Row, start, line.len()));
                }
            }
            row += 1 + consumed;
        }
        for on in RECOGNIZERS.iter().filter_map(|r| r.document) {
            on(&mut tree, &mut doc, &lines);
        }
        tree.finish();
        doc.adopt(tree);
        doc.blocks = blocks;
        doc
    }
}

/// Where a line's text starts from `at` on, past the blanks there.
fn text_from(line: &str, at: usize) -> usize {
    let at = at.min(line.len());
    at + line[at..].len() - line[at..].trim_start().len()
}

/// A line of prose, a list item or a table row: its attributes, the
/// recognizers of the line, its inline forms, then the rows under it that a
/// continued expression or a recognizer claims. Returns how many rows below
/// `row` were claimed, and where the line's title ends: at its first
/// attribute, or a checklist item's trailing `:name`.
#[allow(clippy::too_many_arguments)]
fn text_line(
    tree: &mut Tree,
    doc: &mut Document,
    note: &str,
    lines: &[&str],
    row: usize,
    start: usize,
    checkbox: Option<Checkbox>,
    (on, from): (On, usize),
) -> (usize, usize) {
    let text = lines[row];
    let body = checkbox.map_or(start, |c| c.at + 3);
    let line = Line {
        text,
        row,
        start,
        checkbox,
        body,
        on,
        from,
        attributes: tree.attributes(text, row, body),
    };
    let title_end = line
        .attributes
        .map
        .values()
        .map(|a| a.span.start)
        .chain(
            checkbox
                .and_then(|_| trailing_name(text, row))
                .map(|n| n.span.start - 1),
        )
        .min()
        .unwrap_or(text.len());
    for on in RECOGNIZERS.iter().filter_map(|r| r.line) {
        on(tree, doc, &line);
    }
    tree.prose(&line);
    for on in RECOGNIZERS.iter().filter_map(|r| r.inline) {
        on(tree, doc, &line);
    }
    let mut consumed = tree.continuation(note, lines, row).unwrap_or(0);
    for on in RECOGNIZERS.iter().filter_map(|r| r.block) {
        if let Some(rows) = on(tree, doc, lines, row) {
            consumed = rows;
        }
    }
    (consumed, title_end)
}
