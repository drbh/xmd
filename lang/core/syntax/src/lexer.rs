//! Syntax: the lexer, the expression tree, and the tolerant source scans the
//! editor features read without evaluating anything.
use super::{BinaryOp, Builtin, Literal, Operator, UnaryOp, date_value, literal};
use common::{Code, Currency, is_code};
use std::collections::BTreeSet;
#[derive(Clone, Debug)]
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
                    let ch = s.as_bytes()[i];
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
                if s.as_bytes().get(i) == Some(&b'T') {
                    while i < s.len()
                        && !s.as_bytes()[i].is_ascii_whitespace()
                        && !matches!(s.as_bytes()[i], b')' | b',' | b']' | b'}')
                    {
                        i += 1;
                    }
                }
                Lexeme::Value(date_value(&s[start..i]).ok_or(
                    "Invalid date/time (use an explicit offset for ambiguous local times)",
                )?)
            } else if c.is_ascii_digit()
                || Currency::from_symbol(c).is_some()
                || c == '.' && s.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                i += c.len_utf8();
                while i < s.len()
                    && (s.as_bytes()[i].is_ascii_digit()
                        || s.as_bytes()[i] == b'.'
                        || s.as_bytes()[i] == b','
                            && s.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit))
                {
                    i += 1;
                }
                if s.as_bytes()
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
                i += 1;
                while i < s.len()
                    && (s.as_bytes()[i].is_ascii_alphanumeric() || s.as_bytes()[i] == b'_')
                {
                    i += 1;
                }
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
                        if s.as_bytes().get(i).is_some_and(|b| {
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
    Lambda(Vec<String>, Box<Expr>),
    Apply(Box<Expr>, Vec<Expr>),
}
impl Expr {
    /// Visits this node and every descendant, depth first.
    pub fn walk(&self, visit: &mut impl FnMut(&Expr)) {
        visit(self);
        match self.bare() {
            Self::Unary(_, e) | Self::Property(e, _) | Self::Lambda(_, e) => e.walk(visit),
            Self::Binary(_, a, b) => {
                a.walk(visit);
                b.walk(visit);
            }
            Self::Call(_, args) | Self::Builtin(_, args) | Self::List(args) => {
                args.iter().for_each(|e| e.walk(visit))
            }
            Self::Record(fields) => fields.iter().for_each(|(_, e)| e.walk(visit)),
            Self::Apply(f, args) => {
                f.walk(visit);
                args.iter().for_each(|e| e.walk(visit));
            }
            _ => (),
        }
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
                // A parameter is bound by construction.
                Expr::Param { .. } => (),
                Expr::Lambda(params, body) => {
                    let mut bound = bound.to_vec();
                    bound.extend(params.clone());
                    visit(body, &bound, start, out);
                }
                Expr::Unary(_, e) | Expr::Property(e, _) => visit(e, bound, start, out),
                Expr::Binary(_, a, b) => {
                    visit(a, bound, start, out);
                    visit(b, bound, start, out);
                }
                Expr::Call(name, items) => {
                    if !bound.contains(name) {
                        out.push((name.clone(), start));
                    }
                    for e in items {
                        visit(e, bound, start, out);
                    }
                }
                Expr::Builtin(_, items) | Expr::List(items) => {
                    for e in items {
                        visit(e, bound, start, out);
                    }
                }
                Expr::Record(fields) => {
                    for (_, e) in fields {
                        visit(e, bound, start, out);
                    }
                }
                Expr::Apply(f, args) => {
                    visit(f, bound, start, out);
                    for e in args {
                        visit(e, bound, start, out);
                    }
                }
                _ => (),
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
            match node {
                Expr::Spanned(_, _, e) => pending.push((e, depth)),
                Expr::Unary(_, e) | Expr::Property(e, _) | Expr::Lambda(_, e) => {
                    pending.push((e, depth + 1))
                }
                Expr::Binary(_, a, b) => {
                    pending.push((a, depth + 1));
                    pending.push((b, depth + 1));
                }
                Expr::List(items) | Expr::Call(_, items) | Expr::Builtin(_, items) => {
                    pending.extend(items.iter().map(|e| (e, depth + 1)))
                }
                Expr::Record(fields) => pending.extend(fields.iter().map(|(_, e)| (e, depth + 1))),
                Expr::Apply(f, args) => {
                    pending.push((f, depth + 1));
                    pending.extend(args.iter().map(|e| (e, depth + 1)));
                }
                _ => (),
            }
        }
        Ok(expr)
    }
    fn expression(&mut self, min: u8) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("Expression nesting exceeds 64 levels".into());
        }
        let start = self.tokens.get(self.at).map(|t| t.start).unwrap_or(0);
        let token = self
            .tokens
            .get(self.at)
            .ok_or("Expected an expression")?
            .kind
            .clone();
        self.at += 1;
        let mut lhs = match token {
            Lexeme::Value(v) => Expr::Value(v),
            Lexeme::OpenList => Expr::List(self.arguments(false)?),
            Lexeme::OpenRecord => {
                let mut fields = Vec::new();
                while !matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Lexeme::CloseRecord)
                ) {
                    let key = match self.tokens.get(self.at).map(|t| &t.kind) {
                        Some(Lexeme::Name(key)) | Some(Lexeme::Value(Literal::Text(key))) => {
                            key.clone()
                        }
                        _ => return Err("Expected a record field name".into()),
                    };
                    if fields.iter().any(|(k, _)| k == &key) {
                        return Err(format!("Duplicate field '{key}'"));
                    }
                    self.at += 1;
                    let value = if matches!(
                        self.tokens.get(self.at).map(|t| &t.kind),
                        Some(Lexeme::Colon)
                    ) {
                        self.at += 1;
                        self.expression(0)?
                    } else if identifier(&key) {
                        let token = &self.tokens[self.at - 1];
                        Expr::Spanned(token.start, token.end, Box::new(self.name(key.clone())))
                    } else {
                        return Err("Expected ':'".into());
                    };
                    fields.push((key, value));
                    if !matches!(
                        self.tokens.get(self.at).map(|t| &t.kind),
                        Some(Lexeme::Comma)
                    ) {
                        break;
                    }
                    self.at += 1;
                }
                if !matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Lexeme::CloseRecord)
                ) {
                    return Err("Expected '}'".into());
                }
                self.at += 1;
                Expr::Record(fields)
            }
            Lexeme::Name(n) if n == "fn" => {
                if !matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Lexeme::Left)
                ) {
                    return Err("Expected fn(parameters) => expression".into());
                }
                self.at += 1;
                let mut params = Vec::new();
                while !matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Lexeme::Right)
                ) {
                    let Some(Token {
                        kind: Lexeme::Name(name),
                        ..
                    }) = self.tokens.get(self.at)
                    else {
                        return Err("Expected a parameter name".into());
                    };
                    if params.contains(name)
                        || matches!(name.as_str(), "true" | "false" | "null" | "fn")
                        || is_code(name)
                    {
                        return Err(format!("Invalid or duplicate parameter '{name}'"));
                    }
                    params.push(name.clone());
                    self.at += 1;
                    if !matches!(
                        self.tokens.get(self.at).map(|t| &t.kind),
                        Some(Lexeme::Comma)
                    ) {
                        break;
                    }
                    self.at += 1;
                }
                self.close()?;
                if !matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Lexeme::Op(Operator::Arrow))
                ) {
                    return Err("Expected '=>'".into());
                }
                self.at += 1;
                let enclosing = std::mem::replace(&mut self.parameters, params.clone());
                let body = self.expression(0);
                self.parameters = enclosing;
                Expr::Lambda(params, Box::new(body?))
            }
            Lexeme::Name(n)
                if n == "let"
                    && matches!(
                        self.tokens.get(self.at).map(|t| &t.kind),
                        Some(Lexeme::Left)
                    ) =>
            {
                self.at += 1;
                self.let_form(start)?
            }
            Lexeme::Name(n) => {
                if matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Lexeme::Left)
                ) {
                    self.at += 1;
                    let mut args = Vec::new();
                    if !matches!(
                        self.tokens.get(self.at).map(|t| &t.kind),
                        Some(Lexeme::Right)
                    ) {
                        loop {
                            args.push(self.expression(0)?);
                            if !matches!(
                                self.tokens.get(self.at).map(|t| &t.kind),
                                Some(Lexeme::Comma)
                            ) {
                                break;
                            }
                            self.at += 1;
                        }
                    }
                    self.close()?;
                    // The callee's spelling is decided once, here: a built-in
                    // never has to be recognized again while a note evaluates.
                    match n.parse::<Builtin>() {
                        Ok(builtin) => Expr::Builtin(builtin, args),
                        Err(()) => Expr::Call(n, args),
                    }
                } else {
                    self.name(n)
                }
            }
            Lexeme::Left => {
                let v = self.expression(0)?;
                self.close()?;
                v
            }
            Lexeme::Op(op) if op.unary().is_some() => {
                Expr::Unary(op.unary().unwrap(), Box::new(self.expression(7)?))
            }
            _ => return Err("Expected a value, name, or function".into()),
        };
        lhs = Expr::Spanned(start, self.tokens[self.at - 1].end, Box::new(lhs));
        loop {
            if matches!(
                self.tokens.get(self.at).map(|t| &t.kind),
                Some(Lexeme::Left)
            ) {
                self.at += 1;
                let args = self.arguments(true)?;
                lhs = Expr::Spanned(
                    start,
                    self.tokens[self.at - 1].end,
                    Box::new(Expr::Apply(Box::new(lhs), args)),
                );
                continue;
            }
            if matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(Lexeme::Dot)) {
                self.at += 1;
                let Some(Token {
                    kind: Lexeme::Name(n),
                    ..
                }) = self.tokens.get(self.at)
                else {
                    return Err("Expected property name".into());
                };
                lhs = Expr::Spanned(
                    start,
                    self.tokens[self.at].end,
                    Box::new(Expr::Property(Box::new(lhs), n.clone())),
                );
                self.at += 1;
                continue;
            }
            let Some(Token {
                kind: Lexeme::Op(op),
                ..
            }) = self.tokens.get(self.at)
            else {
                break;
            };
            let Some(op) = op.binary() else {
                return Err(format!("Unknown operator {op}"));
            };
            let bp = op.precedence();
            if bp < min {
                break;
            }
            self.at += 1;
            let rhs = self.expression(bp + 1)?;
            lhs = Expr::Spanned(
                start,
                rhs.bounds().1,
                Box::new(Expr::Binary(op, Box::new(lhs), Box::new(rhs))),
            );
        }
        self.depth -= 1;
        Ok(lhs)
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
        if !matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Lexeme::OpenRecord)
        ) {
            return Err(SHAPE.into());
        }
        self.at += 1;
        let enclosing = self.parameters.clone();
        let parsed = self.let_bindings();
        let bindings = match parsed {
            Ok(bindings) => bindings,
            Err(error) => {
                self.parameters = enclosing;
                return Err(error);
            }
        };
        let body = if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Lexeme::Comma)
        ) {
            self.at += 1;
            if let Some((name, _)) = bindings.last() {
                self.parameters = vec![name.clone()];
            }
            self.expression(0)
        } else {
            Err(SHAPE.into())
        };
        self.parameters = enclosing;
        let body = body?;
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
                        Box::new(Expr::Lambda(vec![name], Box::new(body))),
                        vec![value],
                    )),
                )
            }))
    }
    /// The `{name: value, …}` of a `let`, after its `{`. Each value is parsed
    /// with the previous name as the innermost parameter, as its lambda has it.
    fn let_bindings(&mut self) -> Result<Vec<(String, Expr)>, String> {
        let mut bindings: Vec<(String, Expr)> = Vec::new();
        while !matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Lexeme::CloseRecord)
        ) {
            let Some(Token {
                kind: Lexeme::Name(name),
                ..
            }) = self.tokens.get(self.at)
            else {
                return Err("Expected a name to bind".into());
            };
            let name = name.clone();
            if bindings.iter().any(|(bound, _)| *bound == name)
                || matches!(name.as_str(), "true" | "false" | "null" | "fn" | "let")
                || is_code(&name)
            {
                return Err(format!("Invalid or duplicate name '{name}' in let"));
            }
            self.at += 1;
            if !matches!(
                self.tokens.get(self.at).map(|t| &t.kind),
                Some(Lexeme::Colon)
            ) {
                return Err(format!("Expected ':' after '{name}' in let"));
            }
            self.at += 1;
            if let Some((previous, _)) = bindings.last() {
                self.parameters = vec![previous.clone()];
            }
            let value = self.expression(0)?;
            bindings.push((name, value));
            if !matches!(
                self.tokens.get(self.at).map(|t| &t.kind),
                Some(Lexeme::Comma)
            ) {
                break;
            }
            self.at += 1;
        }
        if !matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Lexeme::CloseRecord)
        ) {
            return Err("Expected '}'".into());
        }
        self.at += 1;
        Ok(bindings)
    }
    fn close(&mut self) -> Result<(), String> {
        if !matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Lexeme::Right)
        ) {
            return Err("Expected ')'".into());
        }
        self.at += 1;
        Ok(())
    }
    fn arguments(&mut self, parentheses: bool) -> Result<Vec<Expr>, String> {
        let closed = |token: Option<&Token>| {
            matches!(token.map(|t| &t.kind), Some(Lexeme::Right) if parentheses)
                || matches!(token.map(|t| &t.kind), Some(Lexeme::CloseList) if !parentheses)
        };
        let mut args = Vec::new();
        while !closed(self.tokens.get(self.at)) {
            args.push(self.expression(0)?);
            if !matches!(
                self.tokens.get(self.at).map(|t| &t.kind),
                Some(Lexeme::Comma)
            ) {
                break;
            }
            self.at += 1;
        }
        if !closed(self.tokens.get(self.at)) {
            return Err("Unclosed arguments or list".into());
        }
        self.at += 1;
        Ok(args)
    }
}

