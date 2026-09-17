use crate::{
    resources::Resource,
    timers::Timer,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, Months, NaiveDate, NaiveDateTime, TimeZone,
    Weekday,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub type TaskKey = (PathBuf, usize);
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Count(usize),
    Money(f64),
    Ratio(f64),
    /// Whole seconds, including for estimates and date arithmetic.
    Duration(i64),
    Date(NaiveDate),
    DateTime(DateTime<FixedOffset>),
    Bool(bool),
    Text(String),
    Resource(Resource),
    Tasks(Vec<TaskKey>),
    Timer(Timer),
}
impl Value {
    pub fn display(&self) -> String {
        match self {
            Self::Number(n) => decimal(*n),
            Self::Count(n) => n.to_string(),
            Self::Money(n) => money(*n),
            Self::Ratio(n) => format!("{}%", decimal(n * 100.0)),
            Self::Duration(s) => {
                if *s == 0 {
                    "0s".into()
                } else if s % 86400 == 0 {
                    format!("{}d", s / 86400)
                } else if s % 3600 == 0 {
                    format!("{}h", s / 3600)
                } else if s % 60 == 0 {
                    format!("{}m", s / 60)
                } else if s.unsigned_abs() >= 60 {
                    format!(
                        "{}{}m {}s",
                        if *s < 0 { "-" } else { "" },
                        s.unsigned_abs() / 60,
                        s.unsigned_abs() % 60
                    )
                } else {
                    format!("{s}s")
                }
            }
            Self::Date(d) => d.to_string(),
            Self::DateTime(d) => d.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Text(s) => s.clone(),
            Self::Resource(r) => r.target.clone(),
            Self::Tasks(t) => format!("{} tasks", t.len()),
            Self::Timer(t) => t.display(),
        }
    }
    pub fn date(&self) -> Result<NaiveDate, String> {
        match self {
            Self::Date(d) => Ok(*d),
            Self::DateTime(d) => Ok(d.with_timezone(&Local).date_naive()),
            _ => Err("Expected a date or appointment time".into()),
        }
    }
    fn scalar(&self) -> Option<f64> {
        match self {
            Self::Number(n) | Self::Ratio(n) => Some(*n),
            Self::Count(n) => Some(*n as f64),
            _ => None,
        }
    }
}
fn decimal(n: f64) -> String {
    let s = format!("{n:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
fn money(n: f64) -> String {
    let s = format!("{:.2}", n.abs());
    let (whole, frac) = s.split_once('.').unwrap();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!(
        "{}${grouped}{}",
        if n < 0.0 { "-" } else { "" },
        if frac == "00" {
            String::new()
        } else {
            format!(".{frac}")
        }
    )
}
pub fn duration(s: &str) -> Option<i64> {
    let (n, unit) = s.split_at(s.char_indices().last()?.0);
    let factor = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        "w" => 604800.0,
        _ => return None,
    };
    let n = n.parse::<f64>().ok()? * factor;
    (n.is_finite() && n.fract() == 0.0 && n.abs() < i64::MAX as f64).then_some(n as i64)
}
pub fn date_value(s: &str) -> Option<Value> {
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(Value::Date(date));
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(s) {
        return Some(Value::DateTime(date));
    }
    if let Ok(date) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M%:z") {
        return Some(Value::DateTime(date));
    }
    for format in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(s, format) {
            return Local
                .from_local_datetime(&date)
                .single()
                .map(|d| Value::DateTime(d.fixed_offset()));
        }
    }
    None
}
pub fn relative_date(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "today" => return Some(today),
        "tomorrow" => return today.succ_opt(),
        "yesterday" => return today.pred_opt(),
        _ => {}
    }
    let weekday = match s.strip_prefix("next ")? {
        "monday" => Weekday::Mon,
        "tuesday" => Weekday::Tue,
        "wednesday" => Weekday::Wed,
        "thursday" => Weekday::Thu,
        "friday" => Weekday::Fri,
        "saturday" => Weekday::Sat,
        "sunday" => Weekday::Sun,
        _ => return None,
    };
    let mut delta = (weekday.num_days_from_monday() as i64
        - today.weekday().num_days_from_monday() as i64)
        .rem_euclid(7);
    if delta == 0 {
        delta = 7;
    }
    today.checked_add_signed(Duration::days(delta))
}
pub fn literal(s: &str) -> Result<Value, String> {
    let s = s.trim();
    if let Some(r) = Resource::parse(s) {
        return Ok(Value::Resource(r));
    }
    if let Some(v) = date_value(s) {
        return Ok(v);
    }
    if let Some(v) = duration(s) {
        return Ok(Value::Duration(v));
    }
    if s == "true" || s == "false" {
        return Ok(Value::Bool(s == "true"));
    }
    let money = s.starts_with('$') || s.starts_with("-$");
    let num = s.replace(['$', ',', '%'], "");
    if let Ok(n) = num.parse::<f64>() {
        if !n.is_finite() {
            return Err("Number must be finite".into());
        }
        return Ok(if money {
            Value::Money(n)
        } else if s.ends_with('%') {
            Value::Ratio(n / 100.0)
        } else {
            Value::Number(n)
        });
    }
    if s.starts_with('"') && s.ends_with('"') {
        return serde_json::from_str::<String>(s)
            .map(Value::Text)
            .map_err(|e| e.to_string());
    }
    Ok(Value::Text(s.into()))
}

