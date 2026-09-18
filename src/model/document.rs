use crate::resources::Resource;
use lsp_types::{Position, Range};
use std::collections::BTreeMap;

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
/// `[remaining / budget]` in prose: a calculation shown in place, without a name.
#[derive(Clone, Debug)]
pub struct Calculation {
    /// The expression: inside the brackets, or the whole line for a line of
    /// math whose variables are bracketed.
    pub span: Span,
    pub source: String,
    /// True for `[a / b]` in prose; false for a whole-line `[a] / [b]`.
    pub bracketed: bool,
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
    pub plans: Vec<crate::plans::Plan>,
    pub days: Vec<crate::itinerary::Day>,
    pub links: Vec<Link>,
    pub calculations: Vec<Calculation>,
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
            if trimmed.starts_with("//") {
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
                && doc.definitions[index].expression
                && doc.definitions[index].named.span.line == row
            {
                let span = doc.definitions[index].value_span;
                let end_row = expression_end(&lines, row, span.start);
                if end_row > row {
                    let prefix: usize = text.split_inclusive('\n').take(row).map(str::len).sum();
                    let length: usize = text[prefix..]
                        .split_inclusive('\n')
                        .take(end_row - row)
                        .map(str::len)
                        .sum();
                    let end = length + lines[end_row].len();
                    let block = &text[prefix..prefix + end];
                    doc.references
                        .retain(|r| r.span.line != row || r.span.start < span.start);
                    doc.highlights
                        .retain(|h| h.span.line != row || h.span.end <= span.start);
                    doc.definitions[index].source = block[span.start..].trim().into();
                    doc.definitions[index].value_span.end = end;
                    doc.definitions[index].end =
                        Span::new(end_row, lines[end_row].len(), lines[end_row].len());
                    doc.expression(block, row, span.start, end);
                    table_end = end_row + 1;
                }
            }
            // A line that is only math, with its variables in brackets, shows its
            // result at the end: `[budget] - [spent]`.
            if is_task.is_none()
                && attrs.is_empty()
                && let Some(source) = line_calculation(trimmed)
            {
                doc.calculations.push(Calculation {
                    span: Span::new(row, start, start + trimmed.trim_end().len()),
                    source,
                    bracketed: false,
                });
            }
            if let Some(index) = doc.definitions.len().checked_sub(1)
                && doc.definitions[index].named.span.line == row
                && doc.definitions[index].expression
                && crate::plans::goal(&doc.definitions[index].source).is_some()
            {
                let mut plan = crate::plans::parse(&doc, index, &lines);
                table_end = plan.end_line;
                for column in &plan.columns {
                    doc.mark(
                        column.span.line,
                        column.span.start,
                        column.span.end,
                        "keyword",
                    );
                }
                for constraint in &plan.constraints {
                    let n = &constraint.named;
                    doc.mark(n.span.line, n.span.start, n.span.end, "variable");
                    doc.expression(
                        lines[constraint.span.line],
                        constraint.span.line,
                        constraint.span.start,
                        constraint.span.end,
                    );
                }
                for reference in &doc.references {
                    // Column names inside sum(table, ...) belong to the table.
                    let in_sum = crate::plans::regions(&plan).any(|region| {
                        region.contains(&doc.text, reference.span)
                            && crate::engine::sum_scope_at(
                                region.source(&doc.text),
                                region.offset_of(&doc.text, reference.span).unwrap_or(0),
                            )
                            .is_some()
                    });
                    if crate::plans::contains(&plan, reference.span, &doc.text)
                        && reference.property.is_none()
                        && !in_sum
                        && !plan.names.iter().any(|n| n.name == reference.name)
                    {
                        plan.names.push(Named {
                            name: reference.name.clone(),
                            span: reference.span,
                        });
                    }
                }
                doc.problems.extend(plan.problems.clone());
                doc.plans.push(plan);
            }
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
                        if let Some((_, span)) = &cell.expression {
                            doc.mark(
                                cell.span.line,
                                cell.span.start,
                                cell.span.start + 1,
                                "operator",
                            );
                            doc.mark(cell.span.line, cell.span.end - 1, cell.span.end, "operator");
                            doc.expression(lines[span.line], span.line, span.start, span.end);
                            continue;
                        }
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
        doc.days = crate::itinerary::parse(&lines);
        for day in &doc.days {
            for stop in &day.stops {
                for detail in &stop.details {
                    if detail.key.eq_ignore_ascii_case("address") && !detail.value.is_empty() {
                        doc.links.push(Link {
                            span: detail.value_span,
                            target: crate::itinerary::map_url(&detail.value),
                        });
                    }
                }
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
        // `total := units * price` needs no brackets: the := says it all.
        if let Some(def) = bare_calculation(line, row, start) {
            let named = def.named.span;
            let source = def.value_span;
            self.mark(row, named.start, named.end, "variable");
            self.mark(
                row,
                source.start.saturating_sub(3),
                source.start,
                "operator",
            );
            self.definitions.push(def);
            self.expression(line, row, source.start, source.end);
            return;
        }
        while i < line.len() {
            if let Some(attr) = attrs.values().find(|a| a.span.start == i) {
                i = attr.span.end;
                continue;
            }
            // `$3,000:budget`, `"Oaxaca City":city`, `https://…/pull/1:pr`: a
            // value followed by :name defines it without brackets.
            if let Some(def) = bare_literal(line, row, i) {
                let value = def.value_span;
                let named = def.named.span;
                self.mark(
                    row,
                    value.start,
                    value.end,
                    if def.source.starts_with('"') || Resource::parse(&def.source).is_some() {
                        "string"
                    } else {
                        "number"
                    },
                );
                self.mark(row, value.end, value.end + 1, "operator");
                self.mark(row, named.start, named.end, "variable");
                if let Some(r) = Resource::parse(&def.source) {
                    self.links.push(Link {
                        span: value,
                        target: r.target,
                    });
                }
                i = named.end;
                self.definitions.push(def);
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
            } else if is_calculation(inner) {
                let span = Span::new(row, inner_start, inner_start + inner.len());
                self.calculations.push(Calculation {
                    span,
                    source: inner.into(),
                    bracketed: true,
                });
                self.mark(row, i, i + 1, "operator");
                self.mark(row, close, close + 1, "operator");
                self.expression(line, row, span.start, span.end);
            }
            i = close + 1;
        }
    }

    fn expression(&mut self, line: &str, row: usize, start: usize, end: usize) {
        // Use the evaluator's lexer, so identifiers and dates have identical boundaries.
        match crate::engine::lex_with_comments(&line[start..end]) {
            Ok(tokens) => {
                let free_names = crate::engine::expression_names(&line[start..end]);
                for token in tokens {
                    let span =
                        Span::new(row, start, end).relative(&self.text, token.start, token.end);
                    let kind = match &token.kind {
                        crate::engine::Lexeme::Name(name) => {
                            let builtin_call = crate::engine::is_builtin_function(name)
                                && line[start + token.end..end].trim_start().starts_with('(');
                            if free_names
                                .as_ref()
                                .is_none_or(|names| names.contains(&token.start))
                                && !builtin_call
                                && !crate::engine::is_code(name)
                                && (token.start == 0
                                    || !line[start..start + token.start].trim_end().ends_with('.'))
                                && !matches!(name.as_str(), "true" | "false" | "null" | "fn")
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
                        crate::engine::Lexeme::Comment => "comment",
                        _ => "operator",
                    };
                    for part in span.fragments(&self.text) {
                        self.mark(part.line, part.start, part.end, kind);
                    }
                }
            }
            Err(_) => {
                for part in Span::new(row, start, end).fragments(&self.text) {
                    self.mark(part.line, part.start, part.end, "string");
                }
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

/// Brackets hold a calculation when the text is a valid expression that reads
/// a name or calls a function. Bare literals such as `[$25]` stay prose, so
/// prices in a sentence are not annotated.
fn is_calculation(inner: &str) -> bool {
    if inner.is_empty() || !crate::engine::Engine::valid_expression(inner) {
        return false;
    }
    let Ok(tokens) = crate::engine::lex(inner) else {
        return false;
    };
    tokens.iter().any(|t| {
        matches!(&t.kind, crate::engine::Lexeme::Name(n)
            if !matches!(n.as_str(), "true" | "false") && !crate::engine::is_code(n))
    })
}
/// The expression for a line of math such as `[budget] - [spent] * 2` or
/// `2 + 2`: brackets around names become spaces, so offsets line up with the
/// line. Prose, list items, lone values, and bare names are not lines of math.
fn line_calculation(trimmed: &str) -> Option<String> {
    let trimmed = trimmed.trim_end();
    if trimmed.is_empty()
        || trimmed.starts_with(['-', '*', '+', '>', '#', '|', '`', '<', '!'])
        || trimmed.contains("](")
    {
        return None;
    }
    let mut masked = String::with_capacity(trimmed.len());
    let mut names: Vec<(usize, usize)> = Vec::new();
    let mut rest = trimmed;
    let mut at = 0;
    while let Some(open) = rest.find('[') {
        let close = rest[open..].find(']')? + open;
        let inner = rest[open + 1..close].trim();
        let (name, property) = inner
            .split_once('.')
            .map(|(n, p)| (n, Some(p)))
            .unwrap_or((inner, None));
        if !identifier(name) || !property.is_none_or(identifier) {
            return None;
        }
        masked.push_str(&rest[..open]);
        masked.push(' ');
        masked.push_str(&rest[open + 1..close]);
        masked.push(' ');
        names.push((at + open + 1, at + close));
        at += close + 1;
        rest = &rest[close + 1..];
    }
    masked.push_str(rest);
    let tokens = crate::engine::lex(&masked).ok()?;
    let mut meaningful = false;
    for (i, token) in tokens.iter().enumerate() {
        match &token.kind {
            crate::engine::Lexeme::Name(n) => {
                let call = matches!(
                    tokens.get(i + 1).map(|t| &t.kind),
                    Some(crate::engine::Lexeme::Left)
                );
                if call {
                    meaningful = true;
                } else if !names
                    .iter()
                    .any(|(s, e)| token.start >= *s && token.end <= *e)
                    && !crate::engine::is_code(n)
                    && !matches!(n.as_str(), "true" | "false")
                {
                    // A bare word is prose, not a variable.
                    return None;
                }
            }
            crate::engine::Lexeme::Op(_) => meaningful = true,
            _ => {}
        }
    }
    (meaningful && crate::engine::Engine::valid_expression(&masked)).then_some(masked)
}
/// A whitespace-delimited word at `at` of the form `value:name`, where the
/// value is a scalar literal, a quoted string, or a resource. `\:` escapes
/// the colon. `10:30am` is a time, and `note:budget` is prose.
fn bare_literal(line: &str, row: usize, at: usize) -> Option<Definition> {
    if at > 0 && !line.as_bytes()[at - 1].is_ascii_whitespace() && line.as_bytes()[at - 1] != b'(' {
        return None;
    }
    let rest = &line[at..];
    let first = rest.chars().next()?;
    if first.is_whitespace() || matches!(first, '[' | '`' | '<' | '\\') {
        return None;
    }
    let value_end = if first == '"' {
        let mut escaped = false;
        let mut close = None;
        for (i, c) in rest.char_indices().skip(1) {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                close = Some(i + 1);
                break;
            }
        }
        close?
    } else {
        // The colon before the name is the last one in the word; earlier ones
        // belong to URLs, geo: coordinates, and times.
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..word_end];
        let mut cut = None;
        let mut search = word.len();
        while let Some(colon) = word[..search].rfind(':') {
            let name = name_len(&word[colon + 1..]);
            if name > 0 && identifier(&word[colon + 1..colon + 1 + name]) {
                cut = Some(colon);
                break;
            }
            search = colon;
        }
        cut?
    };
    let colon = value_end;
    if rest.as_bytes().get(colon) != Some(&b':')
        || colon == 0
        || rest.as_bytes()[colon - 1] == b'\\'
    {
        return None;
    }
    let name_end = colon + 1 + name_len(&rest[colon + 1..]);
    let name = &rest[colon + 1..name_end];
    if !identifier(name) || matches!(name, "true" | "false") {
        return None;
    }
    let value = &rest[..colon];
    let scalar = matches!(
        crate::engine::literal(value),
        Ok(v) if !matches!(v, crate::engine::Value::Text(_)) || value.starts_with('"')
    );
    let resource = Resource::parse(value).is_some();
    if !(scalar || resource) {
        return None;
    }
    // `10:30am`, `3:1pm`: a number before am/pm is a time, not a value.
    if name.eq_ignore_ascii_case("am") || name.eq_ignore_ascii_case("pm") {
        return None;
    }
    Some(Definition {
        named: Named {
            name: name.into(),
            span: Span::new(row, at + colon + 1, at + name_end),
        },
        source: value.into(),
        expression: false,
        value_span: Span::new(row, at, at + colon),
        end: Span::new(row, at + name_end, at + name_end),
    })
}
/// Continue an expression inside delimiters or after an unfinished operator.
/// An outdented declaration/prose line remains a separate document item, even
/// when the preceding expression is missing its closing delimiter.
fn expression_end(lines: &[&str], row: usize, start: usize) -> usize {
    use crate::engine::Lexeme;
    let indent = lines[row].len() - lines[row].trim_start().len();
    let mut depth = 0i32;
    let mut last = row;
    for (index, line) in lines.iter().enumerate().skip(row) {
        let source = if index == row {
            &line[start..]
        } else {
            line.trim_start()
        };
        if index > row {
            let padding = line.len() - line.trim_start().len();
            if !source.is_empty() && padding <= indent && !source.starts_with([')', ']', '}']) {
                break;
            }
            if bare_calculation(line, index, padding).is_some()
                || source
                    .strip_prefix('[')
                    .and_then(|s| s.split_once(']'))
                    .is_some_and(|(_, tail)| tail.trim_start().starts_with(":="))
            {
                break;
            }
        }
        let Ok(tokens) = crate::engine::lex(source) else {
            break;
        };
        for token in &tokens {
            match token.kind {
                Lexeme::Left | Lexeme::OpenList | Lexeme::OpenRecord => depth += 1,
                Lexeme::Right | Lexeme::CloseList | Lexeme::CloseRecord => depth -= 1,
                _ => (),
            }
        }
        if !source.is_empty() {
            last = index;
        }
        let unfinished = tokens.last().is_none_or(|t| {
            matches!(
                t.kind,
                Lexeme::Op(_) | Lexeme::Comma | Lexeme::Dot | Lexeme::Colon
            )
        });
        if depth <= 0 && !unfinished {
            break;
        }
    }
    last
}

/// `name := expression` at the start of a line, without brackets.
fn bare_calculation(line: &str, row: usize, start: usize) -> Option<Definition> {
    let rest = &line[start..];
    let len = name_len(rest);
    let name = &rest[..len];
    if len == 0 || !identifier(name) || matches!(name, "true" | "false") {
        return None;
    }
    let after = &rest[len..];
    let gap = after.len() - after.trim_start().len();
    if !after[gap..].starts_with(":=") {
        return None;
    }
    let expr_start = start + len + gap + 2;
    Some(Definition {
        named: Named {
            name: name.into(),
            span: Span::new(row, start, start + len),
        },
        source: line[expr_start..].trim().into(),
        expression: true,
        value_span: Span::new(row, expr_start, line.len()),
        end: Span::new(row, line.len(), line.len()),
    })
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
