//! The language's inline forms, read from prose and list items into the
//! `Tree`: definitions (`name := expr`, continued over the lines an
//! unfinished expression runs on), named values (`value:name`), bracket
//! forms (`[name]`, `[a / b]`, `[name] := …`, `[value]:name`,
//! `[text](target)`), `@key(value)` attributes as spans, raw links, code
//! spans and comments, and the names and highlighting of an expression.
use crate::blocks::{Heading, Line};
use crate::tree::{HighlightKind, Tree};
use common::{Lines, Resource, Span};
use std::collections::BTreeMap;

pub use syntax::identifier;

#[derive(Clone, Debug)]
pub struct Named {
    pub name: String,
    pub span: Span,
}
impl Named {
    pub(crate) fn new(name: &str, row: usize, start: usize) -> Self {
        Self {
            name: name.into(),
            span: Span::new(row, start, start + name.len()),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Definition {
    pub named: Named,
    pub source: String,
    pub expression: bool,
    pub value_span: Span,
    pub end: Span,
}
impl Definition {
    /// `name := expression`, the expression running from `start` to the end
    /// of `line`.
    fn calculated(named: Named, line: &str, start: usize) -> Self {
        let (row, end) = (named.span.line, line.len());
        Self {
            named,
            source: line[start..].trim().into(),
            expression: true,
            value_span: Span::new(row, start, end),
            end: Span::new(row, end, end),
        }
    }
    /// `value:name`, which ends where its name does.
    fn literal(named: Named, source: &str, value_span: Span) -> Self {
        let end = Span::new(named.span.line, named.span.end, named.span.end);
        Self {
            named,
            source: source.into(),
            expression: false,
            value_span,
            end,
        }
    }
    /// The value span without the blanks it starts with: where the
    /// expression text begins after its `=` or `:=`.
    pub fn expression_span(&self, text: &(impl Lines + ?Sized)) -> Span {
        let raw = self.value_span.source(text);
        Span::new(
            self.value_span.line,
            self.value_span.start + raw.len() - raw.trim_start().len(),
            self.value_span.end,
        )
    }
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
    /// The name and any property, without brackets.
    pub fn full_span(&self) -> Span {
        Span::new(self.span.line, self.span.start, self.end())
    }
}
/// `@key(value)`: the key is the map key it is filed under.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub value: String,
    pub span: Span,
    pub value_span: Span,
}
#[derive(Clone, Debug)]
pub struct Link {
    pub span: Span,
    pub target: String,
}
impl Link {
    pub(crate) fn new(span: Span, target: impl Into<String>) -> Self {
        let target = target.into();
        Self { span, target }
    }
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

/// The `@key(value)` forms on a line, as written: what a key means is a
/// recognizer's business.
#[derive(Default)]
pub(crate) struct Attributes<'a> {
    /// Every attribute in order, a repeated key included.
    pub(crate) list: Vec<(&'a str, Attribute)>,
    /// By key, the last of a repeated key winning.
    pub(crate) map: BTreeMap<String, Attribute>,
    /// An `@key(` that never closes, which ends the scan.
    pub(crate) unclosed: Option<Span>,
}

impl Tree {
    /// A heading's own marks and the bare links written in its title.
    pub(crate) fn heading(&mut self, line: &str, heading: &Heading<'_>) {
        let (row, start, end) = (heading.row, heading.start, heading.title_end);
        self.heading = Some(row);
        self.mark(row, start, end, HighlightKind::Heading);
        if let Some(n) = &heading.named {
            self.paint(n.span, HighlightKind::Variable);
        }
        self.raw_links(line, row, start + heading.level, end);
    }

    /// The last definition, when it is an expression whose name is on `row`:
    /// the one a block under that row may belong to.
    pub(crate) fn opened(&self, row: usize) -> Option<usize> {
        let index = self.definitions.len().checked_sub(1)?;
        let definition = &self.definitions[index];
        (definition.expression && definition.named.span.line == row).then_some(index)
    }

    /// A `name :=` definition whose expression runs past the end of its line,
    /// inside delimiters or after a dangling operator. Returns how many rows
    /// below it the expression took.
    pub(crate) fn continuation(&mut self, text: &str, lines: &[&str], row: usize) -> Option<usize> {
        let index = self.opened(row)?;
        let span = self.definitions[index].value_span;
        let end_row = expression_end(lines, row, span.start);
        if end_row <= row {
            return None;
        }
        let prefix = self.lines.start(row);
        let length = self.lines.start(end_row) - prefix;
        let end = length + lines[end_row].len();
        let block = &text[prefix..prefix + end];
        let before = |at: Span| at.line != row || at.start < span.start;
        self.references.retain(|r| before(r.span));
        self.members.retain(|m| before(m.span));
        self.highlights
            .retain(|h| h.span.line != row || h.span.end <= span.start);
        self.definitions[index].source = block[span.start..].trim().into();
        self.definitions[index].value_span.end = end;
        self.definitions[index].end =
            Span::new(end_row, lines[end_row].len(), lines[end_row].len());
        self.expression(block, row, span.start, end);
        Some(end_row - row)
    }

    fn raw_link(&mut self, line: &str, row: usize, start: usize) -> Option<usize> {
        let end = raw_link_end(line, start)?;
        self.links
            .push(Link::new(Span::new(row, start, end), &line[start..end]));
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

    /// The `@key(value)` forms from `start` on, outside code spans and links.
    pub(crate) fn attributes<'a>(
        &mut self,
        line: &'a str,
        row: usize,
        start: usize,
    ) -> Attributes<'a> {
        let mut attrs = Attributes::default();
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
                attrs.unclosed = Some(Span::new(row, i, line.len()));
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
            attrs.map.insert(key.into(), attr.clone());
            attrs.list.push((key, attr));
            i = end;
        }
        attrs
    }

    /// The inline forms of a line: bare and bracketed definitions, references,
    /// in-place calculations, code spans, comments and links.
    pub(crate) fn prose(&mut self, item: &Line<'_>) {
        let Line {
            text: line,
            row,
            body: start,
            ..
        } = *item;
        let attrs = &item.attributes.map;
        let mut i = start;
        // `total := units * price` needs no brackets: the := says it all.
        if let Some(def) = bare_calculation(line, row, start) {
            let Span { start, end, .. } = def.value_span;
            self.paint(def.named.span, HighlightKind::Variable);
            self.mark(row, start.saturating_sub(3), start, HighlightKind::Operator);
            self.definitions.push(def);
            self.expression(line, row, start, end);
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
                let (value, named) = (def.value_span, def.named.span);
                let resource = Resource::parse(&def.source);
                let text = def.source.starts_with('"') || resource.is_some();
                self.paint(value, HighlightKind::literal(text));
                self.mark(row, value.end, value.end + 1, HighlightKind::Operator);
                self.paint(named, HighlightKind::Variable);
                if let Some(r) = resource {
                    self.links.push(Link::new(value, r.target));
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
            let inner_span = Span::new(row, inner_start, inner_start + inner.len());
            if line.as_bytes().get(close + 1) == Some(&b'(')
                && let Some(end) = line[close + 2..].find(')').map(|o| close + 2 + o)
            {
                self.links
                    .push(Link::new(Span::new(row, i, end + 1), &line[close + 2..end]));
                self.mark(row, i, end + 1, HighlightKind::String);
                i = end + 1;
                continue;
            }
            let tail = &line[close + 1..];
            let after = close + 1 + tail.len() - tail.trim_start().len();
            if line[after..].starts_with(":=") && identifier(inner) {
                let expr_start = after + 2;
                let named = Named::new(inner, row, inner_start);
                self.definitions
                    .push(Definition::calculated(named, line, expr_start));
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
                    let named = Named::new(name, row, close + 2);
                    self.definitions
                        .push(Definition::literal(named, inner, inner_span));
                    let number = inner.starts_with(|c: char| c == '$' || c.is_ascii_digit());
                    self.mark(row, i, close + 1, HighlightKind::literal(!number));
                    self.mark(row, close + 1, close + 2, HighlightKind::Operator);
                    self.mark(row, close + 2, close + 2 + len, HighlightKind::Variable);
                    i = close + 2 + len;
                    continue;
                }
            }
            if let Some((name, property)) = bracket_reference(inner) {
                if property.is_some() {
                    let (_, members) = crate::imports::analyze(inner, &self.text, inner_span);
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
                self.calculations.push(Calculation {
                    span: inner_span,
                    source: inner.into(),
                    bracketed: true,
                });
                self.mark(row, i, i + 1, HighlightKind::Operator);
                self.mark(row, close, close + 1, HighlightKind::Operator);
                self.expression(line, row, inner_span.start, inner_span.end);
            }
            i = close + 1;
        }
    }

    pub(crate) fn expression(&mut self, line: &str, row: usize, start: usize, end: usize) {
        let (source, whole) = (&line[start..end], Span::new(row, start, end));
        let (imports, members) = crate::imports::analyze(source, &self.text, whole);
        self.imports.extend(imports);
        self.members.extend(members);
        // Use the parser's lexer, so identifiers and dates have identical boundaries.
        let Ok(tokens) = syntax::lex_with_comments(source) else {
            for part in whole.fragments(self) {
                self.mark(part.line, part.start, part.end, HighlightKind::String);
            }
            return;
        };
        let free_names = syntax::expression_names(source);
        for token in tokens {
            let span = whole.relative(self, token.start, token.end);
            let kind = match &token.kind {
                syntax::Lexeme::Name(name) => {
                    let builtin_call = (syntax::is_builtin_function(name)
                        || self.forms.contains(name))
                        && source[token.end..].trim_start().starts_with('(');
                    if free_names
                        .as_ref()
                        .is_none_or(|names| names.contains(&token.start))
                        && !builtin_call
                        && !common::is_code(name)
                        && !source[..token.start].trim_end().ends_with('.')
                        && !matches!(name.as_str(), "true" | "false" | "null" | "fn")
                        && (!matches!(name.as_str(), "tomorrow" | "today")
                            || syntax::sum_scope_at(source, token.start).is_some())
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
            for part in span.fragments(self) {
                self.mark(part.line, part.start, part.end, kind);
            }
        }
    }
}

/// `name` or `name.property`, each an identifier: what a bracket can read.
pub(crate) fn bracket_reference(inner: &str) -> Option<(&str, Option<&str>)> {
    let (name, property) = match inner.split_once('.') {
        Some((name, property)) => (name, Some(property)),
        None => (inner, None),
    };
    (identifier(name) && property.is_none_or(identifier)).then_some((name, property))
}
/// The length of the run of identifier bytes `s` starts with.
pub(crate) fn name_len(s: &str) -> usize {
    s.bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
        .count()
}
/// Brackets hold a calculation when the text is a valid expression that reads
/// a name or calls a function. Bare literals such as `[$25]` stay prose, so
/// prices in a sentence are not annotated.
fn is_calculation(inner: &str) -> bool {
    !inner.is_empty()
        && syntax::valid_expression(inner)
        && syntax::lex(inner).is_ok_and(|tokens| {
            tokens.iter().any(|t| {
                matches!(&t.kind, syntax::Lexeme::Name(n)
                    if !matches!(n.as_str(), "true" | "false") && !common::is_code(n))
            })
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
    let named = Named::new(name, row, at + colon + 1);
    let span = Span::new(row, at, at + colon);
    Some(Definition::literal(named, value, span))
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
        // `open := tasks` then an indented `| filter(…)` is one pipeline.
        let piped = lines.get(index + 1).is_some_and(|next| {
            let stage = next.trim_start();
            next.len() - stage.len() > indent && stage.starts_with('|') && !stage.starts_with("||")
        });
        if depth <= 0 && !unfinished && !piped {
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
    let named = Named::new(name, row, start);
    Some(Definition::calculated(named, line, start + len + gap + 2))
}
fn skip_code(line: &str, start: usize) -> usize {
    let count = line[start..].bytes().take_while(|c| *c == b'`').count();
    let marker = "`".repeat(count);
    line[start + count..]
        .find(&marker)
        .map(|n| start + count + n + count)
        .unwrap_or(line.len())
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
            .url(
                std::path::Path::new(&format!("/workspace/{}", common::note_file("note"))),
                None,
            )
            .is_err()
    {
        return None;
    }
    Some(start + candidate.len())
}
