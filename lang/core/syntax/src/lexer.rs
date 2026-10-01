//! Syntax: the lexer, the expression tree, and the tolerant source scans the
//! editor features read without evaluating anything.
use super::operators::{PIPE_PRECEDENCE, UNARY_PRECEDENCE};
use super::{BinaryOp, Builtin, Literal, Operator, UnaryOp, date_value, literal};
use common::{Code, Currency, is_code};
use std::collections::BTreeSet;
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq)]
pub enum Lexeme {
    Comment,
    Value(Literal),
    Name(String),
    Op(Operator),
    Left,
    Right,
    Comma,
    Dot,
    OpenList,
    CloseList,
    OpenRecord,
    CloseRecord,
    Colon,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Lexeme,
    pub start: usize,
    pub end: usize,
}
pub fn lex(s: &str) -> Result<Vec<Token>, String> {
    Ok(lex_with_comments(s)?
        .into_iter()
        .filter(|t| !matches!(t.kind, Lexeme::Comment))
        .collect())
}
pub fn lex_with_comments(s: &str) -> Result<Vec<Token>, String> {
    let bytes = s.as_bytes();
    // The end of the run from `i` on whose bytes `keep` accepts.
    fn run(bytes: &[u8], i: usize, keep: impl Fn(u8) -> bool) -> usize {
        i + bytes[i..].iter().take_while(|b| keep(**b)).count()
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let start = i;
        let kind =
            if s[i..].starts_with("//") {
                i += s[i..].find('\n').unwrap_or(s.len() - i);
                Lexeme::Comment
            } else if c == '"' {
                i += 1;
                let mut escaped = false;
                let mut closed = false;
                while i < s.len() {
                    let ch = bytes[i];
                    i += 1;
                    if ch == b'"' && !escaped {
                        closed = true;
                        break;
                    }
                    escaped = ch == b'\\' && !escaped;
                }
                if !closed {
                    return Err("Unclosed string".into());
                }
                Lexeme::Value(literal(&s[start..i])?)
            } else if c.is_ascii_digit() && s.get(i..i + 10).and_then(date_value).is_some() {
                i += 10;
                if bytes.get(i) == Some(&b'T') {
                    i = run(bytes, i, |b| {
                        !b.is_ascii_whitespace() && !matches!(b, b')' | b',' | b']' | b'}')
                    });
                }
                Lexeme::Value(date_value(&s[start..i]).ok_or(
                    "Invalid date/time (use an explicit offset for ambiguous local times)",
                )?)
            } else if c.is_ascii_digit()
                || Currency::from_symbol(c).is_some()
                || c == '.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                i += c.len_utf8();
                while i < s.len()
                    && (bytes[i].is_ascii_digit()
                        || bytes[i] == b'.'
                        || bytes[i] == b',' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
                {
                    i += 1;
                }
                if bytes
                    .get(i)
                    .is_some_and(|c| matches!(c, b's' | b'm' | b'h' | b'd' | b'w' | b'%'))
                {
                    i += 1;
                }
                // `700 MXN`: a currency code right after a plain number.
                let rest = &s[i..];
                let gap = rest.len() - rest.trim_start().len();
                let code = rest
                    .get(gap..gap + 3)
                    .filter(|code| code.bytes().all(|b| b.is_ascii_uppercase()));
                if gap > 0
                    && code.is_some_and(|code| Currency::parse(code).is_some())
                    && rest[gap + 3..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric())
                    && !s[start..i].ends_with(['s', 'm', 'h', 'd', 'w', '%'])
                {
                    i += gap + 3;
                }
                let v = literal(&s[start..i])?;
                if matches!(v, Literal::Text(_)) {
                    return Err(format!("Invalid number '{}'", &s[start..i]));
                }
                Lexeme::Value(v)
            } else if c.is_ascii_alphabetic() || c == '_' {
                i = run(bytes, i + 1, |b| b.is_ascii_alphanumeric() || b == b'_');
                Lexeme::Name(s[start..i].into())
            } else {
                i += 1;
                match c {
                    '(' => Lexeme::Left,
                    ')' => Lexeme::Right,
                    ',' => Lexeme::Comma,
                    '.' => Lexeme::Dot,
                    '[' => Lexeme::OpenList,
                    ']' => Lexeme::CloseList,
                    '{' => Lexeme::OpenRecord,
                    '}' => Lexeme::CloseRecord,
                    ':' => Lexeme::Colon,
                    '+' | '-' | '*' | '/' | '!' | '=' | '<' | '>' | '&' | '|' => {
                        if bytes.get(i).is_some_and(|b| {
                            *b == b'='
                                || c == '=' && *b == b'>'
                                || *b == c as u8 && matches!(c, '&' | '|')
                        }) {
                            i += 1;
                        }
                        Lexeme::Op(
                            Operator::lex(&s[start..i])
                                .ok_or_else(|| format!("Unexpected character '{c}'"))?,
                        )
                    }
                    _ => return Err(format!("Unexpected character '{c}'")),
                }
            };
        out.push(Token {
            kind,
            start,
            end: i,
        });
        if out.len() > 2048 {
            return Err("Expression is too long".into());
        }
    }
    Ok(out)
}
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Spanned(usize, usize, Box<Expr>),
    Value(Literal),
    /// An uppercase code such as USD or NVDA, recognized once at parse time so
    /// evaluation never has to re-read the spelling.
    Code(Code),
    Name(String),
    /// A name the parser resolved to a parameter of the lambda it sits in,
    /// kept alongside its slot so tooling still reads it as a name. There is
    /// no depth: an outer lambda's parameter is read out of the closure the
    /// inner lambda captured, never off the frame stack, so only the
    /// innermost parameter list is resolved.
    Param {
        name: String,
        index: usize,
    },
    /// A call to a note function: the callee is a name the runtime resolves.
    Call(String, Vec<Expr>),
    /// A call the parser already recognized as a built-in, so evaluation never
    /// re-reads the spelling.
    Builtin(Builtin, Vec<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
    List(Vec<Expr>),
    Record(Vec<(String, Expr)>),
    /// `fn(a, b = default) => body`: the parameters, the defaults of the
    /// trailing ones that have one (in order), and the body, shared with
    /// every closure made from it.
    Lambda(Vec<String>, Vec<Expr>, Arc<Expr>),
    Apply(Box<Expr>, Vec<Expr>),
}
impl Expr {
    /// Calls `visit` on each operand of this node, past any span, in source
    /// order: a lambda's defaults before its body.
    fn for_each_child<'a>(&'a self, mut visit: impl FnMut(&'a Expr)) {
        match self.bare() {
            Self::Unary(_, e) | Self::Property(e, _) => visit(e),
            Self::Lambda(_, defaults, body) => {
                defaults.iter().for_each(&mut visit);
                visit(body);
            }
            Self::Binary(_, a, b) => {
                visit(a);
                visit(b);
            }
            Self::Call(_, args) | Self::Builtin(_, args) | Self::List(args) => {
                args.iter().for_each(visit)
            }
            Self::Record(fields) => fields.iter().for_each(|(_, e)| visit(e)),
            Self::Apply(f, args) => {
                visit(f);
                args.iter().for_each(visit);
            }
            _ => (),
        }
    }
    /// Visits this node and every descendant, depth first.
    pub fn walk(&self, visit: &mut impl FnMut(&Expr)) {
        visit(self);
        self.for_each_child(|e| e.walk(visit));
    }
    /// Whether this node reads a lambda parameter, whose properties are the
    /// caller's, not a note's.
    pub fn mentions_parameter(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| found |= matches!(e.bare(), Expr::Param { .. }));
        found
    }
    /// Free names and their source positions, shared by references and lexical capture.
    pub fn free_names(&self) -> Vec<(String, usize)> {
        fn visit(expr: &Expr, bound: &[String], start: usize, out: &mut Vec<(String, usize)>) {
            match expr {
                Expr::Spanned(start, _, e) => visit(e, bound, *start, out),
                Expr::Name(name) if !bound.contains(name) => out.push((name.clone(), start)),
                Expr::Lambda(params, defaults, body) => {
                    // A default sees the parameters before its own.
                    let first = params.len() - defaults.len();
                    for (i, e) in defaults.iter().enumerate() {
                        let mut bound = bound.to_vec();
                        bound.extend(params[..first + i].iter().cloned());
                        visit(e, &bound, start, out);
                    }
                    let mut bound = bound.to_vec();
                    bound.extend(params.clone());
                    visit(body, &bound, start, out);
                }
                // A parameter is bound by construction, and has no operands.
                _ => {
                    if let Expr::Call(name, _) = expr
                        && !bound.contains(name)
                    {
                        out.push((name.clone(), start));
                    }
                    expr.for_each_child(|e| visit(e, bound, start, out));
                }
            }
        }
        let mut names = Vec::new();
        visit(self, &[], 0, &mut names);
        names
    }
    /// The name this node spells, whether or not it resolved to a parameter.
    pub fn as_name(&self) -> Option<&str> {
        match self.bare() {
            Self::Name(name) | Self::Param { name, .. } => Some(name),
            _ => None,
        }
    }
    pub fn bare(&self) -> &Self {
        if let Self::Spanned(_, _, e) = self {
            e.bare()
        } else {
            self
        }
    }
    pub fn bounds(&self) -> (usize, usize) {
        if let Self::Spanned(s, e, _) = self {
            (*s, *e)
        } else {
            (0, 0)
        }
    }
}
/// Free variable positions, excluding function parameters and record keys.
pub fn expression_names(source: &str) -> Option<BTreeSet<usize>> {
    Some(
        Parser::parse(source)
            .ok()?
            .free_names()
            .into_iter()
            .map(|(_, start)| start)
            .collect(),
    )
}
/// Whether `source` parses as a complete expression.
pub fn valid_expression(source: &str) -> bool {
    Parser::parse(source).is_ok()
}