/// Arguments of a direct timer declaration, retaining the original duration expression.
pub fn timer_arguments(source: &str) -> Option<Vec<&str>> {
    let parsed = Parser::parse(source).ok()?;
    let Expr::Builtin(builtin, _) = parsed.bare() else {
        return None;
    };
    if !matches!(builtin, Builtin::Stopwatch | Builtin::Countdown) {
        return None;
    }
    let name = builtin.as_str();
    let tokens = lex(source).ok()?;
    let call = tokens
        .iter()
        .position(|t| matches!(&t.kind, Lexeme::Name(n) if n == name))?;
    if !matches!(tokens.get(call + 1)?.kind, Lexeme::Left) {
        return None;
    }
    let mut start = tokens[call + 1].end;
    let mut depth = 0;
    let mut args = Vec::new();
    for token in &tokens[call + 2..] {
        match token.kind {
            Lexeme::Left => depth += 1,
            Lexeme::Right if depth > 0 => depth -= 1,
            Lexeme::Comma | Lexeme::Right if depth == 0 => {
                let arg = source[start..token.start].trim();
                if !arg.is_empty() {
                    args.push(arg);
                }
                start = token.end;
                if matches!(token.kind, Lexeme::Right) {
                    break;
                }
            }
            _ => {}
        }
    }
    Some(args)
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
