//! Semantic colors describe Jot's syntax and types, independent of the editor.
//! Keep parser highlights intact: other LSP features use them to identify inert text.
use crate::{
    document::{Attribute, Document, Named, Span},
    engine::{self, Lexeme, Value},
};
use lsp_types::SemanticToken;
mod prose;

pub const TOKEN_TYPES: &[&str] = &[
    "comment",
    "keyword",
    "number",
    "variable",
    "operator",
    "string",
    "heading",
    "function",
    "property",
    "decorator",
    "jotMoney",
    "jotDate",
    "jotDuration",
    "jotRatio",
    "jotBoolean",
    "jotPunctuation",
    "jotCode",
    "jotLink",
    "jotCheckbox",
    "jotTaskDone",
    "jotCheckboxChecked",
    "jotTime",
];
pub const TOKEN_MODIFIERS: &[&str] = &["declaration", "defaultLibrary"];
const DECLARATION: u32 = 1;
const DEFAULT_LIBRARY: u32 = 2;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    // Zero means unstyled prose. The other values are legend indices plus one.
    kind: u8,
    modifiers: u32,
}
fn style(kind: &str, modifiers: u32) -> Style {
    Style {
        kind: (TOKEN_TYPES.iter().position(|t| *t == kind).unwrap() + 1) as u8,
        modifiers,
    }
}
fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Money(_) => "jotMoney",
        Value::Date(_) | Value::DateTime(_) => "jotDate",
        Value::Duration(_) => "jotDuration",
        Value::Ratio(_) => "jotRatio",
        Value::Bool(_) => "jotBoolean",
        Value::Text(_) => "string",
        Value::Resource(_) => "jotLink",
        Value::Number(_) | Value::Count(_) => "number",
        _ => "variable",
    }
}

struct Painter<'a> {
    lines: Vec<&'a str>,
    colors: Vec<Vec<Style>>,
}
impl<'a> Painter<'a> {
    fn new(doc: &'a Document) -> Self {
        let lines: Vec<_> = doc.text.lines().collect();
        let colors = lines
            .iter()
            .map(|l| vec![Style::default(); l.len()])
            .collect();
        Self { lines, colors }
    }
    fn source(&self, span: Span) -> &'a str {
        self.lines
            .get(span.line)
            .and_then(|l| l.get(span.start..span.end))
            .unwrap_or("")
    }
    fn paint(&mut self, span: Span, color: Style) {
        if let Some(line) = self.colors.get_mut(span.line)
            && let Some(bytes) = line.get_mut(span.start..span.end)
        {
            bytes.fill(color);
        }
    }
    fn mark(&mut self, span: Span, kind: &str) {
        self.paint(span, style(kind, 0));
    }
    fn declaration(&mut self, named: &Named) {
        self.paint(named.span, style("variable", DECLARATION));
        let colon = Span::new(
            named.span.line,
            named.span.start.saturating_sub(1),
            named.span.start,
        );
        if self.source(colon) == ":" {
            self.mark(colon, "jotPunctuation");
        }
    }
    fn brackets(&mut self, span: Span) {
        let line = self.lines[span.line];
        if let Some(open) = line[..span.start].rfind('[') {
            self.mark(Span::new(span.line, open, span.start), "jotPunctuation");
        }
        if let Some(close) = line[span.end..].find(']') {
            self.mark(
                Span::new(span.line, span.end, span.end + close + 1),
                "jotPunctuation",
            );
        }
    }
    fn expression(&mut self, span: Span) {
        let source = self.source(span);
        // An unfinished expression should not suddenly become a solid string color.
        self.paint(span, Style::default());
        let Ok(tokens) = engine::lex(source) else {
            return;
        };
        for (i, token) in tokens.iter().enumerate() {
            let mut modifiers = 0;
            let kind = match &token.kind {
                Lexeme::Value(value) => value_kind(value),
                Lexeme::Name(name) => {
                    if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left)) {
                        if crate::intelligence::is_builtin_function(name) {
                            modifiers = DEFAULT_LIBRARY;
                        }
                        "function"
                    } else if matches!(name.as_str(), "true" | "false") {
                        "jotBoolean"
                    } else if i > 0 && matches!(tokens[i - 1].kind, Lexeme::Dot)
                        || engine::sum_scope_at(source, token.start).is_some()
                    {
                        "property"
                    } else {
                        "variable"
                    }
                }
                Lexeme::Op(_) => "operator",
                _ => "jotPunctuation",
            };
            self.paint(
                Span::new(span.line, span.start + token.start, span.start + token.end),
                style(kind, modifiers),
            );
        }
    }
    fn attribute(&mut self, name: &str, attr: &Attribute) {
        self.mark(attr.span, "jotPunctuation");
        self.mark(
            Span::new(attr.span.line, attr.span.start, attr.value_span.start - 1),
            "decorator",
        );
        if matches!(
            name,
            "due" | "scheduled" | "at" | "completed" | "repeat_from"
        ) && engine::relative_date(
            &attr.value,
            chrono::NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(),
        )
        .is_some()
        {
            self.mark(attr.value_span, "jotDate");
        } else if matches!(name, "tag" | "every") {
            self.mark(attr.value_span, "string");
        } else {
            self.expression(attr.value_span);
        }
    }
    fn prose_values(&mut self) {
        let heading = style("heading", 0);
        for row in 0..self.lines.len() {
            for (start, end, kind) in prose::values(self.lines[row]) {
                // Never recolor links, code, comments, names, formulae, or cells.
                if self.colors[row][start..end]
                    .iter()
                    .all(|s| s.kind == 0 || *s == heading)
                {
                    self.mark(Span::new(row, start, end), kind);
                }
            }
        }
    }
    fn finish(self) -> Vec<SemanticToken> {
        let mut result = Vec::new();
        let (mut previous_line, mut previous_start) = (0, 0);
        for (row, line) in self.lines.iter().enumerate() {
            let mut start = 0;
            let mut character = 0;
            let mut active = Style::default();
            // Iterate Unicode scalar boundaries, then encode positions in UTF-16.
            for (byte, ch) in line
                .char_indices()
                .chain(std::iter::once((line.len(), '\0')))
            {
                let next = self.colors[row].get(byte).copied().unwrap_or_default();
                if next != active {
                    if active.kind != 0 {
                        let delta_line = row as u32 - previous_line;
                        result.push(SemanticToken {
                            delta_line,
                            delta_start: if delta_line == 0 {
                                start - previous_start
                            } else {
                                start
                            },
                            length: character - start,
                            token_type: (active.kind - 1) as u32,
                            token_modifiers_bitset: active.modifiers,
                        });
                        previous_line = row as u32;
                        previous_start = start;
                    }
                    start = character;
                    active = next;
                }
                character += ch.len_utf16() as u32;
            }
        }
        result
    }
}

