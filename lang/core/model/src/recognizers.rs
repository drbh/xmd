//! The registry of feature recognizers: the one list of what reads a note's
//! features out of its generic blocks. The generic layer (`blocks`) knows the
//! document's structure and the language's inline forms; each recognizer
//! below reads those blocks and fills its `Document` fields, highlights and
//! problems, so the generic parser never names a feature and a new feature is
//! one row. Order is meaningful: recognizers run in list order at every hook,
//! so what one adds is there for the next.
use crate::blocks::{Block, BlockState, Checkbox, Heading, HighlightKind, Line, Tree, classify};
use crate::declared::On;
use crate::document::Document;
use crate::{
    attributes, calculations, events, itinerary_impl, plans_impl, sections, tables_impl, tasks,
};

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
    // What `@key(value)` means: painting each value, and the unknown,
    // repeated and unclosed attribute problems.
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
    // Events: a line with `@at(…)` and no checkbox.
    Recognizer {
        line: Some(events::recognize),
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
    // Day headings and their timed stops, anywhere in the note.
    Recognizer {
        document: Some(itinerary_impl::recognize),
        ..Recognizer::NONE
    },
];

impl Document {
    pub fn parse(text: String) -> Self {
        let mut tree = Tree::new(text.clone());
        let mut doc = Self::default();
        let lines: Vec<_> = text.lines().collect();
        let mut state = BlockState::default();
        let mut blocks = vec![None; lines.len()];
        let mut row = 0;
        while row < lines.len() {
            let line = lines[row];
            let mut consumed = 0;
            match classify(line, row, &mut state) {
                Block::Fence { start } => tree.mark(row, start, line.len(), HighlightKind::String),
                Block::Comment { start } => {
                    tree.mark(row, start, line.len(), HighlightKind::Comment);
                }
                Block::Blank => {}
                Block::Heading(heading) => {
                    tree.heading(line, &heading);
                    let title = text_from(line, heading.start + heading.level);
                    blocks[row] = Some((On::Heading, title));
                    for on in RECOGNIZERS.iter().filter_map(|r| r.heading) {
                        on(&mut tree, &mut doc, &heading);
                    }
                }
                Block::Item { start, checkbox } => {
                    let marker = checkbox.map_or(start + 1, |c| c.at + 3);
                    blocks[row] = Some((On::Item, text_from(line, marker)));
                    consumed = text_line(&mut tree, &mut doc, &text, &lines, row, start, checkbox);
                }
                Block::Row { start } => {
                    blocks[row] = Some((On::Row, start));
                    consumed = text_line(&mut tree, &mut doc, &text, &lines, row, start, None);
                }
                Block::Prose { start } => {
                    blocks[row] = Some((On::Prose, start));
                    consumed = text_line(&mut tree, &mut doc, &text, &lines, row, start, None);
                }
            }
            // The rows a table or plan claims are still rows; the ones a
            // continued expression claims are part of it.
            for taken in row + 1..=row + consumed {
                let line = lines[taken];
                let start = line.len() - line.trim_start().len();
                if line[start..].starts_with('|') {
                    blocks[taken] = Some((On::Row, start));
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
/// `row` were claimed.
fn text_line(
    tree: &mut Tree,
    doc: &mut Document,
    note: &str,
    lines: &[&str],
    row: usize,
    start: usize,
    checkbox: Option<Checkbox>,
) -> usize {
    let text = lines[row];
    let body = checkbox.map_or(start, |c| c.at + 3);
    let line = Line {
        text,
        row,
        start,
        checkbox,
        body,
        attributes: tree.attributes(text, row, body),
    };
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
    consumed
}
