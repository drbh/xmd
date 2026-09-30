//! A source span, shared by the parser, the evaluator and the editor
//! features: byte offsets from the start of a line, converted to LSP
//! line/UTF-16 coordinates only at the boundary that needs them.
use lsp_types::{Position, Range};

/// Byte offsets from the start of `line`; `end` may extend across later lines.
/// Convert to line/UTF-16 coordinates only at the LSP boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}
impl Span {
    pub fn new(line: usize, start: usize, end: usize) -> Self {
        Self { line, start, end }
    }
    pub fn range(self, text: &(impl Lines + ?Sized)) -> Range {
        let point = |offset| {
            let tail = self.tail(text);
            let prefix = tail.get(..offset).unwrap_or(tail);
            let row = self.line + prefix.bytes().filter(|b| *b == b'\n').count();
            let column = prefix
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .trim_end_matches('\r');
            Position::new(row as u32, column.encode_utf16().count() as u32)
        };
        Range::new(point(self.start), point(self.end))
    }
    fn tail(self, text: &(impl Lines + ?Sized)) -> &str {
        &text.text()[text.line_start(self.line)..]
    }
    pub fn source(self, text: &(impl Lines + ?Sized)) -> &str {
        self.tail(text).get(self.start..self.end).unwrap_or("")
    }
    /// Map expression-relative byte offsets back to their original source line.
    pub fn relative(self, text: &(impl Lines + ?Sized), start: usize, end: usize) -> Self {
        let tail = self.tail(text);
        let prefix = tail.get(..self.start + start).unwrap_or(tail);
        let line = self.line + prefix.bytes().filter(|b| *b == b'\n').count();
        let column = prefix.rsplit('\n').next().unwrap_or("").len();
        Self::new(line, column, column + end.saturating_sub(start))
    }
    pub fn contains(self, text: &(impl Lines + ?Sized), other: Self) -> bool {
        let outer = self.range(text);
        let inner = other.range(text);
        outer.start <= inner.start && inner.end <= outer.end
    }
    pub fn offset_of(self, text: &(impl Lines + ?Sized), other: Self) -> Option<usize> {
        self.contains(text, other).then(|| {
            let lines: usize = self
                .tail(text)
                .split_inclusive('\n')
                .take(other.line - self.line)
                .map(str::len)
                .sum();
            lines + other.start - self.start
        })
    }
    /// Single-line fragments for semantic tokens, excluding newline bytes.
    pub fn fragments(self, text: &(impl Lines + ?Sized)) -> Vec<Self> {
        let mut offset = 0;
        let mut spans = vec![];
        for (row, line) in self.tail(text).split_inclusive('\n').enumerate() {
            if offset >= self.end {
                break;
            }
            let length = line.trim_end_matches(['\r', '\n']).len();
            let start = self.start.saturating_sub(offset);
            let end = self.end.saturating_sub(offset).min(length);
            if start < end {
                spans.push(Self::new(self.line + row, start, end));
            }
            offset += line.len();
        }
        spans
    }
}

/// A text that can say where a line starts. A plain `str` finds out by
/// scanning from the top; a [`LineIndex`] beside the text answers at once,
/// which matters to anything that looks up a line per token.
pub trait Lines {
    fn text(&self) -> &str;
    /// The byte offset where `line` starts, or the text's length past the end.
    fn line_start(&self, line: usize) -> usize;
}
impl Lines for str {
    fn text(&self) -> &str {
        self
    }
    fn line_start(&self, line: usize) -> usize {
        self.split_inclusive('\n').take(line).map(str::len).sum()
    }
}
impl Lines for String {
    fn text(&self) -> &str {
        self
    }
    fn line_start(&self, line: usize) -> usize {
        self.as_str().line_start(line)
    }
}

/// Where each line of a text starts, found in one pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineIndex {
    starts: Vec<usize>,
    len: usize,
}
impl LineIndex {
    pub fn new(text: &str) -> Self {
        let breaks = text.bytes().enumerate().filter(|(_, b)| *b == b'\n');
        Self {
            starts: std::iter::once(0)
                .chain(breaks.map(|(i, _)| i + 1))
                .collect(),
            len: text.len(),
        }
    }
    /// The byte offset where `line` starts, or the text's length past the end.
    pub fn start(&self, line: usize) -> usize {
        self.starts
            .get(line)
            .copied()
            .unwrap_or(self.len)
            .min(self.len)
    }
    /// Line `line` of `text` without its line ending, as `str::lines` gives it.
    pub fn line<'a>(&self, text: &'a str, line: usize) -> &'a str {
        let body = &text[self.start(line)..self.start(line + 1)];
        match body.strip_suffix('\n') {
            Some(body) => body.strip_suffix('\r').unwrap_or(body),
            None => body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_agrees_with_scanning() {
        for text in [
            "",
            "a",
            "a\n",
            "a\nbc\n\nd",
            "a\r\nb\r\n",
            "\n\n",
            "a\r",
            "a\rb\n",
        ] {
            let index = LineIndex::new(text);
            for line in 0..6 {
                assert_eq!(index.start(line), text.line_start(line), "{text:?} {line}");
                assert_eq!(
                    index.line(text, line),
                    text.lines().nth(line).unwrap_or(""),
                    "{text:?} {line}"
                );
            }
        }
    }
}