pub fn semantic_tokens(doc: &Document) -> Vec<SemanticToken> {
    let mut p = Painter::new(doc);
    for h in &doc.highlights {
        p.mark(
            h.span,
            if h.kind == "string" {
                "jotCode"
            } else {
                h.kind
            },
        );
    }
    p.prose_values();
    for section in &doc.sections {
        let line = p.lines[section.line];
        let start = line.len() - line.trim_start().len();
        p.mark(
            Span::new(section.line, start, start + section.level),
            "jotPunctuation",
        );
        if let Some(named) = &section.named {
            p.declaration(named);
        }
    }
    for task in &doc.tasks {
        p.mark(
            Span::new(task.line, task.indent, task.indent + 1),
            "jotPunctuation",
        );
        p.mark(
            task.checkbox,
            if task.checked {
                "jotCheckboxChecked"
            } else {
                "jotCheckbox"
            },
        );
        if task.checked {
            let end = task
                .attributes
                .values()
                .map(|a| a.span.start)
                .chain(task.named.iter().map(|n| n.span.start - 1))
                .min()
                .unwrap_or(p.lines[task.line].len());
            p.mark(Span::new(task.line, task.checkbox.end, end), "jotTaskDone");
        }
        if let Some(named) = &task.named {
            p.declaration(named);
        }
    }
    for def in &doc.definitions {
        p.declaration(&def.named);
        if def.expression {
            p.brackets(def.named.span);
            if def.source == "table" {
                p.mark(def.value_span, "keyword");
            } else {
                p.expression(def.value_span);
            }
        } else {
            p.brackets(def.value_span);
            p.mark(
                def.value_span,
                engine::literal(&def.source)
                    .as_ref()
                    .map(value_kind)
                    .unwrap_or("string"),
            );
        }
    }
    for (name, attr) in doc
        .tasks
        .iter()
        .flat_map(|t| &t.attributes)
        .chain(doc.events.iter().flat_map(|e| &e.attributes))
    {
        p.attribute(name, attr);
    }
    for reference in doc.references.iter().filter(|r| r.bracket) {
        p.brackets(Span::new(
            reference.span.line,
            reference.span.start,
            reference.end(),
        ));
        p.mark(reference.span, "variable");
        if reference.property.is_some() {
            p.mark(
                Span::new(
                    reference.span.line,
                    reference.span.end,
                    reference.span.end + 1,
                ),
                "jotPunctuation",
            );
            p.mark(
                Span::new(reference.span.line, reference.span.end + 1, reference.end()),
                "property",
            );
        }
    }
    for table in &doc.tables {
        // Paint separators first: quoted and escaped pipes inside cells are data.
        for row in table.header..table.end_line {
            for (byte, ch) in p.lines[row].char_indices() {
                if ch == '|' {
                    p.mark(Span::new(row, byte, byte + 1), "jotPunctuation");
                }
            }
        }
        if !table.separators.is_empty() {
            let row = table.header + 1;
            p.mark(Span::new(row, 0, p.lines[row].len()), "jotPunctuation");
        }
        for column in &table.columns {
            p.paint(column.span, style("property", DECLARATION));
        }
        for cell in table.rows.iter().flatten() {
            p.mark(
                cell.span,
                cell.value.as_ref().map(value_kind).unwrap_or("string"),
            );
        }
    }
    for link in &doc.links {
        p.mark(link.span, "jotLink");
        let raw = p.source(link.span);
        if raw.starts_with('[')
            && let Some(close) = raw.find("](")
        {
            for (start, end) in [(0, 1), (close, close + 2), (raw.len() - 1, raw.len())] {
                p.mark(
                    Span::new(
                        link.span.line,
                        link.span.start + start,
                        link.span.start + end,
                    ),
                    "jotPunctuation",
                );
            }
        }
    }
    // Metadata-looking text inside comments must never become active highlighting.
    for h in doc.highlights.iter().filter(|h| h.kind == "comment") {
        p.mark(h.span, "comment");
    }
    p.finish()
}
