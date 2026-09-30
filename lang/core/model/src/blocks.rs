//! The generic layer of a parse: what a note's lines are as document
//! structure, and the language's inline forms inside them. A line is a fence,
//! a comment, a heading (level, title, optional `:name`), a list item
//! (indentation, optional checkbox), a table row, prose or blank; inside prose
//! and list items it reads definitions (`name := expr`, continued over the
//! lines an unfinished expression runs on), named values (`value:name`),
//! bracket forms (`[name]`, `[a / b]`, `[name] := …`, `[value]:name`, `[text](target)`),
//! `@key(value)` attributes as spans, raw links, code spans and comments.
//!
//! Nothing here knows what a task, event, section, table, plan or itinerary
//! is: the recognizers (listed in `recognizers`) read these blocks and fill
//! the note's features. Everything read here lands in a [`Tree`], the shared
//! output the recognizers add to.
use common::{LineIndex, Lines, Resource, Span};
use std::collections::{BTreeMap, BTreeSet};

pub use syntax::identifier;

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
impl Definition {
    /// The value span without the blanks it starts with: where the
    /// expression text begins after its `=` or `:=`.
    pub fn expression_span(&self, text: &str) -> Span {
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

/// `## Title :name`: its indentation, level, title and trailing `:name`.
pub(crate) struct Heading<'a> {
    pub(crate) row: usize,
    pub(crate) start: usize,
    pub(crate) level: usize,
    pub(crate) title: &'a str,
    pub(crate) title_end: usize,
    pub(crate) named: Option<Named>,
}
/// A list item's `[ ]`, `[-]` or `[x]`: where its `[` is and the mark inside.
#[derive(Clone, Copy)]
pub(crate) struct Checkbox {
    pub(crate) at: usize,
    pub(crate) mark: u8,
}
/// What a line is, decided before anything is read from it.
pub(crate) enum Block<'a> {
    /// A fence delimiter, or a line inside an open fence.
    Fence {
        start: usize,
    },
    /// An HTML comment line (opening, inside or closing) or a `//` line.
    Comment {
        start: usize,
    },
    Heading(Heading<'a>),
    /// `- text`, `* text`, `+ text`, or `- [ ] text` with a checkbox.
    Item {
        start: usize,
        checkbox: Option<Checkbox>,
    },
    /// `| a | b |`
    Row {
        start: usize,
    },
    /// Whitespace only: it says nothing about the note.
    Blank,
    /// Everything else.
    Prose {
        start: usize,
    },
}

/// All a line needs to know about the lines before it. A block a recognizer
/// reads over several rows (a table, a plan, a continued expression) is not
/// held here: whoever read those rows reports how many it took, and they
/// never reach the classifier.
#[derive(Default)]
pub(crate) struct BlockState {
    /// The fence marker and the length of its run, while a fence is open.
    fence: Option<(char, usize)>,
    /// Whether an HTML comment is still open.
    comment: bool,
}

/// Decide what a line is and carry the fence or comment it leaves open.
pub(crate) fn classify<'a>(line: &'a str, row: usize, state: &mut BlockState) -> Block<'a> {
    let start = line.len() - line.trim_start().len();
    let trimmed = &line[start..];
    let marker = trimmed.chars().next().unwrap_or(' ');
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    if let Some((kind, count)) = state.fence {
        let closes = marker == kind && run >= count && trimmed[run..].trim().is_empty();
        state.fence = (!closes).then_some((kind, count));
        return Block::Fence { start };
    }
    if (marker == '`' || marker == '~') && run >= 3 {
        state.fence = Some((marker, run));
        return Block::Fence { start };
    }
    if state.comment || trimmed.starts_with("<!--") {
        state.comment = !trimmed.contains("-->");
        return Block::Comment { start };
    }
    if trimmed.starts_with("//") {
        return Block::Comment { start };
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
        return Block::Heading(Heading {
            row,
            start,
            level: run,
            title: line[start + run..title_end].trim(),
            title_end,
            named,
        });
    }
    if trimmed.is_empty() {
        return Block::Blank;
    }
    if marker == '|' {
        return Block::Row { start };
    }
    let bytes = line.as_bytes();
    let item = matches!(marker, '-' | '*' | '+')
        && bytes.get(start + 1).is_none_or(u8::is_ascii_whitespace);
    if !item {
        return Block::Prose { start };
    }
    let at = start + 2;
    let checkbox = (bytes[start + 1..].starts_with(b" [")
        && matches!(bytes.get(at + 1), Some(b' ' | b'-' | b'x' | b'X'))
        && bytes.get(at + 2) == Some(&b']')
        && bytes.get(at + 3).is_none_or(u8::is_ascii_whitespace))
    .then(|| Checkbox {
        at,
        mark: bytes[at + 1],
    });
    Block::Item { start, checkbox }
}

