use lsp_types::{Position, Range};
use std::collections::BTreeMap;

/// Source spans are byte offsets within a line. Convert only at the LSP boundary.
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
        let line = text.lines().nth(self.line).unwrap_or("");
        Range::new(
            Position::new(self.line as u32, utf16(line, self.start)),
            Position::new(self.line as u32, utf16(line, self.end)),
        )
    }
}
pub fn utf16(line: &str, byte: usize) -> u32 {
    line.get(..byte).unwrap_or(line).encode_utf16().count() as u32
}
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
pub fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn name_len(s: &str) -> usize {
    s.bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
        .count()
}

#[derive(Clone, Debug)]
pub struct Named {
    pub name: String,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub named: Named,
    pub source: String,
    pub expression: bool,
    pub value_span: Span,
    pub end: Span,
}
#[derive(Clone, Debug)]
pub struct Reference {
    pub name: String,
    pub span: Span,
    pub bracket: bool,
    pub property: Option<String>,
}
impl Reference {
    pub fn expression(&self) -> String {
        self.property
            .as_ref()
            .map(|p| format!("{}.{p}", self.name))
            .unwrap_or_else(|| self.name.clone())
    }
    pub fn end(&self) -> usize {
        self.span.end + self.property.as_ref().map(|p| p.len() + 1).unwrap_or(0)
    }
}
#[derive(Clone, Debug)]
pub struct Attribute {
    pub value: String,
    pub span: Span,
    pub value_span: Span,
}
#[derive(Clone, Debug)]
pub struct Task {
    pub line: usize,
    pub indent: usize,
    pub checked: bool,
    pub checkbox: Span,
    pub title: String,
    pub named: Option<Named>,
    pub parent: Option<usize>,
    pub attributes: BTreeMap<String, Attribute>,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Section {
    pub line: usize,
    pub end_line: usize,
    pub level: usize,
    pub title: String,
    pub named: Option<Named>,
}
#[derive(Clone, Debug)]
pub struct Event {
    pub line: usize,
    pub title: String,
    pub attributes: BTreeMap<String, Attribute>,
}
#[derive(Clone, Debug)]
pub struct Link {
    pub span: Span,
    pub target: String,
}
#[derive(Clone, Debug)]
pub struct Highlight {
    pub span: Span,
    pub kind: &'static str,
}
#[derive(Clone, Debug)]
pub struct Problem {
    pub span: Span,
    pub message: String,
}
#[derive(Clone, Debug, Default)]
pub struct Document {
    pub text: String,
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    pub tasks: Vec<Task>,
    pub sections: Vec<Section>,
    pub events: Vec<Event>,
    pub tables: Vec<crate::tables::Table>,
    pub links: Vec<Link>,
    pub highlights: Vec<Highlight>,
    pub problems: Vec<Problem>,
}

impl Document {
    pub fn parse(text: String) -> Self {
        let mut doc = Self {
            text: text.clone(),
            ..Self::default()
        };
        let mut fence: Option<(char, usize)> = None;
        let mut comment = false;
        let mut parents: Vec<usize> = Vec::new();
        let lines: Vec<_> = text.lines().collect();
        let mut table_end = 0;
        for (row, line) in lines.iter().copied().enumerate() {
            if row < table_end {
                continue;
            }
            let start = line.len() - line.trim_start().len();
            let trimmed = &line[start..];
            let marker = trimmed.chars().next().unwrap_or(' ');
            let run = trimmed.chars().take_while(|c| *c == marker).count();
            if let Some((kind, count)) = fence {
                if marker == kind && run >= count && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
                doc.mark(row, start, line.len(), "string");
                continue;
            }
            if (marker == '`' || marker == '~') && run >= 3 {
                fence = Some((marker, run));
                doc.mark(row, start, line.len(), "string");
                continue;
            }
            if comment || trimmed.starts_with("<!--") {
                comment = !trimmed.contains("-->");
                doc.mark(row, start, line.len(), "comment");
                continue;
            }
            let heading = marker == '#'
                && run <= 6
                && trimmed
                    .as_bytes()
                    .get(run)
                    .is_some_and(u8::is_ascii_whitespace);
            if heading {
                parents.clear();
                for section in &mut doc.sections {
                    if section.end_line == usize::MAX && section.level >= run {
                        section.end_line = row;
                    }
                }
                let named = trailing_name(line, row);
                let title_end = named
                    .as_ref()
                    .map(|n| n.span.start - 1)
                    .unwrap_or(line.len());
                doc.sections.push(Section {
                    line: row,
                    end_line: usize::MAX,
                    level: run,
                    title: line[start + run..title_end].trim().into(),
                    named: named.clone(),
                });
                doc.mark(row, start, title_end, "heading");
                if let Some(n) = named {
                    doc.mark(row, n.span.start, n.span.end, "variable");
                }
                doc.raw_links(line, row, start + run, title_end);
                continue;
            }
            let checkbox_start = ["- [", "* [", "+ ["]
                .iter()
                .find(|prefix| trimmed.starts_with(**prefix))
                .map(|_| start + 2);
            let is_task = checkbox_start.filter(|s| {
                matches!(line.as_bytes().get(s + 1), Some(b' ' | b'x' | b'X'))
                    && line.as_bytes().get(s + 2) == Some(&b']')
                    && line
                        .as_bytes()
                        .get(s + 3)
                        .is_none_or(u8::is_ascii_whitespace)
            });
            let content_start = is_task.map(|s| s + 3).unwrap_or(start);
            let attrs = doc.attributes(line, row, content_start);
            if let Some(s) = is_task {
                let named = trailing_name(line, row);
                while parents
                    .last()
                    .is_some_and(|i| doc.tasks[*i].indent >= start)
                {
                    parents.pop();
                }
                let title_end = attrs
                    .values()
                    .map(|a| a.span.start)
                    .chain(named.iter().map(|n| n.span.start - 1))
                    .min()
                    .unwrap_or(line.len());
                let tags = line
                    .split_whitespace()
                    .filter_map(|t| t.strip_prefix('#'))
                    .filter(|t| identifier(t))
                    .map(str::to_owned)
                    .chain(
                        attrs
                            .get("tag")
                            .into_iter()
                            .flat_map(|a| a.value.split(',').map(|s| s.trim().to_string())),
                    )
                    .collect();
                doc.tasks.push(Task {
                    line: row,
                    indent: start,
                    checked: line.as_bytes()[s + 1] != b' ',
                    checkbox: Span::new(row, s, s + 3),
                    title: line[s + 3..title_end].trim().into(),
                    named: named.clone(),
                    parent: parents.last().copied(),
                    attributes: attrs.clone(),
                    tags,
                });
                parents.push(doc.tasks.len() - 1);
                doc.mark(row, s, s + 3, "keyword");
                if let Some(n) = named {
                    doc.mark(row, n.span.start, n.span.end, "variable");
                }
            } else if attrs.contains_key("at") {
                let end = attrs
                    .values()
                    .map(|a| a.span.start)
                    .min()
                    .unwrap_or(line.len());
                doc.events.push(Event {
                    line: row,
                    title: line[start..end].trim().trim_start_matches("- ").into(),
                    attributes: attrs.clone(),
                });
            }
            doc.inline(line, row, content_start, &attrs);
            if let Some(index) = doc.definitions.len().checked_sub(1)
                && doc.definitions[index].named.span.line == row
                && doc.definitions[index].expression
                && doc.definitions[index].source == "table"
            {
                let table = crate::tables::parse(&doc, index, &lines);
                table_end = table.end_line;
                // The declaration keyword isn't a global reference.
                doc.references
                    .retain(|r| !(r.span.line == row && r.name == "table"));
                for column in &table.columns {
                    doc.mark(
                        column.span.line,
                        column.span.start,
                        column.span.end,
                        "variable",
                    );
                }
                for cells in &table.rows {
                    for cell in cells {
                        if let Ok(crate::engine::Value::Resource(resource)) = &cell.value {
                            doc.links.push(Link {
                                span: cell.span,
                                target: resource.target.clone(),
                            });
                        }
                        doc.mark(
                            cell.span.line,
                            cell.span.start,
                            cell.span.end,
                            if matches!(
                                &cell.value,
                                Ok(crate::engine::Value::Text(_)
                                    | crate::engine::Value::Resource(_))
                            ) {
                                "string"
                            } else {
                                "number"
                            },
                        );
                    }
                }
                doc.problems.extend(table.problems.clone());
                doc.tables.push(table);
            }
        }
        let lines = text.lines().count();
        for section in &mut doc.sections {
            if section.end_line == usize::MAX {
                section.end_line = lines;
            }
        }
        doc.highlights
            .sort_by_key(|h| (h.span.line, h.span.start, h.span.end));
        doc.highlights.dedup_by_key(|h| h.span);
        doc
    }

