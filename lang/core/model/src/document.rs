use common::{Resource, Span};
use lsp_types::Position;
use std::collections::BTreeMap;

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
pub use syntax::identifier;
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
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Document {
    pub text: String,
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    pub imports: std::collections::BTreeSet<String>,
    pub members: Vec<crate::imports::Member>,
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

/// What a heading line carries: its indentation, level, title and trailing `:name`.
struct Heading<'a> {
    start: usize,
    level: usize,
    title: &'a str,
    title_end: usize,
    named: Option<Named>,
}
/// What a line is, decided before the document is touched: every branch the
/// parse loop used to take at the top of an iteration.
enum Line<'a> {
    /// A fence delimiter, or a line inside an open fence; `open` is the fence
    /// still open after this line.
    Fence {
        start: usize,
        open: Option<(char, usize)>,
    },
    /// An HTML comment line (opening, inside or closing) or a `//` line;
    /// `open` is whether the comment runs on past this line.
    Comment { start: usize, open: bool },
    /// `## Title :name`
    Heading(Heading<'a>),
    /// `- [ ] title`, with `checkbox` at the `[`.
    Task { start: usize, checkbox: usize },
    /// Whitespace only: it says nothing about the note.
    Blank,
    /// Everything else: prose, plain list items, table rows, event lines.
    Prose { start: usize },
}

/// All a line needs to know about the lines before it. A table, plan or
/// multiline expression in progress is not held here: the builder that read
/// those rows reports how many it took, and they never reach the classifier.
#[derive(Default)]
struct BlockState {
    /// The fence marker and the length of its run, while a fence is open.
    fence: Option<(char, usize)>,
    /// Whether an HTML comment is still open.
    comment: bool,
}

/// Decide what a line is, without looking at the document being built.
fn classify<'a>(line: &'a str, row: usize, state: &BlockState) -> Line<'a> {
    let start = line.len() - line.trim_start().len();
    let trimmed = &line[start..];
    let marker = trimmed.chars().next().unwrap_or(' ');
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    if let Some((kind, count)) = state.fence {
        let closes = marker == kind && run >= count && trimmed[run..].trim().is_empty();
        return Line::Fence {
            start,
            open: (!closes).then_some((kind, count)),
        };
    }
    if (marker == '`' || marker == '~') && run >= 3 {
        return Line::Fence {
            start,
            open: Some((marker, run)),
        };
    }
    if state.comment || trimmed.starts_with("<!--") {
        return Line::Comment {
            start,
            open: !trimmed.contains("-->"),
        };
    }
    if trimmed.starts_with("//") {
        return Line::Comment { start, open: false };
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
        return Line::Heading(Heading {
            start,
            level: run,
            title: line[start + run..title_end].trim(),
            title_end,
            named,
        });
    }
    if trimmed.is_empty() {
        return Line::Blank;
    }
    let checkbox = ["- [", "* [", "+ ["]
        .iter()
        .find(|prefix| trimmed.starts_with(**prefix))
        .map(|_| start + 2)
        .filter(|s| {
            matches!(line.as_bytes().get(s + 1), Some(b' ' | b'x' | b'X'))
                && line.as_bytes().get(s + 2) == Some(&b']')
                && line
                    .as_bytes()
                    .get(s + 3)
                    .is_none_or(u8::is_ascii_whitespace)
        });
    match checkbox {
        Some(checkbox) => Line::Task { start, checkbox },
        None => Line::Prose { start },
    }
}

impl Document {
    pub fn parse(text: String) -> Self {
        let mut doc = Self {
            text: text.clone(),
            ..Self::default()
        };
        let lines: Vec<_> = text.lines().collect();
        let mut state = BlockState::default();
        let mut parents: Vec<usize> = Vec::new();
        let mut row = 0;
        while row < lines.len() {
            let line = lines[row];
            let mut consumed = 0;
            match classify(line, row, &state) {
                Line::Fence { start, open } => {
                    state.fence = open;
                    doc.mark(row, start, line.len(), HighlightKind::String);
                }
                Line::Comment { start, open } => {
                    state.comment = open;
                    doc.mark(row, start, line.len(), HighlightKind::Comment);
                }
                Line::Blank => {}
                Line::Heading(heading) => {
                    parents.clear();
                    doc.heading(line, row, heading);
                }
                Line::Task { start, checkbox } => {
                    let attrs = doc.attributes(line, row, checkbox + 3);
                    doc.task(line, row, start, checkbox, &attrs, &mut parents);
                    doc.prose(line, row, checkbox + 3, &attrs);
                    consumed = doc.blocks(&text, &lines, row);
                }
                Line::Prose { start } => {
                    let attrs = doc.attributes(line, row, start);
                    if attrs.contains_key("at") {
                        doc.event(line, row, start, &attrs);
                    }
                    doc.prose(line, row, start, &attrs);
                    if attrs.is_empty() {
                        doc.calculation(line, row, start);
                    }
                    consumed = doc.blocks(&text, &lines, row);
                }
            }
            row += 1 + consumed;
        }
        doc.itinerary(&lines);
        doc.finish(lines.len());
        doc
    }

