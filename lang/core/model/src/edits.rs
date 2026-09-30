//! Applying LSP text edits to a note's source, the one check every proposed
//! edit or position passes before anything reaches a host.
use lsp_types::{Position, TextEdit};

/// The UTF-16 column of byte `byte` in `line`: what an LSP position counts.
pub fn utf16(line: &str, byte: usize) -> u32 {
    line.get(..byte).unwrap_or(line).encode_utf16().count() as u32
}
/// The byte of UTF-16 column `character` in `line`, unless it falls inside
/// a character.
pub fn byte_at(line: &str, character: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, c) in line.char_indices() {
        if units == character {
            return Some(byte);
        }
        units += c.len_utf16() as u32;
        if units > character {
            return None;
        }
    }
    (units == character).then_some(line.len())
}

/// The position just past the last character, where an edit may append.
pub fn end_position(text: &str) -> Position {
    if text.ends_with('\n') {
        Position::new(text.lines().count() as u32, 0)
    } else {
        let lines: Vec<_> = text.lines().collect();
        Position::new(
            lines.len().saturating_sub(1) as u32,
            lines.last().unwrap_or(&"").encode_utf16().count() as u32,
        )
    }
}
/// Where each line starts, so a position resolves to a byte offset without
/// rescanning the text.
pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
}
impl<'a> LineIndex<'a> {
    pub fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { text, starts }
    }
    /// The byte offset of every line, the empty one after a final newline
    /// included.
    pub fn starts(&self) -> &[usize] {
        &self.starts
    }
    pub fn offset(&self, pos: Position) -> Result<usize, String> {
        let row = pos.line as usize;
        let Some(&start) = self.starts.get(row) else {
            return Err("Invalid edit position".into());
        };
        let end = self
            .starts
            .get(row + 1)
            .map_or(self.text.len(), |next| next - 1);
        if start == end && row + 1 == self.starts.len() {
            // Past the final newline only the end of the text is a position.
            return if pos.character == 0 {
                Ok(start)
            } else {
                Err("Invalid edit position".into())
            };
        }
        byte_at(self.text[start..end].trim_end_matches('\r'), pos.character)
            .map(|n| start + n)
            .ok_or("Invalid UTF-16 character boundary".into())
    }
}

pub fn apply_edits(text: &str, edits: &[TextEdit]) -> Result<String, String> {
    let index = LineIndex::new(text);
    let mut replacements = edits
        .iter()
        .map(|e| {
            if e.range.start > e.range.end {
                return Err("Reversed edit range".into());
            }
            Ok((
                index.offset(e.range.start)?,
                index.offset(e.range.end)?,
                e.new_text.clone(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    replacements.sort_by_key(|(start, end, _)| (*start, *end));
    for pair in replacements.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("Overlapping edits".into());
        }
    }
    let mut result = text.to_string();
    for (start, end, new) in replacements.into_iter().rev() {
        result.replace_range(start..end, &new);
    }
    Ok(result)
}
