//! Semantic colors describe WTF's syntax and types, independent of the editor.
//! Keep parser highlights intact: other LSP features use them to identify inert text.
use crate::view::prose;
use common::Span;
use eval::engine;
use lsp_types::SemanticToken;
use model::{Attribute, Document, Named};
use syntax::{Lexeme, Literal};

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
    "wtfMoney",
    "wtfDate",
    "wtfDuration",
    "wtfRatio",
    "wtfBoolean",
    "wtfPunctuation",
    "wtfCode",
    "wtfLink",
    "wtfCheckbox",
    "wtfTaskDone",
    "wtfCheckboxChecked",
    "wtfTime",
    // Itineraries: one hue per stop kind, so a day reads at a glance.
    "wtfDay",
    "wtfPlace",
    "wtfDetailKey",
    "wtfDepart",
    "wtfArrive",
    "wtfTransit",
    "wtfStay",
    "wtfMeal",
    "wtfVisit",
    "wtfExplore",
];
/// The semantic token type for a stop kind.
pub(crate) fn kind_token(kind: &eval::itinerary::Kind) -> &'static str {
    match kind.marker {
        '>' => "wtfDepart",
        '<' => "wtfArrive",
        '~' => "wtfTransit",
        '@' => "wtfStay",
        '*' => "wtfMeal",
        '+' => "wtfVisit",
        _ => "wtfExplore",
    }
}
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
fn value_kind(value: &Literal) -> &'static str {
    match value {
        Literal::Money(..) => "wtfMoney",
        Literal::Date(_) | Literal::DateTime(_) => "wtfDate",
        Literal::Duration(_) => "wtfDuration",
        Literal::Ratio(_) => "wtfRatio",
        Literal::Bool(_) => "wtfBoolean",
        Literal::Text(_) => "string",
        Literal::Resource(_) => "wtfLink",
        Literal::Number(_) => "number",
    }
}