    /// `## Title :name`: opens a section, closes the ones it outranks, and
    /// still shows the bare links written in its title.
    fn heading(&mut self, line: &str, row: usize, heading: Heading<'_>) {
        let Heading {
            start,
            level,
            title,
            title_end,
            named,
        } = heading;
        for section in &mut self.sections {
            if section.end_line == usize::MAX && section.level >= level {
                section.end_line = row;
            }
        }
        self.sections.push(Section {
            line: row,
            end_line: usize::MAX,
            level,
            title: title.into(),
            named: named.clone(),
        });
        self.mark(row, start, title_end, HighlightKind::Heading);
        if let Some(n) = named {
            self.mark(row, n.span.start, n.span.end, HighlightKind::Variable);
        }
        self.raw_links(line, row, start + level, title_end);
    }

    /// `- [ ] title #tag @due(…) :name`: a checkbox item, nested under the
    /// nearest open task indented less than it.
    fn task(
        &mut self,
        line: &str,
        row: usize,
        start: usize,
        checkbox: usize,
        attrs: &BTreeMap<String, Attribute>,
        parents: &mut Vec<usize>,
    ) {
        let named = trailing_name(line, row);
        while parents
            .last()
            .is_some_and(|i| self.tasks[*i].indent >= start)
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
        self.tasks.push(Task {
            line: row,
            indent: start,
            checked: line.as_bytes()[checkbox + 1] != b' ',
            checkbox: Span::new(row, checkbox, checkbox + 3),
            title: line[checkbox + 3..title_end].trim().into(),
            named: named.clone(),
            parent: parents.last().copied(),
            attributes: attrs.clone(),
            tags,
        });
        parents.push(self.tasks.len() - 1);
        self.mark(row, checkbox, checkbox + 3, HighlightKind::Keyword);
        if let Some(n) = named {
            self.mark(row, n.span.start, n.span.end, HighlightKind::Variable);
        }
    }

    /// A line with an `@at(…)` attribute and no checkbox: an event, titled by
    /// the text in front of its first attribute.
    fn event(&mut self, line: &str, row: usize, start: usize, attrs: &BTreeMap<String, Attribute>) {
        let end = attrs
            .values()
            .map(|a| a.span.start)
            .min()
            .unwrap_or(line.len());
        self.events.push(Event {
            line: row,
            title: line[start..end].trim().trim_start_matches("- ").into(),
            attributes: attrs.clone(),
        });
    }

    /// A line that is only math, with its variables in brackets, shows its
    /// result at the end: `[budget] - [spent]`.
    fn calculation(&mut self, line: &str, row: usize, start: usize) {
        let trimmed = &line[start..];
        if let Some(source) = line_calculation(trimmed) {
            self.calculations.push(Calculation {
                span: Span::new(row, start, start + trimmed.trim_end().len()),
                source,
                bracketed: false,
            });
        }
    }

    /// The block a line opens once its inline items are known: a multiline
    /// expression, a plan or a table, each of which may read the lines below
    /// it. Returns how many following rows were consumed.
    fn blocks(&mut self, text: &str, lines: &[&str], row: usize) -> usize {
        let mut consumed = self.definition(text, lines, row).unwrap_or(0);
        if let Some(rows) = self.plan(lines, row) {
            consumed = rows;
        }
        if let Some(rows) = self.table(lines, row) {
            consumed = rows;
        }
        consumed
    }

