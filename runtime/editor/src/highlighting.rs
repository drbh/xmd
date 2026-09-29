//! Semantic colors describe XMD's syntax and types, independent of the editor.
//! Keep parser highlights intact: other LSP features use them to identify inert text.
use crate::prose;
use lang::common::Span;
use lang::eval::engine;
use lang::model::{Attribute, Document, HighlightKind, Named};
use lang::syntax::{Lexeme, Literal};
use lsp_types::SemanticToken;

/// A semantic token type. Declaration order is the legend's order, so a
/// token's discriminant is its index there.
#[derive(Clone, Copy, PartialEq, Eq, strum::VariantNames)]
#[strum(serialize_all = "camelCase")]
pub(crate) enum Token {
    Comment,
    Keyword,
    Number,
    Variable,
    Operator,
    String,
    Heading,
    Function,
    Property,
    Decorator,
    XmdMoney,
    XmdDate,
    XmdDuration,
    XmdRatio,
    XmdBoolean,
    XmdPunctuation,
    XmdCode,
    XmdLink,
    XmdCheckbox,
    XmdTaskDone,
    XmdCheckboxChecked,
    XmdTime,
    // Itineraries: one hue per stop kind, so a day reads at a glance.
    XmdDay,
    XmdPlace,
    XmdDetailKey,
    XmdDepart,
    XmdArrive,
    XmdTransit,
    XmdStay,
    XmdMeal,
    XmdVisit,
    XmdExplore,
}
pub const TOKEN_TYPES: &[&str] = <Token as strum::VariantNames>::VARIANTS;
/// The semantic token type for a stop kind.
fn kind_token(kind: &lang::eval::itinerary::Kind) -> Token {
    match kind.marker {
        '>' => Token::XmdDepart,
        '<' => Token::XmdArrive,
        '~' => Token::XmdTransit,
        '@' => Token::XmdStay,
        '*' => Token::XmdMeal,
        '+' => Token::XmdVisit,
        _ => Token::XmdExplore,
    }
}
pub const TOKEN_MODIFIERS: &[&str] = &["declaration", "defaultLibrary"];
const DECLARATION: u32 = 1;
const DEFAULT_LIBRARY: u32 = 2;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    /// `None` is unstyled prose.
    kind: Option<Token>,
    modifiers: u32,
}
fn style(kind: Token, modifiers: u32) -> Style {
    Style {
        kind: Some(kind),
        modifiers,
    }
}
fn value_kind(value: &Literal) -> Token {
    match value {
        Literal::Money(..) => Token::XmdMoney,
        Literal::Date(_) | Literal::DateTime(_) => Token::XmdDate,
        Literal::Duration(_) => Token::XmdDuration,
        Literal::Ratio(_) => Token::XmdRatio,
        Literal::Bool(_) => Token::XmdBoolean,
        Literal::Text(_) => Token::String,
        Literal::Resource(_) => Token::XmdLink,
        Literal::Number(_) => Token::Number,
    }
}
struct Painter<'a> {
    doc: &'a Document,
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
            doc,
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
    fn mark(&mut self, span: Span, kind: Token) {
        self.paint(span, style(kind, 0));
    }
    fn declaration(&mut self, named: &Named) {
        self.paint(named.span, style(Token::Variable, DECLARATION));
        let colon = Span::new(
            named.span.line,
            named.span.start.saturating_sub(1),
            named.span.start,
        );
        if self.source(colon) == ":" {
            self.mark(colon, Token::XmdPunctuation);
        }
    }
    /// Dim the brackets around a span, when there are any: bare definitions
    /// such as `$3,000:budget` have none.
    fn brackets(&mut self, span: Span) {
        let line = self.lines[span.line];
        if line[..span.start].ends_with('[') {
            self.mark(
                Span::new(span.line, span.start - 1, span.start),
                Token::XmdPunctuation,
            );
        }
        if line[span.end..].starts_with(']') {
            self.mark(
                Span::new(span.line, span.end, span.end + 1),
                Token::XmdPunctuation,
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
                        if lang::eval::engine::is_builtin_function(name) {
                            modifiers = DEFAULT_LIBRARY;
                        }
                        Token::Function
                    } else if matches!(name.as_str(), "true" | "false") {
                        Token::XmdBoolean
                    } else if i > 0 && matches!(tokens[i - 1].kind, Lexeme::Dot)
                        || engine::sum_scope_at(source, token.start).is_some()
                    {
                        Token::Property
                    } else {
                        Token::Variable
                    }
                }
                Lexeme::Comment => Token::Comment,
                Lexeme::Op(_) => Token::Operator,
                _ => Token::XmdPunctuation,
            };
            self.paint(
                span.relative(self.text, token.start, token.end),
                style(kind, modifiers),
            );
        }
    }
    fn attribute(&mut self, name: &str, attr: &Attribute) {
        self.mark(attr.span, Token::XmdPunctuation);
        self.mark(
            Span::new(attr.span.line, attr.span.start, attr.value_span.start - 1),
            Token::Decorator,
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
            self.mark(attr.value_span, Token::XmdDate);
        } else if matches!(name, "tag" | "every") {
            self.mark(attr.value_span, Token::String);
        } else {
            self.expression(attr.value_span);
        }
    }
    /// A pipe grid's cell separators and its `---` rule, when it has one.
    /// Painted before cells: quoted and escaped pipes inside cells are data.
    fn grid(&mut self, header: usize, end_line: usize, ruled: bool) {
        for row in header..end_line {
            for (byte, ch) in self.lines[row].char_indices() {
                if ch == '|' {
                    self.mark(Span::new(row, byte, byte + 1), Token::XmdPunctuation);
                }
            }
        }
        if ruled {
            let row = header + 1;
            self.mark(
                Span::new(row, 0, self.lines[row].len()),
                Token::XmdPunctuation,
            );
        }
    }

    fn highlights(&mut self) {
        for h in &self.doc.highlights {
            let kind = match h.kind {
                HighlightKind::String => Token::XmdCode,
                HighlightKind::Comment => Token::Comment,
                HighlightKind::Heading => Token::Heading,
                HighlightKind::Variable => Token::Variable,
                HighlightKind::Keyword => Token::Keyword,
                HighlightKind::Number => Token::Number,
                HighlightKind::Operator => Token::Operator,
            };
            self.mark(h.span, kind);
        }
    }
    fn prose_values(&mut self) {
        let heading = style(Token::Heading, 0);
        for row in 0..self.lines.len() {
            for (start, end, kind) in prose::values(self.lines[row]) {
                // Never recolor links, code, comments, names, formulae, or cells.
                if self.colors[row][start..end]
                    .iter()
                    .all(|s| s.kind.is_none() || *s == heading)
                {
                    self.mark(Span::new(row, start, end), kind);
                }
            }
        }
    }
    fn sections(&mut self) {
        for section in &self.doc.sections {
            let line = self.lines[section.line];
            let start = line.len() - line.trim_start().len();
            self.mark(
                Span::new(section.line, start, start + section.level),
                Token::XmdPunctuation,
            );
            if let Some(named) = &section.named {
                self.declaration(named);
            }
        }
    }
    fn tasks(&mut self) {
        for task in &self.doc.tasks {
            self.mark(
                Span::new(task.line, task.indent, task.indent + 1),
                Token::XmdPunctuation,
            );
            self.mark(
                task.checkbox,
                if task.checked {
                    Token::XmdCheckboxChecked
                } else {
                    Token::XmdCheckbox
                },
            );
            if task.checked {
                let end = task
                    .attributes
                    .values()
                    .map(|a| a.span.start)
                    .chain(task.named.iter().map(|n| n.span.start - 1))
                    .min()
                    .unwrap_or(self.lines[task.line].len());
                self.mark(
                    Span::new(task.line, task.checkbox.end, end),
                    Token::XmdTaskDone,
                );
            }
            if let Some(named) = &task.named {
                self.declaration(named);
            }
        }
    }
    fn definitions(&mut self) {
        for def in &self.doc.definitions {
            self.declaration(&def.named);
            if !def.expression {
                self.brackets(def.value_span);
                self.mark(
                    def.value_span,
                    lang::syntax::literal(&def.source)
                        .as_ref()
                        .map(value_kind)
                        .unwrap_or(Token::String),
                );
                continue;
            }
            self.brackets(def.named.span);
            if def.source == "table" {
                self.mark(def.value_span, Token::Keyword);
            } else if let Some((_, start, end)) = lang::eval::plans::goal(&def.source) {
                let offset = def.expression_span(self.text).start;
                let line = def.value_span.line;
                self.mark(Span::new(line, offset, offset + start), Token::Keyword);
                self.expression(Span::new(line, offset + start, offset + end));
                self.mark(
                    Span::new(line, offset + end, def.value_span.end),
                    Token::Keyword,
                );
            } else {
                self.expression(def.value_span);
            }
        }
    }
    fn attributes(&mut self) {
        let doc = self.doc;
        for (name, attr) in doc
            .tasks
            .iter()
            .flat_map(|t| &t.attributes)
            .chain(doc.events.iter().flat_map(|e| &e.attributes))
        {
            self.attribute(name, attr);
        }
    }
    fn calculations(&mut self) {
        for calculation in &self.doc.calculations {
            self.brackets(calculation.span);
            self.expression(calculation.span);
        }
    }
    fn references(&mut self) {
        for reference in self.doc.references.iter().filter(|r| r.bracket) {
            let (line, end) = (reference.span.line, reference.span.end);
            self.brackets(Span::new(line, reference.span.start, reference.end()));
            self.mark(reference.span, Token::Variable);
            if reference.property.is_some() {
                self.mark(Span::new(line, end, end + 1), Token::XmdPunctuation);
                self.mark(Span::new(line, end + 1, reference.end()), Token::Property);
            }
        }
    }
    fn plans(&mut self) {
        for plan in &self.doc.plans {
            self.grid(plan.header, plan.end_line, !plan.separators.is_empty());
            for column in &plan.columns {
                self.mark(column.span, Token::Keyword);
            }
            for constraint in &plan.constraints {
                self.paint(constraint.named.span, style(Token::Property, DECLARATION));
                self.expression(constraint.span);
            }
        }
    }
    fn tables(&mut self) {
        for table in &self.doc.tables {
            self.grid(table.header, table.end_line, !table.separators.is_empty());
            for column in &table.columns {
                self.paint(column.span, style(Token::Property, DECLARATION));
            }
            for cell in table.rows.iter().flatten() {
                if let Some((_, inner)) = &cell.expression {
                    self.brackets(*inner);
                    self.expression(*inner);
                } else {
                    self.mark(
                        cell.span,
                        cell.value.as_ref().map(value_kind).unwrap_or(Token::String),
                    );
                }
            }
        }
    }
    fn days(&mut self) {
        for day in &self.doc.days {
            if let Some((_, span)) = &day.weekday {
                self.paint(*span, style(Token::XmdDay, DECLARATION));
                self.paint(
                    Span::new(span.line, span.end, day.date_span.start),
                    style(Token::XmdDay, 0),
                );
            }
            self.paint(day.date_span, style(Token::XmdDay, DECLARATION));
            if let Some((_, span)) = &day.places {
                self.mark(*span, Token::XmdPlace);
            }
            for stop in &day.stops {
                self.mark(stop.time_span, Token::XmdTime);
                let token = stop.kind.map(kind_token).unwrap_or(Token::Heading);
                if let Some(marker) = stop.marker_span {
                    self.paint(marker, style(token, DECLARATION));
                }
                self.mark(stop.title_span, token);
                for detail in &stop.details {
                    self.mark(detail.key_span, Token::XmdDetailKey);
                    let key = detail.key.to_ascii_lowercase();
                    if key.ends_with("number")
                        || key == "seats"
                        || key == "confirmation"
                        || key == "pnr"
                    {
                        self.mark(detail.value_span, Token::XmdCode);
                    } else if key == "cancel by" {
                        self.mark(detail.value_span, Token::XmdDate);
                    }
                    self.mark(
                        Span::new(
                            detail.key_span.line,
                            detail.key_span.end,
                            detail.value_span.start,
                        ),
                        Token::XmdPunctuation,
                    );
                }
            }
        }
    }
    fn links(&mut self) {
        for link in &self.doc.links {
            self.mark(link.span, Token::XmdLink);
            let raw = self.source(link.span);
            if raw.starts_with('[')
                && let Some(close) = raw.find("](")
            {
                for (start, end) in [(0, 1), (close, close + 2), (raw.len() - 1, raw.len())] {
                    self.mark(
                        Span::new(
                            link.span.line,
                            link.span.start + start,
                            link.span.start + end,
                        ),
                        Token::XmdPunctuation,
                    );
                }
            }
        }
    }
    /// Metadata-looking text inside comments must never become active highlighting.
    fn comments(&mut self) {
        for h in self
            .doc
            .highlights
            .iter()
            .filter(|h| h.kind == HighlightKind::Comment)
        {
            self.mark(h.span, Token::Comment);
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
                    if let Some(kind) = active.kind {
                        let delta_line = row as u32 - previous_line;
                        result.push(SemanticToken {
                            delta_line,
                            delta_start: if delta_line == 0 {
                                start - previous_start
                            } else {
                                start
                            },
                            length: character - start,
                            token_type: kind as u32,
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

/// Parser highlights first, then prose values where nothing else claimed the
/// text, then each structure in turn; later paints win.
pub fn semantic_tokens(doc: &Document) -> Vec<SemanticToken> {
    let mut p = Painter::new(doc);
    p.highlights();
    p.prose_values();
    p.sections();
    p.tasks();
    p.definitions();
    p.attributes();
    p.calculations();
    p.references();
    p.plans();
    p.tables();
    p.days();
    p.links();
    p.comments();
    p.finish()
}