struct Painter<'a> {
    text: &'a str,
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
        Self {
            text: &doc.text,
            lines,
            colors,
        }
    }
    fn source(&self, span: Span) -> &'a str {
        span.source(self.text)
    }
    fn paint(&mut self, span: Span, color: Style) {
        for span in span.fragments(self.text) {
            if let Some(line) = self.colors.get_mut(span.line)
                && let Some(bytes) = line.get_mut(span.start..span.end)
            {
                bytes.fill(color);
            }
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
            self.mark(colon, "wtfPunctuation");
        }
    }
    /// Dim the brackets around a span, when there are any: bare definitions
    /// such as `$3,000:budget` have none.
    fn brackets(&mut self, span: Span) {
        let line = self.lines[span.line];
        if line[..span.start].ends_with('[') {
            self.mark(
                Span::new(span.line, span.start - 1, span.start),
                "wtfPunctuation",
            );
        }
        if line[span.end..].starts_with(']') {
            self.mark(
                Span::new(span.line, span.end, span.end + 1),
                "wtfPunctuation",
            );
        }
    }
    fn expression(&mut self, span: Span) {
        let source = self.source(span);
        // An unfinished expression should not suddenly become a solid string color.
        self.paint(span, Style::default());
        let Ok(tokens) = engine::lex_with_comments(source) else {
            return;
        };
        for (i, token) in tokens.iter().enumerate() {
            let mut modifiers = 0;
            let kind = match &token.kind {
                Lexeme::Value(value) => value_kind(value),
                Lexeme::Name(name) => {
                    if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left)) {
                        if crate::language::signature::is_builtin_function(name) {
                            modifiers = DEFAULT_LIBRARY;
                        }
                        "function"
                    } else if matches!(name.as_str(), "true" | "false") {
                        "wtfBoolean"
                    } else if i > 0 && matches!(tokens[i - 1].kind, Lexeme::Dot)
                        || engine::sum_scope_at(source, token.start).is_some()
                    {
                        "property"
                    } else {
                        "variable"
                    }
                }
                Lexeme::Comment => "comment",
                Lexeme::Op(_) => "operator",
                _ => "wtfPunctuation",
            };
            self.paint(
                span.relative(self.text, token.start, token.end),
                style(kind, modifiers),
            );
        }
    }
    fn attribute(&mut self, name: &str, attr: &Attribute) {
        self.mark(attr.span, "wtfPunctuation");
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
            self.mark(attr.value_span, "wtfDate");
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
            if h.kind == model::HighlightKind::String {
                "wtfCode"
            } else {
                h.kind.as_str()
            },
        );
    }
    p.prose_values();
    for section in &doc.sections {
        let line = p.lines[section.line];
        let start = line.len() - line.trim_start().len();
        p.mark(
            Span::new(section.line, start, start + section.level),
            "wtfPunctuation",
        );
        if let Some(named) = &section.named {
            p.declaration(named);
        }
    }
    for task in &doc.tasks {
        p.mark(
            Span::new(task.line, task.indent, task.indent + 1),
            "wtfPunctuation",
        );
        p.mark(
            task.checkbox,
            if task.checked {
                "wtfCheckboxChecked"
            } else {
                "wtfCheckbox"
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
            p.mark(Span::new(task.line, task.checkbox.end, end), "wtfTaskDone");
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
            } else if let Some((_, start, end)) = eval::plans::goal(&def.source) {
                let raw = p.source(def.value_span);
                let offset = def.value_span.start + raw.len() - raw.trim_start().len();
                p.mark(
                    Span::new(def.value_span.line, offset, offset + start),
                    "keyword",
                );
                p.expression(Span::new(def.value_span.line, offset + start, offset + end));
                p.mark(
                    Span::new(def.value_span.line, offset + end, def.value_span.end),
                    "keyword",
                );
            } else {
                p.expression(def.value_span);
            }
        } else {
            p.brackets(def.value_span);
            p.mark(
                def.value_span,
                syntax::literal(&def.source)
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
    for calculation in &doc.calculations {
        p.brackets(calculation.span);
        p.expression(calculation.span);
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
                "wtfPunctuation",
            );
            p.mark(
                Span::new(reference.span.line, reference.span.end + 1, reference.end()),
                "property",
            );
        }
    }
    for plan in &doc.plans {
        for row in plan.header..plan.end_line {
            for (byte, ch) in p.lines[row].char_indices() {
                if ch == '|' {
                    p.mark(Span::new(row, byte, byte + 1), "wtfPunctuation");
                }
            }
        }
        if !plan.separators.is_empty() {
            let row = plan.header + 1;
            p.mark(Span::new(row, 0, p.lines[row].len()), "wtfPunctuation");
        }
        for column in &plan.columns {
            p.mark(column.span, "keyword");
        }
        for constraint in &plan.constraints {
            p.paint(constraint.named.span, style("property", DECLARATION));
            p.expression(constraint.span);
        }
    }
    for table in &doc.tables {
        // Paint separators first: quoted and escaped pipes inside cells are data.
        for row in table.header..table.end_line {
            for (byte, ch) in p.lines[row].char_indices() {
                if ch == '|' {
                    p.mark(Span::new(row, byte, byte + 1), "wtfPunctuation");
                }
            }
        }
        if !table.separators.is_empty() {
            let row = table.header + 1;
            p.mark(Span::new(row, 0, p.lines[row].len()), "wtfPunctuation");
        }
        for column in &table.columns {
            p.paint(column.span, style("property", DECLARATION));
        }
        for cell in table.rows.iter().flatten() {
            if let Some((_, inner)) = &cell.expression {
                p.brackets(*inner);
                p.expression(*inner);
            } else {
                p.mark(
                    cell.span,
                    cell.value.as_ref().map(value_kind).unwrap_or("string"),
                );
            }
        }
    }
    for day in &doc.days {
        if let Some((_, span)) = &day.weekday {
            p.paint(*span, style("wtfDay", DECLARATION));
            p.paint(
                Span::new(span.line, span.end, day.date_span.start),
                style("wtfDay", 0),
            );
        }
        p.paint(day.date_span, style("wtfDay", DECLARATION));
        if let Some((_, span)) = &day.places {
            p.mark(*span, "wtfPlace");
        }
        for stop in &day.stops {
            p.mark(stop.time_span, "wtfTime");
            let token = stop.kind.map(kind_token).unwrap_or("heading");
            if let Some(marker) = stop.marker_span {
                p.paint(marker, style(token, DECLARATION));
            }
            p.mark(stop.title_span, token);
            for detail in &stop.details {
                p.mark(detail.key_span, "wtfDetailKey");
                let key = detail.key.to_ascii_lowercase();
                if key.ends_with("number")
                    || key == "seats"
                    || key == "confirmation"
                    || key == "pnr"
                {
                    p.mark(detail.value_span, "wtfCode");
                } else if key == "cancel by" {
                    p.mark(detail.value_span, "wtfDate");
                }
                p.mark(
                    Span::new(
                        detail.key_span.line,
                        detail.key_span.end,
                        detail.value_span.start,
                    ),
                    "wtfPunctuation",
                );
            }
        }
    }
    for link in &doc.links {
        p.mark(link.span, "wtfLink");
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
                    "wtfPunctuation",
                );
            }
        }
    }
    // Metadata-looking text inside comments must never become active highlighting.
    for h in doc
        .highlights
        .iter()
        .filter(|h| h.kind == model::HighlightKind::Comment)
    {
        p.mark(h.span, "comment");
    }
    p.finish()
}