    /// A `name :=` definition whose expression runs past the end of its line,
    /// inside delimiters or after a dangling operator.
    fn definition(&mut self, text: &str, lines: &[&str], row: usize) -> Option<usize> {
        let index = self.definitions.len().checked_sub(1)?;
        if !self.definitions[index].expression || self.definitions[index].named.span.line != row {
            return None;
        }
        let span = self.definitions[index].value_span;
        let end_row = expression_end(lines, row, span.start);
        if end_row <= row {
            return None;
        }
        let prefix: usize = text.split_inclusive('\n').take(row).map(str::len).sum();
        let length: usize = text[prefix..]
            .split_inclusive('\n')
            .take(end_row - row)
            .map(str::len)
            .sum();
        let end = length + lines[end_row].len();
        let block = &text[prefix..prefix + end];
        self.references
            .retain(|r| r.span.line != row || r.span.start < span.start);
        self.members
            .retain(|m| m.span.line != row || m.span.start < span.start);
        self.highlights
            .retain(|h| h.span.line != row || h.span.end <= span.start);
        self.definitions[index].source = block[span.start..].trim().into();
        self.definitions[index].value_span.end = end;
        self.definitions[index].end =
            Span::new(end_row, lines[end_row].len(), lines[end_row].len());
        self.expression(block, row, span.start, end);
        Some(end_row - row)
    }

