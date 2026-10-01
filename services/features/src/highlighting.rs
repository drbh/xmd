//! Semantic colors describe XMD's syntax and types, independent of the editor.
//! Keep parser highlights intact: other LSP features use them to identify inert text.
use crate::prose;
use lang::common::Span;
use lang::document::recognized::Paint;
use lang::document::{Attribute, Document, HighlightKind, Named};
use lang::syntax::{AttributeValue, Lexeme, Literal};
use lsp_types::SemanticToken;

/// A semantic token type. Declaration order is the legend's order, so a
/// token's discriminant is its index there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::VariantNames, strum::EnumString)]
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
    /// A control's state, as a tri-state checkbox shows it: off, on and
    /// mixed. A host may make a span painted so clickable.
    XmdToggle,
    /// The text of something finished, struck through.
    XmdFinished,
    XmdToggleOn,
    XmdToggleMixed,
    XmdTime,
    /// The key of a `Key: value` line.
    XmdKey,
    // A categorical palette: a module that needs distinguishable hues picks
    // categories through its recognizers' paints, and the theme colors them.
    XmdCategory1,
    XmdCategory2,
    XmdCategory3,
    XmdCategory4,
    XmdCategory5,
    XmdCategory6,
    XmdCategory7,
    XmdCategory8,
    XmdCategory9,
    XmdCategory10,
}
pub const TOKEN_TYPES: &[&str] = <Token as strum::VariantNames>::VARIANTS;
/// The token a recognizer's paint is drawn with: one the legend already has.
/// The match is exhaustive, so every paint has its token.
fn paint_token(paint: Paint) -> Token {
    macro_rules! tokens {
        ($($same:ident)*; $($paint:ident => $token:ident,)*) => {
            match paint {
                $(Paint::$same => Token::$same,)*
                $(Paint::$paint => Token::$token,)*
            }
        };
    }
    tokens!(
        Keyword Number String Variable Heading Function Property Decorator Operator Comment;
        Punctuation => XmdPunctuation, Money => XmdMoney, Date => XmdDate, Time => XmdTime,
        Duration => XmdDuration, Boolean => XmdBoolean, Link => XmdLink, Code => XmdCode,
        Key => XmdKey, Toggle => XmdToggle, ToggleOn => XmdToggleOn,
        ToggleMixed => XmdToggleMixed, Finished => XmdFinished,
        Category1 => XmdCategory1, Category2 => XmdCategory2, Category3 => XmdCategory3,
        Category4 => XmdCategory4, Category5 => XmdCategory5, Category6 => XmdCategory6,
        Category7 => XmdCategory7, Category8 => XmdCategory8, Category9 => XmdCategory9,
        Category10 => XmdCategory10,
    )
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
    /// The prelude functions the note calls by name, which paint like
    /// built-ins.
    library: &'a [String],
    text: &'a str,
    lines: Vec<&'a str>,
    colors: Vec<Vec<Style>>,
}
impl<'a> Painter<'a> {
    fn new(doc: &'a Document, library: &'a [String]) -> Self {
        let lines: Vec<_> = doc.text().lines().collect();
        let colors = lines
            .iter()
            .map(|l| vec![Style::default(); l.len()])
            .collect();
        Self {
            doc,
            library,
            text: doc.text(),
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
        let Ok(tokens) = lang::syntax::lex_with_comments(source) else {
            return;
        };
        for (i, token) in tokens.iter().enumerate() {
            let mut modifiers = 0;
            let kind = match &token.kind {
                Lexeme::Value(value) => value_kind(value),
                Lexeme::Name(name) => {
                    if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left)) {
                        if lang::syntax::is_builtin_function(name) || self.library.contains(name) {
                            modifiers = DEFAULT_LIBRARY;
                        }
                        Token::Function
                    } else if matches!(name.as_str(), "true" | "false") {
                        Token::XmdBoolean
                    } else if i > 0 && matches!(tokens[i - 1].kind, Lexeme::Dot)
                        || lang::syntax::sum_scope_at(source, token.start).is_some()
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
        // What the value holds decides its paint, as in the parser: a date
        // that reads as one without evaluating, an expression, or plain text.
        match self.doc.attribute_value(name) {
            Some(AttributeValue::When) if lang::syntax::is_relative_date(&attr.value) => {
                self.mark(attr.value_span, Token::XmdDate);
            }
            Some(AttributeValue::Date) if lang::syntax::stamp(&attr.value).is_some() => {
                self.mark(attr.value_span, Token::XmdDate);
            }
            Some(value) if value.is_expression() => self.expression(attr.value_span),
            _ => self.mark(attr.value_span, Token::String),
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
        for h in self.doc.highlights() {
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
    /// The groups a module's recognizers paint, over prose values and under
    /// the note's own structure.
    fn recognized(&mut self) {
        for group in self.doc.recognized().iter().flat_map(|m| &m.groups) {
            if let Some(paint) = group.paint {
                let modifiers = if group.declaration { DECLARATION } else { 0 };
                self.paint(group.span, style(paint_token(paint), modifiers));
            }
        }
    }
    fn sections(&mut self) {
        for section in self.doc.sections() {
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
    /// A checklist item's name declares it; how its checkbox and title look
    /// is what a module's recognizers paint.
    fn checklist(&mut self) {
        for named in self.doc.tasks().iter().filter_map(|t| t.named.as_ref()) {
            self.declaration(named);
        }
    }
    fn definitions(&mut self) {
        for (index, def) in self.doc.definitions().iter().enumerate() {
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
            } else if let Some((start, end)) = self
                .doc
                .form_of(index)
                .filter(|formed| formed.has_table())
                .and_then(|_| inside(&def.source))
            {
                // A form that takes a table paints its call as a keyword, as
                // `table` does, around the expressions it is handed.
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
        for (name, attr) in doc.claimed_attributes() {
            self.attribute(name, attr);
        }
    }
    fn calculations(&mut self) {
        for calculation in self.doc.calculations() {
            self.brackets(calculation.span);
            self.expression(calculation.span);
        }
    }
    fn references(&mut self) {
        for reference in self.doc.references().iter().filter(|r| r.bracket) {
            let (line, end) = (reference.span.line, reference.span.end);
            self.brackets(Span::new(line, reference.span.start, reference.end()));
            self.mark(reference.span, Token::Variable);
            if reference.property.is_some() {
                self.mark(Span::new(line, end, end + 1), Token::XmdPunctuation);
                self.mark(Span::new(line, end + 1, reference.end()), Token::Property);
            }
        }
    }
    fn forms(&mut self) {
        for formed in self.doc.forms().iter().filter(|f| f.has_table()) {
            self.grid(
                formed.header,
                formed.end_line,
                !formed.separators.is_empty(),
            );
            for column in &formed.columns {
                self.mark(column.span, Token::Keyword);
            }
            for cells in &formed.rows {
                for ((_, span), column) in cells.iter().zip(&formed.form.table) {
                    if column.reads.is_expression() {
                        self.expression(*span);
                    } else {
                        self.paint(*span, style(Token::Property, DECLARATION));
                    }
                }
            }
        }
    }
    fn tables(&mut self) {
        for table in self.doc.tables() {
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
    fn links(&mut self) {
        for link in self.doc.links() {
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
            .highlights()
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
/// text, then each structure in turn; later paints win. `library` names the
/// prelude functions the note reaches (`Workspace::prelude_names`), which
/// paint as built-ins do.
pub fn semantic_tokens(doc: &Document, library: &[String]) -> Vec<SemanticToken> {
    let mut p = Painter::new(doc, library);
    p.highlights();
    p.prose_values();
    p.recognized();
    p.sections();
    p.checklist();
    p.definitions();
    p.attributes();
    p.calculations();
    p.references();
    p.forms();
    p.tables();
    p.links();
    p.comments();
    p.finish()
}

/// The byte range inside a call's parentheses, `name(` to `)`, in `source`.
fn inside(source: &str) -> Option<(usize, usize)> {
    let (name, _) = lang::eval::forms::call(source)?;
    let open = name.len() + source[name.len()..].find('(')?;
    Some((open + 1, source.trim_end().len() - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind of stop the itinerary module declares has a category of
    /// its own, on its marker and its title alike, so a kind added there
    /// needs a category too.
    #[test]
    fn every_stop_kind_has_its_own_category() {
        let mut doc = Document::parse(String::new());
        doc.recognize(&lang::eval::Workspace::new(vec![]).modules().recognizers());
        let stop = doc
            .rules()
            .iter()
            .find(|r| r.module == "itinerary" && r.name == "stop")
            .expect("the itinerary's stop rule");
        // Each kind once, as the terms of its markers name it.
        let mut kinds: Vec<&str> = Vec::new();
        for term in stop.terms("marker") {
            if !kinds.contains(&&*term.term) {
                kinds.push(&term.term);
            }
        }
        assert!(kinds.len() > 1);
        for group in ["marker", "title"] {
            let (_, brush) = stop.tokens.iter().find(|(g, _)| g == group).unwrap();
            let tokens: Vec<Token> = kinds
                .iter()
                .map(|kind| brush.term_paint(kind).map_or(Token::Heading, paint_token))
                .collect();
            for (kind, token) in kinds.iter().zip(&tokens) {
                let name = TOKEN_TYPES[*token as usize];
                assert!(name.starts_with("xmdCategory"), "{kind} has no category");
            }
            for (i, token) in tokens.iter().enumerate() {
                assert!(!tokens[..i].contains(token), "{token:?} is shared");
            }
        }
    }
}