    fn mark(&mut self, line: usize, start: usize, end: usize, kind: &'static str) {
        if end > start {
            self.highlights.push(Highlight {
                span: Span::new(line, start, end),
                kind,
            });
        }
    }

    fn raw_link(&mut self, line: &str, row: usize, start: usize) -> Option<usize> {
        let end = crate::resources::raw_link_end(line, start)?;
        self.links.push(Link {
            span: Span::new(row, start, end),
            target: line[start..end].into(),
        });
        self.mark(row, start, end, "string");
        Some(end)
    }
    fn raw_links(&mut self, line: &str, row: usize, mut start: usize, end: usize) {
        let line = &line[..end];
        while start < end {
            if line[start..].starts_with("<!--") {
                break;
            }
            if line.as_bytes()[start] == b'`' {
                start = skip_code(line, start);
                continue;
            }
            if let Some(next) = self.raw_link(line, row, start) {
                start = next;
            } else {
                start += line[start..].chars().next().unwrap().len_utf8();
            }
        }
    }

    fn attributes(&mut self, line: &str, row: usize, start: usize) -> BTreeMap<String, Attribute> {
        let mut attrs = BTreeMap::new();
        let mut i = start;
        while i < line.len() {
            if line.as_bytes()[i] == b'`' {
                i = skip_code(line, i);
                continue;
            }
            if let Some(end) = crate::resources::raw_link_end(line, i) {
                i = end;
                continue;
            }
            if line.as_bytes()[i] != b'@' {
                i += line[i..].chars().next().unwrap().len_utf8();
                continue;
            }
            let len = name_len(&line[i + 1..]);
            let open = i + 1 + len;
            if len == 0 || line.as_bytes().get(open) != Some(&b'(') {
                i += 1;
                continue;
            }
            let mut depth = 1;
            let mut end = open + 1;
            while end < line.len() && depth > 0 {
                match line.as_bytes()[end] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            if depth != 0 {
                self.problems.push(Problem {
                    span: Span::new(row, i, line.len()),
                    message: "Unclosed task attribute".into(),
                });
                break;
            }
            let key = &line[i + 1..open];
            let attr = Attribute {
                value: line[open + 1..end - 1].trim().into(),
                span: Span::new(row, i, end),
                value_span: Span::new(row, open + 1, end - 1),
            };
            self.mark(row, i, open + 1, "keyword");
            self.mark(row, end - 1, end, "operator");
            if matches!(key, "due" | "scheduled" | "at")
                && crate::engine::relative_date(&attr.value, chrono::Local::now().date_naive())
                    .is_some()
            {
                self.mark(row, open + 1, end - 1, "number");
            } else if matches!(
                key,
                "due" | "scheduled" | "at" | "estimate" | "after" | "timer"
            ) {
                self.expression(line, row, open + 1, end - 1);
            } else {
                self.mark(row, open + 1, end - 1, "string");
            }
            if attrs.insert(key.into(), attr).is_some() {
                self.problems.push(Problem {
                    span: Span::new(row, i, end),
                    message: format!("Duplicate @{key} attribute"),
                });
            }
            if !matches!(
                key,
                "due"
                    | "scheduled"
                    | "at"
                    | "estimate"
                    | "after"
                    | "every"
                    | "tag"
                    | "completed"
                    | "repeat_from"
                    | "timer"
            ) {
                self.problems.push(Problem {
                    span: Span::new(row, i, end),
                    message: format!("Unknown attribute @{key}"),
                });
            }
            i = end;
        }
        attrs
    }

    fn inline(
        &mut self,
        line: &str,
        row: usize,
        start: usize,
        attrs: &BTreeMap<String, Attribute>,
    ) {
        let mut i = start;
        while i < line.len() {
            if let Some(attr) = attrs.values().find(|a| a.span.start == i) {
                i = attr.span.end;
                continue;
            }
            if line[i..].starts_with("<!--") {
                let end = line[i..]
                    .find("-->")
                    .map(|o| i + o + 3)
                    .unwrap_or(line.len());
                self.mark(row, i, end, "comment");
                i = end;
                continue;
            }
            if line.as_bytes()[i] == b'`' {
                let end = skip_code(line, i);
                self.mark(row, i, end, "string");
                i = end;
                continue;
            }
            if let Some(end) = self.raw_link(line, row, i) {
                i = end;
                continue;
            }
            if line.as_bytes()[i] != b'[' {
                i += line[i..].chars().next().unwrap().len_utf8();
                continue;
            }
            let Some(close) = line[i + 1..].find(']').map(|n| i + 1 + n) else {
                break;
            };
            let inner = line[i + 1..close].trim();
            let inner_start = i + 1 + line[i + 1..close].find(inner).unwrap_or(0);
            if line.as_bytes().get(close + 1) == Some(&b'(')
                && let Some(end) = line[close + 2..].find(')').map(|o| close + 2 + o)
            {
                self.links.push(Link {
                    span: Span::new(row, i, end + 1),
                    target: line[close + 2..end].to_string(),
                });
                self.mark(row, i, end + 1, "string");
                i = end + 1;
                continue;
            }
            let tail = &line[close + 1..];
            let after = close + 1 + tail.len() - tail.trim_start().len();
            if line[after..].starts_with(":=") && identifier(inner) {
                let expr_start = after + 2;
                self.definitions.push(Definition {
                    named: Named {
                        name: inner.into(),
                        span: Span::new(row, inner_start, inner_start + inner.len()),
                    },
                    source: line[expr_start..].trim().into(),
                    expression: true,
                    value_span: Span::new(row, expr_start, line.len()),
                    end: Span::new(row, line.len(), line.len()),
                });
                self.mark(row, i, close + 1, "variable");
                self.mark(row, after, after + 2, "operator");
                self.expression(line, row, expr_start, line.len());
                break;
            }
            // No whitespace between ] and : in literal definitions, to avoid prose ambiguity.
            if line.as_bytes().get(close + 1) == Some(&b':') {
                let len = name_len(&line[close + 2..]);
                let name = &line[close + 2..close + 2 + len];
                if identifier(name) {
                    self.definitions.push(Definition {
                        named: Named {
                            name: name.into(),
                            span: Span::new(row, close + 2, close + 2 + len),
                        },
                        source: inner.into(),
                        expression: false,
                        value_span: Span::new(row, inner_start, inner_start + inner.len()),
                        end: Span::new(row, close + 2 + len, close + 2 + len),
                    });
                    self.mark(
                        row,
                        i,
                        close + 1,
                        if inner.starts_with('$')
                            || inner.chars().next().is_some_and(|c| c.is_ascii_digit())
                        {
                            "number"
                        } else {
                            "string"
                        },
                    );
                    self.mark(row, close + 1, close + 2, "operator");
                    self.mark(row, close + 2, close + 2 + len, "variable");
                    i = close + 2 + len;
                    continue;
                }
            }
            let (name, property) = inner
                .split_once('.')
                .map(|(n, p)| (n, Some(p)))
                .unwrap_or((inner, None));
            if identifier(name) && property.is_none_or(identifier) {
                self.references.push(Reference {
                    name: name.into(),
                    span: Span::new(row, inner_start, inner_start + name.len()),
                    bracket: true,
                    property: property.map(str::to_string),
                });
                self.mark(row, i, close + 1, "variable");
            }
            i = close + 1;
        }
    }

    fn expression(&mut self, line: &str, row: usize, start: usize, end: usize) {
        // Use the evaluator's lexer, so identifiers and dates have identical boundaries.
        match crate::engine::lex(&line[start..end]) {
            Ok(tokens) => {
                for token in tokens {
                    let span = Span::new(row, start + token.start, start + token.end);
                    let kind = match &token.kind {
                        crate::engine::Lexeme::Name(name) => {
                            if !line[start + token.end..end].trim_start().starts_with('(')
                                && (token.start == 0
                                    || !line[start..start + token.start].trim_end().ends_with('.'))
                                && !matches!(name.as_str(), "true" | "false")
                                && (!matches!(name.as_str(), "tomorrow" | "today")
                                    || crate::engine::sum_scope_at(&line[start..end], token.start)
                                        .is_some())
                            {
                                self.references.push(Reference {
                                    name: name.clone(),
                                    span,
                                    bracket: false,
                                    property: None,
                                });
                            }
                            "variable"
                        }
                        crate::engine::Lexeme::Value(_) => "number",
                        _ => "operator",
                    };
                    self.mark(row, span.start, span.end, kind);
                }
            }
            Err(_) => {
                self.mark(row, start, end, "string");
            }
        }
    }
    pub fn line(&self, row: usize) -> &str {
        self.text.lines().nth(row).unwrap_or("")
    }
    pub fn line_end(&self, row: usize) -> Position {
        Position::new(row as u32, utf16(self.line(row), self.line(row).len()))
    }
}

fn skip_code(line: &str, start: usize) -> usize {
    let count = line[start..].bytes().take_while(|c| *c == b'`').count();
    let marker = "`".repeat(count);
    line[start + count..]
        .find(&marker)
        .map(|n| start + count + n + count)
        .unwrap_or(line.len())
}
fn trailing_name(line: &str, row: usize) -> Option<Named> {
    // A task/heading name occupies a whitespace-delimited :identifier field.
    let limit = line.find(" @").unwrap_or(line.len());
    let text = line[..limit].trim_end();
    let at = text.rfind(" :")? + 2;
    let name = &text[at..];
    identifier(name).then(|| Named {
        name: name.into(),
        span: Span::new(row, at, at + name.len()),
    })
}