    /// A `name :=` definition whose expression states a goal: a plan, over the
    /// columns, constraints and table rows written under it.
    fn plan(&mut self, lines: &[&str], row: usize) -> Option<usize> {
        let index = self.definitions.len().checked_sub(1)?;
        let definition = &self.definitions[index];
        if definition.named.span.line != row
            || !definition.expression
            || crate::plans::goal(&definition.source).is_none()
        {
            return None;
        }
        let mut plan = crate::plans_impl::parse(self, index, lines);
        let end_line = plan.end_line;
        for column in &plan.columns {
            self.mark(
                column.span.line,
                column.span.start,
                column.span.end,
                HighlightKind::Keyword,
            );
        }
        for constraint in &plan.constraints {
            let n = &constraint.named;
            self.mark(
                n.span.line,
                n.span.start,
                n.span.end,
                HighlightKind::Variable,
            );
            self.expression(
                lines[constraint.span.line],
                constraint.span.line,
                constraint.span.start,
                constraint.span.end,
            );
        }
        for reference in &self.references {
            // Column names inside sum(table, ...) belong to the table.
            let in_sum = crate::plans::regions(&plan).any(|region| {
                region.contains(&self.text, reference.span)
                    && syntax::sum_scope_at(
                        region.source(&self.text),
                        region.offset_of(&self.text, reference.span).unwrap_or(0),
                    )
                    .is_some()
            });
            if crate::plans_impl::contains(&plan, reference.span, &self.text)
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
        self.problems.extend(plan.problems.clone());
        self.plans.push(plan);
        Some(end_line.saturating_sub(row + 1))
    }

    /// `name := table` followed by a markdown table: its columns, and every
    /// cell's literal, formula or link.
    fn table(&mut self, lines: &[&str], row: usize) -> Option<usize> {
        let index = self.definitions.len().checked_sub(1)?;
        let definition = &self.definitions[index];
        if definition.named.span.line != row
            || !definition.expression
            || definition.source != "table"
        {
            return None;
        }
        let table = crate::tables_impl::parse(self, index, lines);
        let end_line = table.end_line;
        // The declaration keyword isn't a global reference.
        self.references
            .retain(|r| !(r.span.line == row && r.name == "table"));
        for column in &table.columns {
            self.mark(
                column.span.line,
                column.span.start,
                column.span.end,
                HighlightKind::Variable,
            );
        }
        for cells in &table.rows {
            for cell in cells {
                if let Some((_, span)) = &cell.expression {
                    self.mark(
                        cell.span.line,
                        cell.span.start,
                        cell.span.start + 1,
                        HighlightKind::Operator,
                    );
                    self.mark(
                        cell.span.line,
                        cell.span.end - 1,
                        cell.span.end,
                        HighlightKind::Operator,
                    );
                    self.expression(lines[span.line], span.line, span.start, span.end);
                    continue;
                }
                if let Ok(syntax::Literal::Resource(resource)) = &cell.value {
                    self.links.push(Link {
                        span: cell.span,
                        target: resource.target.clone(),
                    });
                }
                self.mark(
                    cell.span.line,
                    cell.span.start,
                    cell.span.end,
                    if matches!(
                        &cell.value,
                        Ok(syntax::Literal::Text(_) | syntax::Literal::Resource(_))
                    ) {
                        HighlightKind::String
                    } else {
                        HighlightKind::Number
                    },
                );
            }
        }
        self.problems.extend(table.problems.clone());
        self.tables.push(table);
        Some(end_line.saturating_sub(row + 1))
    }

    /// Day and stop blocks, wherever in the note they are, and a map link for
    /// every address they carry.
    fn itinerary(&mut self, lines: &[&str]) {
        self.days = crate::itinerary_impl::parse(lines);
        for day in &self.days {
            for stop in &day.stops {
                for detail in &stop.details {
                    if detail.key.eq_ignore_ascii_case("address") && !detail.value.is_empty() {
                        self.links.push(Link {
                            span: detail.value_span,
                            target: crate::itinerary_impl::map_url(&detail.value),
                        });
                    }
                }
            }
        }
    }

    /// Close the sections still open at the end of the note, and put the
    /// highlights in reading order.
    fn finish(&mut self, lines: usize) {
        for section in &mut self.sections {
            if section.end_line == usize::MAX {
                section.end_line = lines;
            }
        }
        self.highlights
            .sort_by_key(|h| (h.span.line, h.span.start, h.span.end));
        self.highlights.dedup_by_key(|h| h.span);
    }
    fn mark(&mut self, line: usize, start: usize, end: usize, kind: HighlightKind) {
        if end > start {
            self.highlights.push(Highlight {
                span: Span::new(line, start, end),
                kind,
            });
        }
    }

    fn raw_link(&mut self, line: &str, row: usize, start: usize) -> Option<usize> {
        let end = raw_link_end(line, start)?;
        self.links.push(Link {
            span: Span::new(row, start, end),
            target: line[start..end].into(),
        });
        self.mark(row, start, end, HighlightKind::String);
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
            if let Some(end) = raw_link_end(line, i) {
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
            self.mark(row, i, open + 1, HighlightKind::Keyword);
            self.mark(row, end - 1, end, HighlightKind::Operator);
            if matches!(key, "due" | "scheduled" | "at") && syntax::is_relative_date(&attr.value) {
                self.mark(row, open + 1, end - 1, HighlightKind::Number);
            } else if matches!(
                key,
                "due" | "scheduled" | "at" | "estimate" | "after" | "timer"
            ) {
                self.expression(line, row, open + 1, end - 1);
            } else {
                self.mark(row, open + 1, end - 1, HighlightKind::String);
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

    /// The prose of a line: bare and bracketed definitions, references,
    /// in-place calculations, code spans, comments and links.
    fn prose(&mut self, line: &str, row: usize, start: usize, attrs: &BTreeMap<String, Attribute>) {
        let mut i = start;
        // `total := units * price` needs no brackets: the := says it all.
        if let Some(def) = bare_calculation(line, row, start) {
            let named = def.named.span;
            let source = def.value_span;
            self.mark(row, named.start, named.end, HighlightKind::Variable);
            self.mark(
                row,
                source.start.saturating_sub(3),
                source.start,
                HighlightKind::Operator,
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
                        HighlightKind::String
                    } else {
                        HighlightKind::Number
                    },
                );
                self.mark(row, value.end, value.end + 1, HighlightKind::Operator);
                self.mark(row, named.start, named.end, HighlightKind::Variable);
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
                self.mark(row, i, end, HighlightKind::Comment);
                i = end;
                continue;
            }
            if line.as_bytes()[i] == b'`' {
                let end = skip_code(line, i);
                self.mark(row, i, end, HighlightKind::String);
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
            let Some(close) = close_bracket(line, i) else {
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
                self.mark(row, i, end + 1, HighlightKind::String);
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
                self.mark(row, i, close + 1, HighlightKind::Variable);
                self.mark(row, after, after + 2, HighlightKind::Operator);
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
                            HighlightKind::Number
                        } else {
                            HighlightKind::String
                        },
                    );
                    self.mark(row, close + 1, close + 2, HighlightKind::Operator);
                    self.mark(row, close + 2, close + 2 + len, HighlightKind::Variable);
                    i = close + 2 + len;
                    continue;
                }
            }
            let (name, property) = inner
                .split_once('.')
                .map(|(n, p)| (n, Some(p)))
                .unwrap_or((inner, None));
            if identifier(name) && property.is_none_or(identifier) {
                if property.is_some() {
                    let (_, members) = crate::imports::analyze(
                        inner,
                        &self.text,
                        Span::new(row, inner_start, inner_start + inner.len()),
                    );
                    self.members.extend(members);
                }
                self.references.push(Reference {
                    name: name.into(),
                    span: Span::new(row, inner_start, inner_start + name.len()),
                    bracket: true,
                    property: property.map(str::to_string),
                });
                self.mark(row, i, close + 1, HighlightKind::Variable);
            } else if is_calculation(inner) {
                let span = Span::new(row, inner_start, inner_start + inner.len());
                self.calculations.push(Calculation {
                    span,
                    source: inner.into(),
                    bracketed: true,
                });
                self.mark(row, i, i + 1, HighlightKind::Operator);
                self.mark(row, close, close + 1, HighlightKind::Operator);
                self.expression(line, row, span.start, span.end);
            }
            i = close + 1;
        }
    }

