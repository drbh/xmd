//! What a note's lines are as document structure. A line is a fence, a
//! comment, a heading (level, title, optional `:name`), a list item
//! (indentation, optional checkbox), a table row, prose or blank, decided
//! before anything is read from it; the inline forms inside prose and list
//! items are read in `inline`.
//!
//! Nothing here knows what a task, event, section, table, form or itinerary
//! is: the recognizers (listed in `recognizers`) read these blocks and fill
//! the note's features into the shared `Tree`.
use crate::inline::{Attributes, Named};
use common::Span;
use syntax::identifier;

/// `## Title :name`: its indentation, level, title and trailing `:name`.
pub(crate) struct Heading<'a> {
    pub(crate) row: usize,
    pub(crate) start: usize,
    pub(crate) level: usize,
    pub(crate) title: &'a str,
    pub(crate) title_end: usize,
    pub(crate) named: Option<Named>,
}
/// A list item's `[ ]`, `[-]` or `[x]`: where its `[` is and the mark inside.
#[derive(Clone, Copy)]
pub(crate) struct Checkbox {
    pub(crate) at: usize,
    pub(crate) mark: u8,
}
/// What a line is, decided before anything is read from it. A `start` is
/// the line's indentation.
pub(crate) enum Block<'a> {
    /// A fence delimiter, or a line inside an open fence.
    Fence(usize),
    /// An HTML comment line (opening, inside or closing) or a `//` line.
    Comment(usize),
    Heading(Heading<'a>),
    /// `- text`, `* text`, `+ text`, or `- [ ] text` with a checkbox.
    Item {
        start: usize,
        checkbox: Option<Checkbox>,
    },
    /// `| a | b |`
    Row(usize),
    /// Whitespace only: it says nothing about the note.
    Blank,
    /// Everything else.
    Prose(usize),
}

/// All a line needs to know about the lines before it. A block a recognizer
/// reads over several rows (a table, a form's table, a continued expression) is not
/// held here: whoever read those rows reports how many it took, and they
/// never reach the classifier.
#[derive(Default)]
pub(crate) struct BlockState {
    /// The fence marker and the length of its run, while a fence is open.
    fence: Option<(char, usize)>,
    /// Whether an HTML comment is still open.
    comment: bool,
}

/// Decide what a line is and carry the fence or comment it leaves open.
pub(crate) fn classify<'a>(line: &'a str, row: usize, state: &mut BlockState) -> Block<'a> {
    let start = line.len() - line.trim_start().len();
    let trimmed = &line[start..];
    let marker = trimmed.chars().next().unwrap_or(' ');
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    if let Some((kind, count)) = state.fence {
        let closes = marker == kind && run >= count && trimmed[run..].trim().is_empty();
        state.fence = (!closes).then_some((kind, count));
        return Block::Fence(start);
    }
    if (marker == '`' || marker == '~') && run >= 3 {
        state.fence = Some((marker, run));
        return Block::Fence(start);
    }
    if state.comment || trimmed.starts_with("<!--") {
        state.comment = !trimmed.contains("-->");
        return Block::Comment(start);
    }
    if trimmed.starts_with("//") {
        return Block::Comment(start);
    }
    if marker == '#'
        && run <= 6
        && trimmed
            .as_bytes()
            .get(run)
            .is_some_and(u8::is_ascii_whitespace)
    {
        let named = trailing_name(line, row);
        let title_end = named
            .as_ref()
            .map(|n| n.span.start - 1)
            .unwrap_or(line.len());
        return Block::Heading(Heading {
            row,
            start,
            level: run,
            title: line[start + run..title_end].trim(),
            title_end,
            named,
        });
    }
    if trimmed.is_empty() {
        return Block::Blank;
    }
    if marker == '|' {
        return Block::Row(start);
    }
    let bytes = line.as_bytes();
    let item = matches!(marker, '-' | '*' | '+')
        && bytes.get(start + 1).is_none_or(u8::is_ascii_whitespace);
    if !item {
        return Block::Prose(start);
    }
    let at = start + 2;
    let checkbox = (bytes[start + 1..].starts_with(b" [")
        && matches!(bytes.get(at + 1), Some(b' ' | b'-' | b'x' | b'X'))
        && bytes.get(at + 2) == Some(&b']')
        && bytes.get(at + 3).is_none_or(u8::is_ascii_whitespace))
    .then(|| Checkbox {
        at,
        mark: bytes[at + 1],
    });
    Block::Item { start, checkbox }
}

/// A line of prose, a list item or a table row, with its `@key(value)`
/// attributes already found: what recognizers of such lines are handed.
pub(crate) struct Line<'a> {
    pub(crate) text: &'a str,
    pub(crate) row: usize,
    /// The indentation.
    pub(crate) start: usize,
    pub(crate) checkbox: Option<Checkbox>,
    /// Where the inline forms begin: past a checkbox, otherwise at the
    /// indentation (a plain list marker is read as prose).
    pub(crate) body: usize,
    /// Which block it is, and where its text starts: past a list marker and
    /// any checkbox, at a row's `|`, past prose's indentation.
    pub(crate) on: crate::declared::On,
    pub(crate) from: usize,
    pub(crate) attributes: Attributes<'a>,
    /// Where its title ends: at its first attribute, or a checklist item's
    /// trailing `:name`.
    pub(crate) title_end: usize,
}

pub(crate) fn trailing_name(line: &str, row: usize) -> Option<Named> {
    // A task/heading name occupies a whitespace-delimited :identifier field.
    let limit = line.find(" @").unwrap_or(line.len());
    let text = line[..limit].trim_end();
    let at = text.rfind(" :")? + 2;
    let name = &text[at..];
    identifier(name).then(|| Named::new(name, row, at))
}
/// Pipes in quoted strings and escaped pipes are cell contents, not separators.
pub fn cells(line: &str, row: usize) -> Option<Vec<(String, Span)>> {
    let start = line.len() - line.trim_start().len();
    let end = line.trim_end().len();
    if !line[start..].starts_with('|') || end <= start + 1 {
        return None;
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut last = start + 1;
    let mut result = Vec::new();
    for (i, c) in line.char_indices().filter(|(i, _)| *i > start && *i < end) {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if c == '|' && !quoted {
            let raw = &line[last..i];
            let from = last + raw.len() - raw.trim_start().len();
            let to = from + raw.trim().len();
            result.push((raw.trim().into(), Span::new(row, from, to)));
            last = i + 1;
        }
    }
    (last == end && !quoted).then_some(result)
}