#[derive(Clone, Debug)]
pub enum Lexeme {
    Value(Value),
    Name(String),
    Op(String),
    Left,
    Right,
    Comma,
    Dot,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Lexeme,
    pub start: usize,
    pub end: usize,
}
pub fn lex(s: &str) -> Result<Vec<Token>, String> {
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
            if c == '"' {
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
                        && !matches!(s.as_bytes()[i], b')' | b',')
                    {
                        i += 1;
                    }
                }
                Lexeme::Value(date_value(&s[start..i]).ok_or(
                    "Invalid date/time (use an explicit offset for ambiguous local times)",
                )?)
            } else if c.is_ascii_digit()
                || c == '$'
                || c == '.' && s.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                i += 1;
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
                let v = literal(&s[start..i])?;
                if matches!(v, Value::Text(_)) {
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
                    '+' | '-' | '*' | '/' | '!' | '=' | '<' | '>' | '&' | '|' => {
                        if s.as_bytes()
                            .get(i)
                            .is_some_and(|b| *b == b'=' || *b == c as u8 && matches!(c, '&' | '|'))
                        {
                            i += 1;
                        }
                        Lexeme::Op(s[start..i].into())
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
#[derive(Clone, Debug)]
enum Expr {
    Value(Value),
    Name(String),
    Call(String, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
}
struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}
impl Parser {
    fn parse(s: &str) -> Result<Expr, String> {
        let mut p = Self {
            tokens: lex(s)?,
            at: 0,
            depth: 0,
        };
        let expr = p.expression(0)?;
        if p.at != p.tokens.len() {
            return Err("Unexpected trailing expression".into());
        }
        Ok(expr)
    }
    fn expression(&mut self, min: u8) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("Expression nesting exceeds 64 levels".into());
        }
        let token = self
            .tokens
            .get(self.at)
            .ok_or("Expected an expression")?
            .kind
            .clone();
        self.at += 1;
        let mut lhs = match token {
            Lexeme::Value(v) => Expr::Value(v),
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
                    Expr::Call(n, args)
                } else {
                    Expr::Name(n)
                }
            }
            Lexeme::Left => {
                let v = self.expression(0)?;
                self.close()?;
                v
            }
            Lexeme::Op(op) if matches!(op.as_str(), "-" | "+" | "!") => {
                Expr::Unary(op, Box::new(self.expression(7)?))
            }
            _ => return Err("Expected a value, name, or function".into()),
        };
        loop {
            if matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(Lexeme::Dot)) {
                self.at += 1;
                let Some(Token {
                    kind: Lexeme::Name(n),
                    ..
                }) = self.tokens.get(self.at)
                else {
                    return Err("Expected property name".into());
                };
                lhs = Expr::Property(Box::new(lhs), n.clone());
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
            let bp = match op.as_str() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" => 3,
                "<" | "<=" | ">" | ">=" => 4,
                "+" | "-" => 5,
                "*" | "/" => 6,
                _ => return Err(format!("Unknown operator {op}")),
            };
            if bp < min {
                break;
            }
            let op = op.clone();
            self.at += 1;
            let rhs = self.expression(bp + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        self.depth -= 1;
        Ok(lhs)
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
}