    fn expression(&mut self, line: &str, row: usize, start: usize, end: usize) {
        let (imports, members) =
            crate::imports::analyze(&line[start..end], &self.text, Span::new(row, start, end));
        self.imports.extend(imports);
        self.members.extend(members);
        // Use the parser's lexer, so identifiers and dates have identical boundaries.
        match syntax::lex_with_comments(&line[start..end]) {
            Ok(tokens) => {
                let free_names = syntax::expression_names(&line[start..end]);
                for token in tokens {
                    let span =
                        Span::new(row, start, end).relative(&self.text, token.start, token.end);
                    let kind = match &token.kind {
                        syntax::Lexeme::Name(name) => {
                            let builtin_call = syntax::is_builtin_function(name)
                                && line[start + token.end..end].trim_start().starts_with('(');
                            if free_names
                                .as_ref()
                                .is_none_or(|names| names.contains(&token.start))
                                && !builtin_call
                                && !common::is_code(name)
                                && (token.start == 0
                                    || !line[start..start + token.start].trim_end().ends_with('.'))
                                && !matches!(name.as_str(), "true" | "false" | "null" | "fn")
                                && (!matches!(name.as_str(), "tomorrow" | "today")
                                    || syntax::sum_scope_at(&line[start..end], token.start)
                                        .is_some())
                            {
                                self.references.push(Reference {
                                    name: name.clone(),
                                    span,
                                    bracket: false,
                                    property: None,
                                });
                            }
                            HighlightKind::Variable
                        }
                        syntax::Lexeme::Value(_) => HighlightKind::Number,
                        syntax::Lexeme::Comment => HighlightKind::Comment,
                        _ => HighlightKind::Operator,
                    };
                    for part in span.fragments(&self.text) {
                        self.mark(part.line, part.start, part.end, kind);
                    }
                }
            }
            Err(_) => {
                for part in Span::new(row, start, end).fragments(&self.text) {
                    self.mark(part.line, part.start, part.end, HighlightKind::String);
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
    /// The plan this definition solves, when it is one.
    pub fn plan_of(&self, definition: usize) -> Option<&crate::plans::Plan> {
        self.plans.iter().find(|p| p.definition == definition)
    }
    /// The table this definition lays out, when it is one.
    pub fn table_of(&self, definition: usize) -> Option<&crate::tables::Table> {
        self.tables.iter().find(|t| t.definition == definition)
    }
    /// The first and last rows a definition spans: through its table or plan
    /// when it lays one out, otherwise through its own value.
    pub fn definition_rows(&self, definition: usize) -> (usize, usize) {
        let def = &self.definitions[definition];
        let first = def.named.span.line;
        let last = self
            .table_of(definition)
            .map(|t| t.end_line)
            .or_else(|| self.plan_of(definition).map(|p| p.end_line))
            .map_or(def.end.line, |end| end.saturating_sub(1));
        (first, last.max(first))
    }
    /// The leaf tasks under a section heading: what its checklist counts.
    pub fn section_tasks(&self, section: usize) -> impl Iterator<Item = usize> + '_ {
        let section = &self.sections[section];
        self.tasks
            .iter()
            .enumerate()
            .filter(move |(i, t)| {
                t.line > section.line
                    && t.line < section.end_line
                    && !self.tasks.iter().any(|child| child.parent == Some(*i))
            })
            .map(|(i, _)| i)
    }
}

/// Every byte range in a note that holds an expression: named definitions, an
/// in-place calculation, a plan's constraints, and the timing/duration
/// attributes on a task. Shared by rename/refactor scans and by a table's
/// `sum` scope lookup, so both agree on what counts as an expression.
pub fn expression_regions(doc: &Document) -> Vec<Span> {
    doc.definitions
        .iter()
        .filter(|d| d.expression)
        .map(|d| d.value_span)
        .chain(doc.calculations.iter().map(|c| c.span))
        .chain(
            doc.plans
                .iter()
                .flat_map(|p| p.constraints.iter().map(|c| c.span)),
        )
        .chain(doc.tasks.iter().flat_map(|t| {
            t.attributes
                .iter()
                .filter(|(k, _)| {
                    matches!(
                        k.as_str(),
                        "due" | "scheduled" | "at" | "after" | "estimate"
                    )
                })
                .map(|(_, a)| a.value_span)
        }))
        .collect()
}

/// Brackets hold a calculation when the text is a valid expression that reads
/// a name or calls a function. Bare literals such as `[$25]` stay prose, so
/// prices in a sentence are not annotated.
fn is_calculation(inner: &str) -> bool {
    if inner.is_empty() || !syntax::valid_expression(inner) {
        return false;
    }
    let Ok(tokens) = syntax::lex(inner) else {
        return false;
    };
    tokens.iter().any(|t| {
        matches!(&t.kind, syntax::Lexeme::Name(n)
            if !matches!(n.as_str(), "true" | "false") && !common::is_code(n))
    })
}

/// Inline calculations can contain lists and quoted closing brackets, e.g.
/// `[sparkline([1, 2, 3])]` or `[debug({label: "]"})]`.
fn close_bracket(line: &str, open: usize) -> Option<usize> {
    let mut depth = 1;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, byte) in line.as_bytes()[open + 1..].iter().enumerate() {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            quoted = !quoted;
        } else if !quoted {
            match byte {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(open + 1 + offset);
                    }
                }
                _ => (),
            }
        }
    }
    None
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
    let tokens = syntax::lex(&masked).ok()?;
    let mut meaningful = false;
    for (i, token) in tokens.iter().enumerate() {
        match &token.kind {
            syntax::Lexeme::Name(n) => {
                let call = matches!(
                    tokens.get(i + 1).map(|t| &t.kind),
                    Some(syntax::Lexeme::Left)
                );
                if call {
                    meaningful = true;
                } else if !names
                    .iter()
                    .any(|(s, e)| token.start >= *s && token.end <= *e)
                    && !common::is_code(n)
                    && !matches!(n.as_str(), "true" | "false")
                {
                    // A bare word is prose, not a variable.
                    return None;
                }
            }
            syntax::Lexeme::Op(_) => meaningful = true,
            _ => {}
        }
    }
    (meaningful && syntax::valid_expression(&masked)).then_some(masked)
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
        syntax::literal(value),
        Ok(v) if !matches!(v, syntax::Literal::Text(_)) || value.starts_with('"')
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
    use syntax::Lexeme;
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
        let Ok(tokens) = syntax::lex(source) else {
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

/// End byte of a raw resource at a prose boundary. Uses no filesystem/network IO.
fn raw_link_end(line: &str, start: usize) -> Option<usize> {
    if start > 0
        && !line[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || "(<\"'“‘".contains(c))
    {
        return None;
    }
    let rest = &line[start..];
    let end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || "<>\"'`“”‘’".contains(*c))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let mut candidate = &rest[..end];
    // Keep balanced parentheses (common in URLs), but exclude sentence punctuation.
    loop {
        let last = candidate.chars().next_back()?;
        let trim = ".,;:!?".contains(last)
            || match last {
                ')' => candidate.matches(')').count() > candidate.matches('(').count(),
                ']' => candidate.matches(']').count() > candidate.matches('[').count(),
                '}' => candidate.matches('}').count() > candidate.matches('{').count(),
                _ => false,
            };
        if !trim {
            break;
        }
        candidate = &candidate[..candidate.len() - last.len_utf8()];
    }
    if matches!(candidate, "/" | "./" | "../" | "~/") {
        return None;
    }
    let resource = common::Resource::parse(candidate)?;
    // A browser has no home directory, but can still recognize and highlight ~/.
    if !candidate.starts_with("~/")
        && resource
            .url(std::path::Path::new(&format!(
                "/workspace/{}",
                common::note_file("note")
            )))
            .is_err()
    {
        return None;
    }
    Some(start + candidate.len())
}