pub struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
    /// Parameters of the lambda currently being parsed, saved and restored
    /// around each body, so a name can be resolved to a slot where it is
    /// written rather than looked up every time it is read.
    parameters: Vec<String>,
}

fn simple_name(source: &str) -> Option<String> {
    Parser::parse(source).ok()?.as_name().map(str::to_string)
}

/// Tolerant lexical scopes for sum(table, row_expression). The evaluator checks
/// syntax; this also works before a closing ')' is typed for LSP completion.
pub fn sum_scope_at(source: &str, position: usize) -> Option<String> {
    let tokens = lex(source).ok()?;
    let mut result = None;
    for (i, token) in tokens.iter().enumerate() {
        if !matches!(&token.kind, Lexeme::Name(n) if n == "sum")
            || !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left))
        {
            continue;
        }
        let start = tokens[i + 1].end;
        let mut depth = 0;
        let mut comma = None;
        let mut end = source.len();
        for token in &tokens[i + 2..] {
            match token.kind {
                Lexeme::Left => depth += 1,
                Lexeme::Right if depth > 0 => depth -= 1,
                Lexeme::Right => {
                    end = token.start;
                    break;
                }
                Lexeme::Comma if depth == 0 && comma.is_none() => comma = Some(token),
                _ => {}
            }
        }
        if let Some(comma) = comma {
            if position >= start && position < comma.start {
                return None;
            }
            if position >= comma.end && position <= end {
                result = simple_name(&source[start..comma.start]);
            }
        }
    }
    result
}
impl Parser {
    pub fn parse(s: &str) -> Result<Expr, String> {
        let mut p = Self {
            tokens: lex(s)?,
            at: 0,
            depth: 0,
            parameters: Vec::new(),
        };
        let expr = p.expression(0)?;
        if p.at != p.tokens.len() {
            return Err("Unexpected trailing expression".into());
        }
        let mut pending = vec![(&expr, 0)];
        while let Some((node, depth)) = pending.pop() {
            if depth > 64 {
                return Err("Expression depth exceeds 64 levels".into());
            }
            node.for_each_child(|e| pending.push((e, depth + 1)));
        }
        Ok(expr)
    }
    fn peek(&self) -> Option<&Lexeme> {
        self.tokens.get(self.at).map(|t| &t.kind)
    }
    /// Steps past the next token when it is `kind`.
    fn eat(&mut self, kind: &Lexeme) -> bool {
        let found = self.peek() == Some(kind);
        self.at += usize::from(found);
        found
    }
    /// Steps past the next token, which has to be `kind`.
    fn expect(&mut self, kind: &Lexeme, error: &str) -> Result<(), String> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(error.into())
        }
    }
    fn close(&mut self) -> Result<(), String> {
        self.expect(&Lexeme::Right, "Expected ')'")
    }
    /// Comma-separated items up to `close`, not past it, a trailing comma
    /// allowed. Each item is read knowing the ones before it.
    fn separated<T>(
        &mut self,
        close: &Lexeme,
        mut item: impl FnMut(&mut Self, &[T]) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let mut items = Vec::new();
        while self.peek() != Some(close) {
            items.push(item(self, &items)?);
            if !self.eat(&Lexeme::Comma) {
                break;
            }
        }
        Ok(items)
    }
    /// Runs `parse` with `parameters` as the innermost lambda's, then puts
    /// the enclosing ones back.
    fn scoped<T>(
        &mut self,
        parameters: Vec<String>,
        parse: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let enclosing = std::mem::replace(&mut self.parameters, parameters);
        let parsed = parse(self);
        self.parameters = enclosing;
        parsed
    }
    fn expression(&mut self, min: u8) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("Expression nesting exceeds 64 levels".into());
        }
        let start = self.tokens.get(self.at).map(|t| t.start).unwrap_or(0);
        let token = self.peek().ok_or("Expected an expression")?.clone();
        self.at += 1;
        let mut lhs = match token {
            Lexeme::Value(v) => Expr::Value(v),
            Lexeme::OpenList => Expr::List(self.arguments(false)?),
            Lexeme::OpenRecord => Expr::Record(self.fields(
                "Expected a record field name",
                |p, key, (start, end), value| {
                    value.unwrap_or_else(|| Expr::Spanned(start, end, Box::new(p.name(key.into()))))
                },
            )?),
            Lexeme::Name(n) if n == "fn" => {
                self.expect(&Lexeme::Left, "Expected fn(parameters) => expression")?;
                let params = self.separated(&Lexeme::Right, |p, before: &[(String, _)]| {
                    let Some(Lexeme::Name(name)) = p.peek() else {
                        return Err("Expected a parameter name".into());
                    };
                    let name = name.clone();
                    if before.iter().any(|(param, _)| *param == name)
                        || matches!(name.as_str(), "true" | "false" | "null" | "fn")
                        || is_code(&name)
                    {
                        return Err(format!("Invalid or duplicate parameter '{name}'"));
                    }
                    p.at += 1;
                    // `name = default`: this and every later parameter may be
                    // left out of a call, which then evaluates the default
                    // with the parameters before it bound.
                    if p.eat(&Lexeme::Op(Operator::Unsupported("="))) {
                        let before = before.iter().map(|(param, _)| param.clone()).collect();
                        let default = p.scoped(before, |p| p.expression(0))?;
                        Ok((name, Some(default)))
                    } else if before.iter().any(|(_, default)| default.is_some()) {
                        Err(format!(
                            "Parameter '{name}' needs a default: it follows one that has one"
                        ))
                    } else {
                        Ok((name, None))
                    }
                })?;
                self.close()?;
                self.expect(&Lexeme::Op(Operator::Arrow), "Expected '=>'")?;
                let (params, defaults): (Vec<_>, Vec<_>) = params.into_iter().unzip();
                let body = self.scoped(params.clone(), |p| p.expression(0))?;
                Expr::Lambda(
                    params,
                    defaults.into_iter().flatten().collect(),
                    Arc::new(body),
                )
            }
            Lexeme::Name(n) if n == "let" && self.peek() == Some(&Lexeme::Left) => {
                self.at += 1;
                self.let_form(start)?
            }
            Lexeme::Name(n) if self.eat(&Lexeme::Left) => {
                let mut args = Vec::new();
                if self.peek() != Some(&Lexeme::Right) {
                    loop {
                        args.push(self.expression(0)?);
                        if !self.eat(&Lexeme::Comma) {
                            break;
                        }
                    }
                }
                self.close()?;
                // The callee's spelling is decided once, here: a built-in
                // never has to be recognized again while a note evaluates.
                match n.parse::<Builtin>() {
                    Ok(builtin) => Expr::Builtin(builtin, args),
                    Err(()) => Expr::Call(n, args),
                }
            }
            Lexeme::Name(n) => self.name(n),
            Lexeme::Left => {
                let v = self.expression(0)?;
                self.close()?;
                v
            }
            Lexeme::Op(op) if op.unary().is_some() => {
                let operand = self.expression(UNARY_PRECEDENCE)?;
                if is_row_function(&operand) {
                    return Err(ROW_OPERAND.into());
                }
                Expr::Unary(op.unary().unwrap(), Box::new(operand))
            }
            Lexeme::Dot => {
                let body = self.scoped(vec![ROW.into()], Self::row_body)?;
                Expr::Lambda(vec![ROW.into()], vec![], Arc::new(body))
            }
            _ => return Err("Expected a value, name, or function".into()),
        };
        lhs = Expr::Spanned(start, self.tokens[self.at - 1].end, Box::new(lhs));
        loop {
            if self.eat(&Lexeme::Left) {
                let args = self.arguments(true)?;
                lhs = Expr::Spanned(
                    start,
                    self.tokens[self.at - 1].end,
                    Box::new(Expr::Apply(Box::new(lhs), args)),
                );
                continue;
            }
            if self.eat(&Lexeme::Dot) {
                let Some(Token {
                    kind: Lexeme::Name(n),
                    end,
                    ..
                }) = self.tokens.get(self.at)
                else {
                    return Err("Expected property name".into());
                };
                lhs = Expr::Spanned(
                    start,
                    *end,
                    Box::new(Expr::Property(Box::new(lhs), n.clone())),
                );
                self.at += 1;
                continue;
            }
            let Some(&Lexeme::Op(op)) = self.peek() else {
                break;
            };
            if op == Operator::Pipe {
                if PIPE_PRECEDENCE < min {
                    break;
                }
                self.at += 1;
                lhs = self.pipe(start, lhs)?;
                continue;
            }
            let Some(op) = op.binary() else {
                return Err(format!("Unknown operator {op}"));
            };
            let bp = op.precedence();
            if bp < min {
                break;
            }
            self.at += 1;
            let rhs = self.expression(bp + 1)?;
            if is_row_function(&lhs) || is_row_function(&rhs) {
                return Err(ROW_OPERAND.into());
            }
            lhs = Expr::Spanned(
                start,
                rhs.bounds().1,
                Box::new(Expr::Binary(op, Box::new(lhs), Box::new(rhs))),
            );
        }
        self.depth -= 1;
        Ok(lhs)
    }
    /// After a `|`: the function on the right, called with `input` as its
    /// first argument. `xs | f(a)` is `f(xs, a)` and `xs | f` is `f(xs)`, so
    /// a pipe leaves nothing behind for evaluation to know about.
    fn pipe(&mut self, start: usize, input: Expr) -> Result<Expr, String> {
        let token = self
            .tokens
            .get(self.at)
            .ok_or("Expected a function after '|'")?;
        let next = self.tokens.get(self.at + 1).map(|t| &t.kind);
        if let Lexeme::Name(name) = &token.kind {
            if name == "let" {
                return Err("A value cannot be piped into let".into());
            }
            // `tasks | where !done` was the query pipeline's; say what replaced it.
            if let Some(instead) = retired_stage(name)
                && next.is_some_and(begins_expression)
            {
                return Err(format!(
                    "Pipeline stages are gone: write | {instead} instead of | {name} …"
                ));
            }
        }
        let callee = self.expression(PIPE_PRECEDENCE + 1)?;
        let piped = match callee.bare().clone() {
            Expr::Builtin(builtin, args) => {
                Expr::Builtin(builtin, [input].into_iter().chain(args).collect())
            }
            Expr::Call(name, args) => Expr::Call(name, [input].into_iter().chain(args).collect()),
            Expr::Apply(function, args) => {
                Expr::Apply(function, [input].into_iter().chain(args).collect())
            }
            Expr::Name(name) => match name.parse::<Builtin>() {
                Ok(builtin) => Expr::Builtin(builtin, vec![input]),
                Err(()) => Expr::Call(name, vec![input]),
            },
            Expr::Param { .. } | Expr::Property(..) | Expr::Lambda(..) => {
                Expr::Apply(Box::new(callee.clone()), vec![input])
            }
            _ => return Err("Expected a function or a call after '|'".into()),
        };
        Ok(Expr::Spanned(start, callee.bounds().1, Box::new(piped)))
    }
    /// After a leading `.`, with the row as the only parameter: `.a.b` is
    /// `fn(x) => x.a.b`, and `.{a, b: .c}` is `fn(x) => {a: x.a, b: x.c}`.
    /// The parameter cannot be written in source, so it never shadows a name.
    fn row_body(&mut self) -> Result<Expr, String> {
        let row = || Expr::Param {
            name: ROW.into(),
            index: 0,
        };
        match self.peek() {
            Some(Lexeme::Name(_)) => {
                let mut path = row();
                loop {
                    let Some(Token {
                        kind: Lexeme::Name(field),
                        start,
                        end,
                    }) = self.tokens.get(self.at)
                    else {
                        return Err("Expected property name".into());
                    };
                    path = Expr::Spanned(
                        *start,
                        *end,
                        Box::new(Expr::Property(Box::new(path), field.clone())),
                    );
                    self.at += 1;
                    if !self.eat(&Lexeme::Dot) {
                        return Ok(path);
                    }
                }
            }
            Some(Lexeme::OpenRecord) => {
                self.at += 1;
                Ok(Expr::Record(self.fields(
                    "Expected '}'",
                    |_, key, (start, end), value| {
                        match value {
                            // A function of the row, `.c` or `fn(r) => …`, is applied
                            // to it; anything else is the field's value as written.
                            Some(value) => match value.bare() {
                                Expr::Lambda(params, _, _) if params.len() == 1 => {
                                    let (s, e) = value.bounds();
                                    Expr::Spanned(
                                        s,
                                        e,
                                        Box::new(Expr::Apply(Box::new(value), vec![row()])),
                                    )
                                }
                                _ => value,
                            },
                            None => Expr::Spanned(
                                start,
                                end,
                                Box::new(Expr::Property(Box::new(row()), key.into())),
                            ),
                        }
                    },
                )?))
            }
            _ => Err("Expected a field name or {…} after '.'".into()),
        }
    }
    /// A record's fields after its `{`, through its `}`: `key: value`, or a
    /// bare identifier `key` as shorthand, which `field` is handed with no
    /// value. `eof` is the error for text that ends inside it.
    fn fields(
        &mut self,
        eof: &str,
        field: impl Fn(&Self, &str, (usize, usize), Option<Expr>) -> Expr,
    ) -> Result<Vec<(String, Expr)>, String> {
        let fields = self.separated(&Lexeme::CloseRecord, |p, before: &[(String, Expr)]| {
            let token = p.tokens.get(p.at).ok_or(eof)?;
            let (Lexeme::Name(key) | Lexeme::Value(Literal::Text(key))) = &token.kind else {
                return Err("Expected a record field name".into());
            };
            let (key, bounds) = (key.clone(), (token.start, token.end));
            if before.iter().any(|(k, _)| *k == key) {
                return Err(format!("Duplicate field '{key}'"));
            }
            p.at += 1;
            let value = if p.eat(&Lexeme::Colon) {
                Some(p.expression(0)?)
            } else if identifier(&key) {
                None
            } else {
                return Err("Expected ':'".into());
            };
            let value = field(p, &key, bounds, value);
            Ok((key, value))
        })?;
        self.expect(&Lexeme::CloseRecord, "Expected '}'")?;
        Ok(fields)
    }
    /// A bare name: an uppercase code is a literal, and a name that spells a
    /// parameter of the enclosing lambda is resolved to its slot.
    fn name(&self, name: String) -> Expr {
        if let Some(code) = Code::parse(&name) {
            return Expr::Code(code);
        }
        match self.parameters.iter().position(|p| *p == name) {
            Some(index) => Expr::Param { name, index },
            None => Expr::Name(name),
        }
    }
    /// `let({a: 1, b: a + 1}, body)`, after its `(`: names bound once, each
    /// seeing the ones before it. It is a lambda call written the other way
    /// round, `(fn(a) => (fn(b) => body)(a + 1))(1)`, and parses into exactly
    /// that, so evaluation and capture work as they do for any function.
    fn let_form(&mut self, start: usize) -> Result<Expr, String> {
        const SHAPE: &str = "let expects {name: value, …} and a body";
        self.expect(&Lexeme::OpenRecord, SHAPE)?;
        let (bindings, body) = self.scoped(self.parameters.clone(), |p| {
            let bindings = p.let_bindings()?;
            if !p.eat(&Lexeme::Comma) {
                return Err(SHAPE.into());
            }
            if let Some((name, _)) = bindings.last() {
                p.parameters = vec![name.clone()];
            }
            Ok((bindings, p.expression(0)?))
        })?;
        self.close()?;
        let end = self.tokens[self.at - 1].end;
        Ok(bindings
            .into_iter()
            .rev()
            .fold(body, |body, (name, value)| {
                Expr::Spanned(
                    start,
                    end,
                    Box::new(Expr::Apply(
                        Box::new(Expr::Lambda(vec![name], vec![], Arc::new(body))),
                        vec![value],
                    )),
                )
            }))
    }
    /// The `{name: value, …}` of a `let`, after its `{`. Each value is parsed
    /// with the previous name as the innermost parameter, as its lambda has it.
    fn let_bindings(&mut self) -> Result<Vec<(String, Expr)>, String> {
        let bindings = self.separated(&Lexeme::CloseRecord, |p, before: &[(String, Expr)]| {
            let Some(Lexeme::Name(name)) = p.peek() else {
                return Err("Expected a name to bind".into());
            };
            let name = name.clone();
            if before.iter().any(|(bound, _)| *bound == name)
                || matches!(name.as_str(), "true" | "false" | "null" | "fn" | "let")
                || is_code(&name)
            {
                return Err(format!("Invalid or duplicate name '{name}' in let"));
            }
            p.at += 1;
            if !p.eat(&Lexeme::Colon) {
                return Err(format!("Expected ':' after '{name}' in let"));
            }
            if let Some((previous, _)) = before.last() {
                p.parameters = vec![previous.clone()];
            }
            Ok((name, p.expression(0)?))
        })?;
        self.expect(&Lexeme::CloseRecord, "Expected '}'")?;
        Ok(bindings)
    }
    fn arguments(&mut self, parentheses: bool) -> Result<Vec<Expr>, String> {
        let close = if parentheses {
            Lexeme::Right
        } else {
            Lexeme::CloseList
        };
        let args = self.separated(&close, |p, _| p.expression(0))?;
        self.expect(&close, "Unclosed arguments or list")?;
        Ok(args)
    }
}