/// Arguments of a direct timer declaration, retaining the original duration expression.
pub fn timer_arguments(source: &str) -> Option<Vec<&str>> {
    let Expr::Call(name, _) = Parser::parse(source).ok()? else {
        return None;
    };
    if !matches!(name.as_str(), "stopwatch" | "countdown") {
        return None;
    }
    let tokens = lex(source).ok()?;
    let call = tokens
        .iter()
        .position(|t| matches!(&t.kind, Lexeme::Name(n) if n == &name))?;
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

pub struct Engine<'a> {
    pub workspace: &'a Workspace,
    pub today: NaiveDate,
    pub now: DateTime<FixedOffset>,
    pub time_dependent: bool,
    memo: BTreeMap<Symbol, Result<Value, String>>,
    stack: Vec<Symbol>,
}
impl<'a> Engine<'a> {
    pub fn new(workspace: &'a Workspace, today: NaiveDate) -> Self {
        let mut engine = Self::at(workspace, Local::now().fixed_offset());
        engine.today = today;
        engine
    }
    /// One clock snapshot per evaluation; injectable for deterministic tests.
    pub fn at(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self {
            workspace,
            today: now.with_timezone(&Local).date_naive(),
            now,
            time_dependent: false,
            memo: BTreeMap::new(),
            stack: Vec::new(),
        }
    }
    pub fn eval(&mut self, path: &Path, expression: &str) -> Result<Value, String> {
        self.expr(path, &Parser::parse(expression)?)
    }
    pub fn named(&mut self, path: &Path, name: &str) -> Result<Value, String> {
        let symbol = self.workspace.resolve(path, name)?;
        self.symbol(&symbol)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> Result<Value, String> {
        if let Some(v) = self.memo.get(symbol) {
            return v.clone();
        }
        if self.stack.contains(symbol) {
            return Err(format!(
                "Dependency cycle involving '{}'",
                self.workspace.named(symbol).name
            ));
        }
        if self.stack.len() >= 64 {
            return Err("Dependency chain exceeds 64 levels".into());
        }
        self.stack.push(symbol.clone());
        let doc = &self.workspace.documents[&symbol.path];
        let result = match symbol.kind {
            SymbolKind::Definition(i) => {
                let def = &doc.definitions[i];
                if def.expression {
                    self.eval(&symbol.path, &def.source).map(|v| match v {
                        Value::Timer(mut timer)
                            if timer.origin.is_none() && timer_arguments(&def.source).is_some() =>
                        {
                            timer.origin = Some(symbol.clone());
                            Value::Timer(timer)
                        }
                        other => other,
                    })
                } else {
                    literal(&def.source).map(|v| match v {
                        Value::Resource(mut r) => {
                            r.origin = Some(symbol.path.clone());
                            Value::Resource(r)
                        }
                        other => other,
                    })
                }
            }
            SymbolKind::Task(i) => Ok(Value::Bool(self.task_done(&symbol.path, i))),
            SymbolKind::Section(i) => {
                let section = &doc.sections[i];
                Ok(Value::Tasks(
                    doc.tasks
                        .iter()
                        .enumerate()
                        .filter(|(i, t)| {
                            t.line > section.line
                                && t.line < section.end_line
                                && !doc.tasks.iter().any(|t| t.parent == Some(*i))
                        })
                        .map(|(i, _)| (symbol.path.clone(), i))
                        .collect(),
                ))
            }
        };
        self.stack.pop();
        self.memo.insert(symbol.clone(), result.clone());
        result
    }
    fn expr(&mut self, path: &Path, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Value(v) => Ok(v.clone()),
            Expr::Name(n) => match n.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => self.named(path, n),
            },
            Expr::Call(n, args) => {
                if n == "now" && args.is_empty() {
                    self.time_dependent = true;
                    return Ok(Value::DateTime(self.now));
                }
                if matches!(n.as_str(), "stopwatch" | "countdown") {
                    let values = args
                        .iter()
                        .map(|a| self.expr(path, a))
                        .collect::<Result<Vec<_>, _>>()?;
                    let timer = Timer::new(n, &values, self.now)?;
                    self.time_dependent |= timer.running();
                    return Ok(Value::Timer(timer));
                }
                if n == "today" && args.is_empty() {
                    return Ok(Value::Date(self.today));
                }
                if args.len() != 1 {
                    return Err(format!("{n} expects one argument"));
                }
                let value = self.expr(path, &args[0])?;
                if n == "date"
                    && let Value::Text(s) = value
                {
                    return date_value(&s)
                        .or_else(|| relative_date(&s, self.today).map(Value::Date))
                        .ok_or("Unrecognized date".into());
                }
                let Value::Tasks(tasks) = value else {
                    return Err(format!("{n} expects a named checklist heading"));
                };
                let done = tasks.iter().filter(|(p, i)| self.task_done(p, *i)).count();
                match n.as_str() {
                    "total" => Ok(Value::Count(tasks.len())),
                    "completed" => Ok(Value::Count(done)),
                    "remaining" => Ok(Value::Count(tasks.len() - done)),
                    "effort" => {
                        let mut seconds = 0i64;
                        for (p, i) in tasks {
                            if !self.task_done(&p, i) {
                                let task = &self.workspace.documents[&p].tasks[i];
                                if let Some(attr) = task.attributes.get("estimate") {
                                    let Value::Duration(m) = self.eval(&p, &attr.value)? else {
                                        return Err("@estimate requires a duration".into());
                                    };
                                    if m < 0 {
                                        return Err("Estimate cannot be negative".into());
                                    }
                                    seconds = seconds.checked_add(m).ok_or("Duration overflow")?;
                                }
                            }
                        }
                        Ok(Value::Duration(seconds))
                    }
                    _ => Err(format!("Unknown function '{n}'")),
                }
            }
            Expr::Unary(op, v) => {
                let v = self.expr(path, v)?;
                match (op.as_str(), v) {
                    ("!", Value::Bool(b)) => Ok(Value::Bool(!b)),
                    ("-", Value::Number(n)) => Ok(Value::Number(-n)),
                    ("-", Value::Money(n)) => Ok(Value::Money(-n)),
                    ("-", Value::Duration(n)) => n
                        .checked_neg()
                        .map(Value::Duration)
                        .ok_or("Duration overflow".into()),
                    ("+", v) if v.scalar().is_some() => Ok(v),
                    _ => Err("Invalid unary operation".into()),
                }
            }
            Expr::Binary(op, a, b) => {
                let a = self.expr(path, a)?;
                if op == "&&" && a == Value::Bool(false) {
                    return Ok(a);
                }
                if op == "||" && a == Value::Bool(true) {
                    return Ok(a);
                }
                binary(op, a, self.expr(path, b)?)
            }
            Expr::Property(v, key) => {
                let v = self.expr(path, v)?;
                match v {
                    Value::Timer(timer) => timer.property(key),
                    Value::Resource(resource) => {
                        if key == "url" {
                            return Ok(Value::Text(resource.target));
                        }
                        if key == "exists" {
                            return Ok(Value::Bool(
                                resource
                                    .url(path)?
                                    .to_file_path()
                                    .map(|p| p.exists())
                                    .unwrap_or(false),
                            ));
                        }
                        let m = self
                            .workspace
                            .cache
                            .get(&resource.target)
                            .ok_or("No cached GitHub status; run jot refresh")?;
                        match key.as_str() {
                            "merged" => m
                                .merged
                                .map(Value::Bool)
                                .ok_or("merged is only available on pull requests".into()),
                            "state" => Ok(Value::Text(m.state.clone())),
                            "title" => Ok(Value::Text(m.title.clone())),
                            "checks_passed" => m
                                .checks
                                .as_ref()
                                .map(|c| Value::Bool(c == "passing"))
                                .ok_or("No checks reported".into()),
                            _ => Err(format!("Unknown resource property '{key}'")),
                        }
                    }
                    _ => Err("Only resource and timer values have properties".into()),
                }
            }
        }
    }
    pub fn task_done(&self, path: &Path, i: usize) -> bool {
        let doc = &self.workspace.documents[path];
        let children: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent == Some(i))
            .map(|(j, _)| j)
            .collect();
        if children.is_empty() {
            doc.tasks[i].checked
        } else {
            children.into_iter().all(|j| self.task_done(path, j))
        }
    }
    pub fn blocked(&mut self, path: &Path, i: usize) -> Result<Vec<String>, String> {
        self.blocked_inner(path, i, &mut Vec::new())
    }
    fn blocked_inner(
        &mut self,
        path: &Path,
        i: usize,
        stack: &mut Vec<TaskKey>,
    ) -> Result<Vec<String>, String> {
        let key = (path.to_path_buf(), i);
        if stack.contains(&key) {
            return Err("Task dependency cycle".into());
        }
        if stack.len() > 64 {
            return Err("Task dependency chain is too deep".into());
        }
        stack.push(key);
        let task = &self.workspace.documents[path].tasks[i];
        let mut blocked = Vec::new();
        if let Some(attr) = task.attributes.get("after") {
            for name in attr.value.split(',').map(str::trim) {
                if let Ok(s) = self.workspace.resolve(path, name)
                    && let SymbolKind::Task(j) = s.kind
                {
                    self.blocked_inner(&s.path, j, stack)?;
                }
                let ready = match self.eval(path, name)? {
                    Value::Bool(b) => b,
                    Value::Tasks(ts) => ts.iter().all(|(p, j)| self.task_done(p, *j)),
                    _ => {
                        return Err(
                            "@after requires task names, checklists, or boolean expressions".into(),
                        );
                    }
                };
                if !ready {
                    blocked.push(name.to_string());
                }
            }
        }
        stack.pop();
        Ok(blocked)
    }
    pub fn when(&mut self, path: &Path, source: &str) -> Result<Value, String> {
        if let Some(v) = date_value(source) {
            return Ok(v);
        }
        if let Some(v) = relative_date(source, self.today) {
            return Ok(Value::Date(v));
        }
        let value = self.eval(path, source)?;
        value.date()?;
        Ok(value)
    }
}
fn binary(op: &str, a: Value, b: Value) -> Result<Value, String> {
    use Value::*;
    if matches!(op, "==" | "!=") {
        let equal = a
            .scalar()
            .zip(b.scalar())
            .map(|(a, b)| a == b)
            .unwrap_or(a == b);
        return Ok(Bool(equal == (op == "==")));
    }
    if let (Bool(a), Bool(b)) = (&a, &b) {
        return match op {
            "&&" => Ok(Bool(*a && *b)),
            "||" => Ok(Bool(*a || *b)),
            _ => Err("Invalid boolean operator".into()),
        };
    }
    if matches!(op, "<" | "<=" | ">" | ">=") {
        let cmp = match (&a, &b) {
            (Date(a), Date(b)) => a.partial_cmp(b),
            (DateTime(a), DateTime(b)) => a.partial_cmp(b),
            (Duration(a), Duration(b)) => a.partial_cmp(b),
            (Money(a), Money(b)) => a.partial_cmp(b),
            _ => a
                .scalar()
                .zip(b.scalar())
                .and_then(|(a, b)| a.partial_cmp(&b)),
        }
        .ok_or("Cannot compare these value types")?;
        return Ok(Bool(match op {
            "<" => cmp.is_lt(),
            "<=" => cmp.is_le(),
            ">" => cmp.is_gt(),
            _ => cmp.is_ge(),
        }));
    }
    match (op, &a, &b) {
        ("-", Date(a), Date(b)) => return Ok(Duration((*a - *b).num_seconds())),
        ("+" | "-", Date(a), Duration(m)) => {
            if m % 86400 != 0 {
                return Err(
                    "A date requires whole-day durations; use a date/time for hours".into(),
                );
            }
            let delta = chrono::Duration::try_seconds(*m).ok_or("Duration overflow")?;
            return if op == "+" {
                a.checked_add_signed(delta)
            } else {
                a.checked_sub_signed(delta)
            }
            .map(Date)
            .ok_or("Date overflow".into());
        }
        ("+" | "-", DateTime(a), Duration(m)) => {
            let delta = chrono::Duration::try_seconds(*m).ok_or("Duration overflow")?;
            return if op == "+" {
                a.checked_add_signed(delta)
            } else {
                a.checked_sub_signed(delta)
            }
            .map(DateTime)
            .ok_or("Date/time overflow".into());
        }
        ("-", DateTime(a), DateTime(b)) => return Ok(Duration((*a - *b).num_seconds())),
        ("+" | "-", Duration(a), Duration(b)) => {
            return if op == "+" {
                a.checked_add(*b)
            } else {
                a.checked_sub(*b)
            }
            .map(Duration)
            .ok_or("Duration overflow".into());
        }
        ("/", Duration(a), Duration(b)) => {
            return if *b == 0 {
                Err("Division by zero".into())
            } else {
                Ok(Ratio(*a as f64 / *b as f64))
            };
        }
        ("+", Text(a), Text(b)) => return Ok(Text(format!("{a}{b}"))),
        _ => {}
    }
    let money_a = matches!(a, Money(_));
    let money_b = matches!(b, Money(_));
    let counts = matches!((&a, &b), (Count(_), Count(_)));
    if matches!(op, "*" | "/") {
        let scaled = match (&a, &b) {
            (Duration(m), v) => v.scalar().map(|n| {
                if op == "*" {
                    *m as f64 * n
                } else {
                    *m as f64 / n
                }
            }),
            (v, Duration(m)) if op == "*" => v.scalar().map(|n| *m as f64 * n),
            _ => None,
        };
        if let Some(m) = scaled {
            if !m.is_finite() || m.fract() != 0.0 || m.abs() >= i64::MAX as f64 {
                return Err("Duration must fit in whole seconds".into());
            }
            return Ok(Duration(m as i64));
        }
    }
    let x = if let Money(n) = a {
        Some(n)
    } else {
        a.scalar()
    }
    .ok_or("Unsupported arithmetic types")?;
    let y = if let Money(n) = b {
        Some(n)
    } else {
        b.scalar()
    }
    .ok_or("Unsupported arithmetic types")?;
    let n = match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => {
            if y == 0.0 {
                return Err("Division by zero".into());
            }
            x / y
        }
        _ => return Err(format!("Unknown operator {op}")),
    };
    if !n.is_finite() {
        return Err("Number overflow".into());
    }
    if op == "*" && money_a && money_b {
        return Err("Cannot multiply two money values".into());
    }
    if op == "/" && !money_a && money_b {
        return Err("Cannot divide a scalar by money".into());
    }
    if op == "/" && (money_a && money_b || counts) {
        return Ok(Ratio(n));
    }
    Ok(if money_a || money_b {
        Money(n)
    } else {
        Number(n)
    })
}

