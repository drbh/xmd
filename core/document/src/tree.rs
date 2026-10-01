//! What the generic layer of a parse has read so far, and what the
//! recognizers add to: a [`Tree`] holds the note's text, its inline forms
//! (read in `inline`), and the highlighting and problems of every block, and
//! becomes the matching fields of the `Document`. Here are the tree itself
//! and how anything marks a span or reports a problem on it.
use crate::inline::{Calculation, Definition, Link, Reference};
use common::{LineIndex, Lines, Span};
use std::collections::BTreeSet;

/// The syntactic role of a span, as the parser sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum HighlightKind {
    String,
    Comment,
    Heading,
    Variable,
    Keyword,
    Number,
    Operator,
}
impl HighlightKind {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// How a literal reads: as text, or as a number, money, a date...
    pub(crate) fn literal(text: bool) -> Self {
        if text { Self::String } else { Self::Number }
    }
}
#[derive(Clone, Debug)]
pub struct Highlight {
    pub span: Span,
    pub kind: HighlightKind,
}
#[derive(Clone, Debug)]
pub struct Problem {
    pub span: Span,
    pub message: String,
}

/// What the generic layer has read so far, and what recognizers add to: the
/// language's inline forms and the highlighting and problems of every block.
/// It becomes the matching fields of the `Document`.
#[derive(Default)]
pub(crate) struct Tree {
    pub(crate) text: String,
    pub(crate) lines: LineIndex,
    /// The row of the last heading read.
    pub(crate) heading: Option<usize>,
    pub(crate) definitions: Vec<Definition>,
    pub(crate) references: Vec<Reference>,
    pub(crate) imports: BTreeSet<String>,
    pub(crate) members: Vec<crate::imports::Member>,
    pub(crate) links: Vec<Link>,
    pub(crate) calculations: Vec<Calculation>,
    pub(crate) highlights: Vec<Highlight>,
    pub(crate) problems: Vec<Problem>,
    /// The forms modules declare, which a call names the way it names a
    /// built-in rather than a value of the note.
    pub(crate) forms: Vec<String>,
}
impl Lines for Tree {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_start(&self, line: usize) -> usize {
        self.lines.start(line)
    }
}

impl Tree {
    pub(crate) fn new(text: String) -> Self {
        Self {
            lines: LineIndex::new(&text),
            text,
            ..Self::default()
        }
    }

    pub(crate) fn mark(&mut self, line: usize, start: usize, end: usize, kind: HighlightKind) {
        let span = Span::new(line, start, end);
        if end > start {
            self.highlights.push(Highlight { span, kind });
        }
    }
    pub(crate) fn paint(&mut self, span: Span, kind: HighlightKind) {
        self.mark(span.line, span.start, span.end, kind);
    }
    pub(crate) fn problem(&mut self, span: Span, message: String) {
        self.problems.push(Problem { span, message });
    }

    /// Put the highlights in reading order, one per span.
    pub(crate) fn finish(&mut self) {
        self.highlights
            .sort_by_key(|h| (h.span.line, h.span.start, h.span.end));
        self.highlights.dedup_by_key(|h| h.span);
    }
}