/// A line of prose, a list item or a table row, with its `@key(value)`
/// attributes already found: what recognizers of such lines are handed.
pub(crate) struct Line<'a> {
    pub(crate) text: &'a str,
    pub(crate) row: usize,
    /// The indentation.
    pub(crate) start: usize,
    pub(crate) checkbox: Option<Checkbox>,
    /// Where the inline forms begin: past a checkbox, otherwise at the
    /// indentation (a plain list marker is read as prose).
    pub(crate) body: usize,
    pub(crate) attributes: Attributes<'a>,
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

/// What the generic layer has read so far, and what recognizers add to: the
/// language's inline forms and the highlighting and problems of every block.
/// It becomes the matching fields of the `Document`.
#[derive(Default)]
pub(crate) struct Tree {
    pub(crate) text: String,
    pub(crate) lines: LineIndex,
    /// The row of the last heading read.
    pub(crate) heading: Option<usize>,
    pub(crate) definitions: Vec<Definition>,
    pub(crate) references: Vec<Reference>,
    pub(crate) imports: BTreeSet<String>,
    pub(crate) members: Vec<crate::imports::Member>,
    pub(crate) links: Vec<Link>,
    pub(crate) calculations: Vec<Calculation>,
    pub(crate) highlights: Vec<Highlight>,
    pub(crate) problems: Vec<Problem>,
}
impl Lines for Tree {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_start(&self, line: usize) -> usize {
        self.lines.start(line)
    }
}

impl Tree {
    pub(crate) fn new(text: String) -> Self {
        Self {
            lines: LineIndex::new(&text),
            text,
            ..Self::default()
        }
    }

    pub(crate) fn mark(&mut self, line: usize, start: usize, end: usize, kind: HighlightKind) {
        if end > start {
            self.highlights.push(Highlight {
                span: Span::new(line, start, end),
                kind,
            });
        }
    }

    /// Put the highlights in reading order, one per span.
    pub(crate) fn finish(&mut self) {
        self.highlights
            .sort_by_key(|h| (h.span.line, h.span.start, h.span.end));
        self.highlights.dedup_by_key(|h| h.span);
    }

    /// A heading's own marks and the bare links written in its title.
    pub(crate) fn heading(&mut self, line: &str, heading: &Heading<'_>) {
        let row = heading.row;
        self.heading = Some(row);
        self.mark(
            row,
            heading.start,
            heading.title_end,
            HighlightKind::Heading,
        );
        if let Some(n) = &heading.named {
            self.mark(row, n.span.start, n.span.end, HighlightKind::Variable);
        }
        self.raw_links(line, row, heading.start + heading.level, heading.title_end);
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

    pub(crate) fn expression(&mut self, line: &str, row: usize, start: usize, end: usize) {
        let (imports, members) =
            crate::imports::analyze(&line[start..end], &self.text, Span::new(row, start, end));
        self.imports.extend(imports);
        self.members.extend(members);
        // Use the parser's lexer, so identifiers and dates have identical boundaries.
        match syntax::lex_with_comments(&line[start..end]) {
            Ok(tokens) => {
                let free_names = syntax::expression_names(&line[start..end]);
                for token in tokens {
                    let span = Span::new(row, start, end).relative(self, token.start, token.end);
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
                    for part in span.fragments(self) {
                        self.mark(part.line, part.start, part.end, kind);
                    }
                }
            }
            Err(_) => {
                for part in Span::new(row, start, end).fragments(self) {
                    self.mark(part.line, part.start, part.end, HighlightKind::String);
                }
            }
        }
    }
}

fn name_len(s: &str) -> usize {
    s.bytes()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
        .count()
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
pub(crate) fn skip_code(line: &str, start: usize) -> usize {
    let count = line[start..].bytes().take_while(|c| *c == b'`').count();
    let marker = "`".repeat(count);
    line[start + count..]
        .find(&marker)
        .map(|n| start + count + n + count)
        .unwrap_or(line.len())
}
pub(crate) fn trailing_name(line: &str, row: usize) -> Option<Named> {
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
/// Pipes in quoted strings and escaped pipes are cell contents, not separators.
pub fn cells(line: &str, row: usize) -> Option<Vec<(String, Span)>> {
    let start = line.len() - line.trim_start().len();
    let end = line.trim_end().len();
    if !line[start..].starts_with('|') || end <= start + 1 {
        return None;
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut last = start + 1;
    let mut result = Vec::new();
    for (i, c) in line.char_indices().filter(|(i, _)| *i > start && *i < end) {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if c == '|' && !quoted {
            let raw = &line[last..i];
            let from = last + raw.len() - raw.trim_start().len();
            let to = from + raw.trim().len();
            result.push((raw.trim().into(), Span::new(row, from, to)));
            last = i + 1;
        }
    }
    (last == end && !quoted).then_some(result)
}