/// Repeat from the previous due date, advancing beyond completion; month repeats
/// retain the original day-of-month so Jan 31 -> Feb 28 -> Mar 31.
pub fn next_occurrence(
    rule: &str,
    anchor: NaiveDate,
    completed: NaiveDate,
) -> Result<NaiveDate, String> {
    let month_step = match rule.trim() {
        "month" | "monthly" => Some(1),
        "year" | "yearly" => Some(12),
        _ => None,
    };
    for n in 1u32..=12000 {
        let candidate = if let Some(step) = month_step {
            anchor.checked_add_months(Months::new(n * step))
        } else {
            let days = match rule.trim() {
                "day" | "daily" => 1,
                "week" | "weekly" => 7,
                s => duration(s)
                    .filter(|d| *d > 0 && *d % 86400 == 0)
                    .map(|d| d / 86400)
                    .ok_or(
                        "@every supports day, week, month, year, or positive whole-day durations",
                    )?,
            };
            days.checked_mul(n as i64)
                .and_then(chrono::Duration::try_days)
                .and_then(|d| anchor.checked_add_signed(d))
        };
        let candidate = candidate.ok_or("Recurrence date overflow")?;
        if candidate > completed {
            return Ok(candidate);
        }
    }
    Err("Recurrence exceeded its search limit".into())
}
