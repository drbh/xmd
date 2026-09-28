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
    pub fn range(self, text: &str) -> Range {
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
    fn tail(self, text: &str) -> &str {
        let offset: usize = text
            .split_inclusive('\n')
            .take(self.line)
            .map(str::len)
            .sum();
        &text[offset..]
    }
    pub fn source(self, text: &str) -> &str {
        self.tail(text).get(self.start..self.end).unwrap_or("")
    }
    /// Map expression-relative byte offsets back to their original source line.
    pub fn relative(self, text: &str, start: usize, end: usize) -> Self {
        let tail = self.tail(text);
        let prefix = tail.get(..self.start + start).unwrap_or(tail);
        let line = self.line + prefix.bytes().filter(|b| *b == b'\n').count();
        let column = prefix.rsplit('\n').next().unwrap_or("").len();
        Self::new(line, column, column + end.saturating_sub(start))
    }
    pub fn contains(self, text: &str, other: Self) -> bool {
        let outer = self.range(text);
        let inner = other.range(text);
        outer.start <= inner.start && inner.end <= outer.end
    }
    pub fn offset_of(self, text: &str, other: Self) -> Option<usize> {
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
    pub fn fragments(self, text: &str) -> Vec<Self> {
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