/// The parameter of `.a` and `.{…}`: not an identifier, so no note can name it.
const ROW: &str = ".";
const ROW_OPERAND: &str = "`.field` is a function of a row, not a value: write fn(x) => x.field to use it in an expression";

/// Whether `expr` is `.a` or `.{…}`, which an operator cannot take.
fn is_row_function(expr: &Expr) -> bool {
    matches!(expr.bare(), Expr::Lambda(params, _, _) if params.len() == 1 && params[0] == ROW)
}

/// The function that replaced a stage of the retired query pipeline.
fn retired_stage(name: &str) -> Option<&'static str> {
    Some(match name {
        "where" => "filter(fn(x) => …)",
        "select" => "map(.{…})",
        "sort" => "sort_by(.field)",
        "limit" => "slice(0, n)",
        "group" => "group_by(.field)",
        _ => return None,
    })
}

/// Whether a token can start an operand, as a stage's argument did.
fn begins_expression(kind: &Lexeme) -> bool {
    matches!(
        kind,
        Lexeme::Name(_)
            | Lexeme::Value(_)
            | Lexeme::Left
            | Lexeme::OpenList
            | Lexeme::OpenRecord
            | Lexeme::Dot
            | Lexeme::Op(Operator::Not)
    )
}

/// Names reserved by the evaluator, also used by references and editor highlighting.
pub fn is_builtin_function(name: &str) -> bool {
    name.parse::<super::Builtin>().is_ok()
}
/// A bare word a note can name a definition, task, section, column or
/// parameter with.
pub fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
