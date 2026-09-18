use crate::{
    document::Span,
    resources::Resource,
    timers::Timer,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, Months, NaiveDate, NaiveDateTime, TimeZone,
    Weekday,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub type TaskKey = (PathBuf, usize);
/// An ISO 4217 code such as USD or EUR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency([u8; 3]);
impl Currency {
    pub const USD: Currency = Currency(*b"USD");
    pub fn parse(code: &str) -> Option<Self> {
        let bytes = code.as_bytes();
        (bytes.len() == 3 && bytes.iter().all(u8::is_ascii_uppercase))
            .then(|| Currency([bytes[0], bytes[1], bytes[2]]))
    }
    pub fn from_symbol(symbol: char) -> Option<Self> {
        Some(match symbol {
            '$' => Self::USD,
            '€' => Currency(*b"EUR"),
            '£' => Currency(*b"GBP"),
            '¥' => Currency(*b"JPY"),
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("???")
    }
    pub fn symbol(&self) -> Option<char> {
        match self.as_str() {
            "USD" => Some('$'),
            "EUR" => Some('€'),
            "GBP" => Some('£'),
            "JPY" => Some('¥'),
            _ => None,
        }
    }
}
impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// A day's weather from a cached lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub high: f64,
    pub low: f64,
    pub summary: String,
    /// Chance of precipitation, 0 to 1, when the source reports it.
    pub precipitation: Option<f64>,
    pub fahrenheit: bool,
}
impl Forecast {
    pub fn display(&self) -> String {
        let unit = if self.fahrenheit { "°F" } else { "°C" };
        let mut s = format!(
            "{}{unit} / {}{unit} · {}",
            decimal(self.high),
            decimal(self.low),
            self.summary
        );
        if let Some(p) = self.precipitation
            && p >= 0.2
        {
            s.push_str(&format!(" · {}% rain", (p * 100.0).round()));
        }
        s
    }
    pub fn property(&self, name: &str) -> Result<Value, String> {
        match name {
            "high" => Ok(Value::Number(self.high)),
            "low" => Ok(Value::Number(self.low)),
            "summary" => Ok(Value::Text(self.summary.clone())),
            "rain" => self
                .precipitation
                .map(Value::Ratio)
                .ok_or("This forecast has no precipitation chance".into()),
            _ => Err(format!("Unknown forecast property '{name}'")),
        }
    }
}
/// A 3–5 letter uppercase name is a code literal (USD, EUR, NVDA), never a
/// reference to a note value.
pub fn is_code(name: &str) -> bool {
    (name.len() == 1 || (3..=5).contains(&name.len()))
        && name.bytes().all(|b| b.is_ascii_uppercase())
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Count(usize),
    Money(f64, Currency),
    Forecast(Forecast),
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
    Table(std::sync::Arc<crate::tables::TableValue>),
    Plan(std::sync::Arc<crate::plans::PlanValue>),
}
impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Number(_) => "Number",
            Self::Count(_) => "Count",
            Self::Money(..) => "Money",
            Self::Forecast(_) => "Forecast",
            Self::Ratio(_) => "Ratio",
            Self::Duration(_) => "Duration",
            Self::Date(_) => "Date",
            Self::DateTime(_) => "DateTime",
            Self::Bool(_) => "Boolean",
            Self::Text(_) => "Text",
            Self::Resource(_) => "Resource",
            Self::Tasks(_) => "Checklist",
            Self::Timer(t) if t.limit.is_some() => "Countdown",
            Self::Timer(_) => "Stopwatch",
            Self::Table(_) => "Table",
            Self::Plan(_) => "Plan",
        }
    }
    /// A round-trippable expression, unlike the human-readable display label.
    pub fn source(&self) -> Option<String> {
        Some(match self {
            Self::Number(n) => n.to_string(),
            Self::Money(n, c) => match c.symbol() {
                Some(symbol) if *n < 0.0 => format!("-{symbol}{}", n.abs()),
                Some(symbol) => format!("{symbol}{n}"),
                None => format!("{n} {c}"),
            },
            Self::Ratio(n) if (n * 100.0).is_finite() => format!("{}%", n * 100.0),
            Self::Duration(n) => format!("{n}s"),
            Self::Date(d) => d.to_string(),
            Self::DateTime(d) => d.to_rfc3339(),
            Self::Bool(b) => b.to_string(),
            Self::Text(s) => serde_json::to_string(s).ok()?,
            _ => return None,
        })
    }
    pub fn display(&self) -> String {
        match self {
            Self::Number(n) => decimal(*n),
            Self::Count(n) => n.to_string(),
            Self::Money(n, c) => money(*n, *c),
            Self::Forecast(f) => f.display(),
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
            Self::Table(t) => format!("{} rows · {} columns", t.rows.len(), t.columns.len()),
            Self::Plan(p) => p.objective.display(),
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
pub fn decimal(n: f64) -> String {
    let s = format!("{n:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
fn money(n: f64, currency: Currency) -> String {
    let s = format!("{:.2}", n.abs());
    let (whole, frac) = s.split_once('.').unwrap();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let cents = if frac == "00" {
        String::new()
    } else {
        format!(".{frac}")
    };
    let sign = if n < 0.0 { "-" } else { "" };
    match currency.symbol() {
        Some(symbol) => format!("{sign}{symbol}{grouped}{cents}"),
        None => format!("{sign}{grouped}{cents} {currency}"),
    }
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
    // Money: a symbol before the number ($3, €450, -£12) or a code after it (700 MXN).
    let (body, code_after) = match s.rsplit_once(' ') {
        Some((body, code)) if Currency::parse(code).is_some() => (body, Currency::parse(code)),
        _ => (s, None),
    };
    let (sign, unsigned) = match body.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", body),
    };
    let symbol = unsigned.chars().next().and_then(Currency::from_symbol);
    let currency = symbol.or(code_after);
    let digits = match symbol {
        Some(_) => &unsigned[unsigned.chars().next().unwrap().len_utf8()..],
        None => unsigned,
    };
    let num = format!("{sign}{}", digits.replace([',', '%'], ""));
    if let Ok(n) = num.parse::<f64>() {
        if !n.is_finite() {
            return Err("Number must be finite".into());
        }
        return Ok(if let Some(currency) = currency {
            Value::Money(n, currency)
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
pub(crate) enum Expr {
    Spanned(usize, usize, Box<Expr>),
    Value(Value),
    Name(String),
    Call(String, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    Property(Box<Expr>, String),
}
impl Expr {
    fn bare(&self) -> &Self {
        if let Self::Spanned(_, _, e) = self {
            e.bare()
        } else {
            self
        }
    }
    fn bounds(&self) -> (usize, usize) {
        if let Self::Spanned(s, e, _) = self {
            (*s, *e)
        } else {
            (0, 0)
        }
    }
}
pub(crate) struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

pub fn simple_name(source: &str) -> Option<String> {
    let parsed = Parser::parse(source).ok()?;
    if let Expr::Name(name) = parsed.bare() {
        Some(name.clone())
    } else {
        None
    }
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
    pub(crate) fn parse(s: &str) -> Result<Expr, String> {
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
        lhs = Expr::Spanned(start, self.tokens[self.at - 1].end, Box::new(lhs));
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
            lhs = Expr::Spanned(
                start,
                rhs.bounds().1,
                Box::new(Expr::Binary(op, Box::new(lhs), Box::new(rhs))),
            );
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
    let parsed = Parser::parse(source).ok()?;
    let Expr::Call(name, _) = parsed.bare() else {
        return None;
    };
    if !matches!(name.as_str(), "stopwatch" | "countdown") {
        return None;
    }
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

#[derive(Clone, Debug)]
pub struct EvalFailure {
    pub path: PathBuf,
    pub span: Span,
    pub message: String,
    pub related: Vec<Symbol>,
}
/// `terms · variables + constant`, carrying a unit so money and durations
/// never mix silently.
#[derive(Clone, Debug, PartialEq)]
pub struct Linear {
    pub terms: BTreeMap<String, f64>,
    pub constant: f64,
    /// Set when `kind` is Money, so euros and dollars never add up silently.
    pub currency: Option<Currency>,
    /// Unit of the whole form; "Any" while it is only bare variables.
    pub kind: &'static str,
    /// Unit the variables were multiplied by, so a goal seek can tell whether
    /// its unknown is money, a duration, or a plain number.
    pub scale: &'static str,
}
impl Linear {
    fn constant(kind: &'static str, value: f64) -> Self {
        Self {
            terms: BTreeMap::new(),
            constant: value,
            currency: None,
            kind,
            scale: "Number",
        }
    }
    fn variable(name: &str) -> Self {
        Self {
            terms: [(name.to_string(), 1.0)].into(),
            constant: 0.0,
            currency: None,
            kind: "Any",
            scale: "Number",
        }
    }
    fn is_zero(&self) -> bool {
        self.constant == 0.0 && self.terms.values().all(|c| *c == 0.0)
    }
    /// The shared unit of two forms, treating a bare zero as unitless.
    fn combined(a: &Self, b: &Self) -> Option<&'static str> {
        if a.kind == b.kind {
            if a.kind == "Money" && a.currency != b.currency && !a.is_zero() && !b.is_zero() {
                return None;
            }
            Some(a.kind)
        } else if a.is_zero() || a.kind == "Any" {
            Some(b.kind)
        } else if b.is_zero() || b.kind == "Any" {
            Some(a.kind)
        } else {
            None
        }
    }
    pub fn minus(&self, other: &Self) -> Result<Self, String> {
        self.add(other, -1.0)
    }
    /// The unit of a variable in this form, or `None` when it is scaled by
    /// two different units.
    pub fn unknown_kind(&self) -> Option<&'static str> {
        match (self.kind, self.scale) {
            ("Any", _) => Some("Number"),
            (kind, "Number") => Some(kind),
            (kind, scale) if kind == scale => Some("Number"),
            _ => None,
        }
    }
    fn scaled(mut self, factor: f64) -> Self {
        for c in self.terms.values_mut() {
            *c *= factor;
        }
        self.constant *= factor;
        self
    }
    fn add(&self, other: &Self, sign: f64) -> Result<Self, String> {
        let kind = Self::combined(self, other).ok_or_else(|| {
            if let (Some(a), Some(b)) = (self.currency, other.currency) {
                format!("Cannot add {a} and {b}; convert with to(value, {b})")
            } else {
                format!("Cannot add {} and {}", self.kind, other.kind)
            }
        })?;
        let mut result = self.clone();
        result.kind = kind;
        result.currency = self.currency.or(other.currency);
        if !self.terms.is_empty() && !other.terms.is_empty() && self.scale != other.scale {
            return Err(format!(
                "Cannot add terms scaled by {} and {}",
                self.scale, other.scale
            ));
        }
        if self.terms.is_empty() {
            result.scale = other.scale;
        }
        for (name, c) in &other.terms {
            *result.terms.entry(name.clone()).or_insert(0.0) += sign * c;
        }
        result.constant += sign * other.constant;
        Ok(result)
    }
    fn multiply(&self, other: &Self) -> Result<Self, String> {
        let (form, factor) = if other.terms.is_empty() {
            (self, other)
        } else if self.terms.is_empty() {
            (other, self)
        } else {
            return Err("Plans must stay linear: multiply variables by constants only".into());
        };
        let kind = match (form.kind, factor.kind) {
            (k, "Number") | ("Number", k) => k,
            ("Any", k) => k,
            (a, b) => return Err(format!("Cannot multiply {a} by {b}")),
        };
        let mut result = form.clone().scaled(factor.constant);
        result.kind = kind;
        result.currency = form.currency.or(factor.currency);
        if factor.kind != "Number" && !form.terms.is_empty() {
            if form.scale != "Number" {
                return Err(format!("Cannot multiply {} by {}", form.scale, factor.kind));
            }
            result.scale = factor.kind;
        }
        Ok(result)
    }
    fn divide(&self, other: &Self) -> Result<Self, String> {
        if !other.terms.is_empty() {
            return Err("Plans must stay linear: divide by constants only".into());
        }
        if other.constant == 0.0 {
            return Err("Division by zero".into());
        }
        let kind = match (self.kind, other.kind) {
            (k, "Number") => k,
            (a, b) if a == b => "Number",
            (a, b) => return Err(format!("Cannot divide {a} by {b}")),
        };
        let mut result = self.clone().scaled(1.0 / other.constant);
        result.kind = kind;
        if kind != "Money" {
            result.currency = None;
        }
        if other.kind != "Number" && !self.terms.is_empty() {
            result.scale = if self.scale == other.kind {
                "Number"
            } else {
                return Err(format!("Cannot divide {} by {}", self.scale, other.kind));
            };
        }
        Ok(result)
    }
}
pub struct Engine<'a> {
    pub workspace: &'a Workspace,
    pub today: NaiveDate,
    pub now: DateTime<FixedOffset>,
    pub time_dependent: bool,
    pub failure: Option<EvalFailure>,
    contexts: Vec<(PathBuf, Span)>,
    memo: BTreeMap<Symbol, Result<Value, String>>,
    stack: Vec<Symbol>,
    row_values: Vec<RowScope>,
    /// Decision-column variables met while linearizing a plan.
    pub row_variables: Vec<RowVariable>,
    /// Lookup keys read during evaluation, hit or miss, for hovers and refresh.
    pub wanted: Vec<String>,
    /// Definitions being walked symbolically, separate from the value stack:
    /// a goal seek is legitimately on both at once.
    linear_stack: Vec<Symbol>,
    steps: usize,
}
/// Column values for one table row while a `sum` row expression runs.
struct RowScope {
    table: String,
    values: BTreeMap<String, Value>,
    /// Decision columns, mapped to the per-row variable name a plan uses.
    decisions: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RowVariable {
    pub name: String,
    pub table: Symbol,
    pub column: usize,
    pub row: usize,
    pub domain: crate::tables::Domain,
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
            failure: None,
            contexts: Vec::new(),
            memo: BTreeMap::new(),
            stack: Vec::new(),
            row_values: Vec::new(),
            row_variables: Vec::new(),
            wanted: Vec::new(),
            linear_stack: Vec::new(),
            steps: 0,
        }
    }
    pub fn eval(&mut self, path: &Path, expression: &str) -> Result<Value, String> {
        self.eval_at(path, expression, Span::new(0, 0, expression.len()))
    }
    pub fn eval_at(&mut self, path: &Path, expression: &str, span: Span) -> Result<Value, String> {
        if self.contexts.is_empty() && self.stack.is_empty() && self.row_values.is_empty() {
            self.steps = 0;
        }
        self.contexts.push((path.into(), span));
        let result = match Parser::parse(expression) {
            Ok(expr) => self.expr(path, &expr),
            Err(message) => {
                let tokens = lex(expression).unwrap_or_default();
                let bounds = tokens
                    .last()
                    .map(|t| (t.start, t.end))
                    .unwrap_or((0, expression.len()));
                self.fail(bounds, &message);
                Err(message)
            }
        };
        self.contexts.pop();
        result
    }
    fn fail(&mut self, bounds: (usize, usize), message: &str) {
        if self.failure.is_none()
            && let Some((path, base)) = self.contexts.last()
        {
            self.failure = Some(EvalFailure {
                path: path.clone(),
                span: Span::new(base.line, base.start + bounds.0, base.start + bounds.1),
                message: message.into(),
                related: vec![],
            });
        }
    }
    pub fn valid_expression(source: &str) -> bool {
        Parser::parse(source).is_ok()
    }
    pub fn is_subexpression(source: &str, start: usize, end: usize) -> bool {
        fn contains(expr: &Expr, start: usize, end: usize) -> bool {
            if expr.bounds() == (start, end) {
                return true;
            }
            match expr {
                Expr::Spanned(_, _, inner) => contains(inner, start, end),
                Expr::Call(_, args) => args.iter().any(|e| contains(e, start, end)),
                Expr::Unary(_, e) | Expr::Property(e, _) => contains(e, start, end),
                Expr::Binary(_, a, b) => contains(a, start, end) || contains(b, start, end),
                _ => false,
            }
        }
        Parser::parse(source).is_ok_and(|e| contains(&e, start, end))
    }
    /// Return a substitution trace without re-evaluating side effects (evaluation is pure).
    pub fn substituted(&mut self, path: &Path, source: &str) -> Result<String, String> {
        let tokens = lex(source)?;
        let mut edits = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            if let Lexeme::Name(name) = &token.kind
                && !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left))
                && (i == 0 || !matches!(tokens[i - 1].kind, Lexeme::Dot))
                && !matches!(name.as_str(), "true" | "false")
                && sum_scope_at(source, token.start).is_none()
                && let Ok(value) = self.named(path, name)
            {
                if matches!(value, Value::Table(_)) {
                    continue;
                }
                // For properties substitute the complete access, not a timer's display text.
                let end = if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Dot)) {
                    tokens.get(i + 2).map(|t| t.end).unwrap_or(token.end)
                } else {
                    token.end
                };
                let value = if end > token.end {
                    self.eval(path, &source[token.start..end])?
                } else {
                    value
                };
                edits.push((token.start, end, value.display()));
            }
        }
        let mut result = source.to_string();
        for (start, end, value) in edits.into_iter().rev() {
            result.replace_range(start..end, &value);
        }
        Ok(result)
    }
    pub fn named(&mut self, path: &Path, name: &str) -> Result<Value, String> {
        let symbol = self.workspace.resolve(path, name)?;
        self.symbol(&symbol)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> Result<Value, String> {
        if self.contexts.is_empty() && self.stack.is_empty() && self.row_values.is_empty() {
            self.steps = 0;
        }
        if let Some(v) = self.memo.get(symbol) {
            return v.clone();
        }
        if let Some(start) = self.stack.iter().position(|s| s == symbol) {
            let mut related = self.stack[start..].to_vec();
            related.push(symbol.clone());
            let message = format!(
                "Dependency cycle: {}",
                related
                    .iter()
                    .map(|s| self.workspace.named(s).name.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ")
            );
            self.failure = Some(EvalFailure {
                path: symbol.path.clone(),
                span: self.workspace.named(symbol).span,
                message: message.clone(),
                related,
            });
            return Err(message);
        }
        if self.stack.len() >= 64 {
            return Err("Dependency chain exceeds 64 levels".into());
        }
        self.stack.push(symbol.clone());
        // Named definitions never capture a caller's row locals.
        let caller_rows = std::mem::take(&mut self.row_values);
        let doc = &self.workspace.documents[&symbol.path];
        let result = match symbol.kind {
            SymbolKind::Definition(i) => {
                let def = &doc.definitions[i];
                if let Some(plan) = doc.plans.iter().find(|p| p.definition == i) {
                    crate::plans::solve(self, symbol, plan)
                } else if def.expression && crate::plans::seek_body(&def.source).is_some() {
                    crate::plans::seek(self, symbol)
                } else if let Some(table) = doc.tables.iter().find(|t| t.definition == i) {
                    if let Some(problem) = table.problems.first() {
                        self.failure = Some(EvalFailure {
                            path: symbol.path.clone(),
                            span: problem.span,
                            message: problem.message.clone(),
                            related: vec![],
                        });
                        Err(problem.message.clone())
                    } else {
                        self.table_value(symbol, table)
                    }
                } else if def.expression {
                    let raw =
                        &doc.line(def.value_span.line)[def.value_span.start..def.value_span.end];
                    let offset = raw.len() - raw.trim_start().len();
                    self.eval_at(
                        &symbol.path,
                        &def.source,
                        Span::new(
                            def.value_span.line,
                            def.value_span.start + offset,
                            def.value_span.end,
                        ),
                    )
                    .map(|v| match v {
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
            SymbolKind::Column(_, _) => {
                Err("A column needs a row context, e.g. sum(table, column)".into())
            }
            SymbolKind::Variable(plan, name) => {
                let name = doc.plans[plan].names[name].name.clone();
                let definition = Symbol {
                    path: symbol.path.clone(),
                    kind: SymbolKind::Definition(doc.plans[plan].definition),
                };
                match self.symbol(&definition)? {
                    Value::Plan(p) => p.property(&name),
                    _ => Err("Expected a plan".into()),
                }
            }
        };
        self.row_values = caller_rows;
        self.stack.pop();
        self.memo.insert(symbol.clone(), result.clone());
        result
    }
    /// A linear form over `vars`; every other name is evaluated to a constant.
    pub fn linear(
        &mut self,
        path: &Path,
        source: &str,
        span: Span,
        vars: &BTreeSet<String>,
    ) -> Result<Linear, String> {
        self.contexts.push((path.into(), span));
        let result = match Parser::parse(source) {
            Ok(expr) => self.linear_expr(path, &expr, vars),
            Err(message) => {
                self.fail((0, source.len()), &message);
                Err(message)
            }
        };
        self.contexts.pop();
        result
    }
    /// `lhs <= rhs`, `lhs >= rhs`, or `lhs == rhs` as two linear forms.
    pub fn constraint(
        &mut self,
        path: &Path,
        source: &str,
        span: Span,
        vars: &BTreeSet<String>,
    ) -> Result<(Linear, String, Linear), String> {
        self.contexts.push((path.into(), span));
        let result = (|| {
            let expr = Parser::parse(source).inspect_err(|m| self.fail((0, source.len()), m))?;
            let Expr::Binary(op, lhs, rhs) = expr.bare() else {
                let message =
                    "A constraint compares two sides with <=, >=, or ==, e.g. bagels >= 12";
                self.fail((0, source.len()), message);
                return Err(message.into());
            };
            if !matches!(op.as_str(), "<=" | ">=" | "==") {
                let message = format!("Constraints use <=, >=, or ==, not {op}");
                self.fail((0, source.len()), &message);
                return Err(message);
            }
            let lhs = self.linear_expr(path, lhs, vars)?;
            let rhs = self.linear_expr(path, rhs, vars)?;
            if Linear::combined(&lhs, &rhs).is_none() {
                let message = format!("Cannot compare {} with {}", lhs.kind, rhs.kind);
                self.fail((0, source.len()), &message);
                return Err(message);
            }
            Ok((lhs, op.clone(), rhs))
        })();
        self.contexts.pop();
        result
    }
    /// An ordinary calculation's source, for symbolic descent. Tables, plans,
    /// goal seeks and literals are opaque and evaluate to constants instead.
    fn definition_source(&self, path: &Path, name: &str) -> Option<(Symbol, String, Span)> {
        let symbol = self.workspace.resolve(path, name).ok()?;
        let SymbolKind::Definition(i) = symbol.kind else {
            return None;
        };
        let doc = &self.workspace.documents[&symbol.path];
        let def = &doc.definitions[i];
        if !def.expression
            || doc.tables.iter().any(|t| t.definition == i)
            || doc.plans.iter().any(|p| p.definition == i)
            || crate::plans::seek_body(&def.source).is_some()
            || crate::plans::goal(&def.source).is_some()
        {
            return None;
        }
        let raw = &doc.line(def.value_span.line)[def.value_span.start..def.value_span.end];
        let offset = raw.len() - raw.trim_start().len();
        Some((
            symbol.clone(),
            def.source.clone(),
            Span::new(
                def.value_span.line,
                def.value_span.start + offset,
                def.value_span.end,
            ),
        ))
    }
    fn linear_expr(
        &mut self,
        path: &Path,
        expr: &Expr,
        vars: &BTreeSet<String>,
    ) -> Result<Linear, String> {
        let constant = |value: Value| -> Result<Linear, String> {
            match value {
                Value::Number(n) | Value::Ratio(n) => Ok(Linear::constant("Number", n)),
                Value::Count(n) => Ok(Linear::constant("Number", n as f64)),
                Value::Money(n, currency) => {
                    let mut form = Linear::constant("Money", n);
                    form.currency = Some(currency);
                    Ok(form)
                }
                Value::Duration(s) => Ok(Linear::constant("Duration", s as f64)),
                other => Err(format!(
                    "Plans work with numbers, money, and durations, not {}",
                    other.type_name()
                )),
            }
        };
        match expr {
            Expr::Spanned(start, end, inner) => {
                let result = self.linear_expr(path, inner, vars);
                if let Err(message) = &result {
                    self.fail((*start, *end), message);
                }
                result
            }
            Expr::Name(n) if vars.contains(n) && self.row_values.is_empty() => {
                Ok(Linear::variable(n))
            }
            Expr::Name(n)
                if self
                    .row_values
                    .last()
                    .is_some_and(|scope| scope.decisions.contains_key(n)) =>
            {
                let variable = self.row_values.last().unwrap().decisions[n].clone();
                if variable.is_empty() {
                    return Err(format!("'{n}' is a decision column; use it inside a plan"));
                }
                Ok(Linear::variable(&variable))
            }
            // Walk into calculations symbolically, so a goal seek can see its
            // own name through any chain of definitions.
            Expr::Name(n)
                if self.row_values.is_empty()
                    && !matches!(n.as_str(), "true" | "false")
                    && self.definition_source(path, n).is_some() =>
            {
                let (symbol, source, span) = self.definition_source(path, n).unwrap();
                if self.linear_stack.contains(&symbol) {
                    return Err(format!("Dependency cycle through {n}"));
                }
                self.linear_stack.push(symbol.clone());
                let result = self.linear(&symbol.path, &source, span, vars);
                self.linear_stack.pop();
                result
            }
            Expr::Call(n, args) if n == "sum" && args.len() == 2 => {
                self.linear_sum(path, args, vars)
            }
            Expr::Unary(op, inner) => {
                let form = self.linear_expr(path, inner, vars)?;
                match op.as_str() {
                    "-" => Ok(form.scaled(-1.0)),
                    "+" => Ok(form),
                    _ => Err("Plans cannot negate booleans".into()),
                }
            }
            Expr::Binary(op, a, b) if matches!(op.as_str(), "+" | "-" | "*" | "/") => {
                let a = self.linear_expr(path, a, vars)?;
                let b = self.linear_expr(path, b, vars)?;
                match op.as_str() {
                    "+" => a.add(&b, 1.0),
                    "-" => a.add(&b, -1.0),
                    "*" => a.multiply(&b),
                    _ => a.divide(&b),
                }
            }
            Expr::Binary(op, _, _) => Err(format!(
                "'{op}' belongs at the top of a constraint, not inside an expression"
            )),
            other => constant(self.expr(path, other)?),
        }
    }
    fn expr(&mut self, path: &Path, expr: &Expr) -> Result<Value, String> {
        self.steps += 1;
        if self.steps > 200_000 {
            let message = "Evaluation exceeds 200,000 steps; simplify nested row calculations";
            self.fail(expr.bounds(), message);
            return Err(message.into());
        }
        match expr {
            Expr::Spanned(start, end, expr) => {
                let result = self.expr(path, expr);
                if let Err(message) = &result {
                    self.fail((*start, *end), message);
                }
                result
            }
            Expr::Value(v) => Ok(v.clone()),
            Expr::Name(n) => match n.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                code if is_code(code) => Ok(Value::Text(code.to_string())),
                _ => {
                    if let Some(scope) = self.row_values.last() {
                        if scope.decisions.contains_key(n) {
                            return Err(format!(
                                "'{n}' is a decision column; a plan chooses it, so sum over it inside maximize or minimize"
                            ));
                        }
                        scope.values.get(n).cloned().ok_or_else(|| {
                            format!("Unknown column '{n}' in table '{}'", scope.table)
                        })
                    } else {
                        self.named(path, n)
                    }
                }
            },
            Expr::Call(n, args) => {
                if n == "sum" {
                    return self.sum(path, args).map(|(value, _)| value);
                }
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
                if matches!(n.as_str(), "rate" | "to" | "forecast" | "quote") {
                    return self.lookup(path, n, args);
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
                    ("-", Value::Money(n, c)) => Ok(Value::Money(-n, c)),
                    ("-", Value::Ratio(n)) => Ok(Value::Ratio(-n)),
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
                let right = self.expr(path, b)?;
                let types = format!("{} {op} {}", a.type_name(), right.type_name());
                binary(op, a, right).map_err(|message| {
                    let message = format!("{message} ({types})");
                    self.fail(b.bounds(), &message);
                    message
                })
            }
            Expr::Property(v, key) => {
                let v = self.expr(path, v)?;
                match v {
                    Value::Timer(timer) => timer.property(key),
                    Value::Forecast(forecast) => forecast.property(key),
                    Value::Plan(plan) => plan.property(key),
                    Value::Resource(resource) => {
                        if key == "url" {
                            return Ok(Value::Text(resource.target));
                        }
                        if key == "exists" {
                            #[cfg(target_arch = "wasm32")]
                            return Err("Local file existence is unavailable in the browser".into());
                            #[cfg(not(target_arch = "wasm32"))]
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
                            .ok_or("No cached GitHub status; run wtf refresh")?;
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

    fn sum(&mut self, path: &Path, args: &[Expr]) -> Result<(Value, Vec<Value>), String> {
        if args.len() != 2 {
            return Err(
                "sum expects a table and a row expression: sum(groceries, quantity * price)".into(),
            );
        }
        let Expr::Name(name) = args[0].bare() else {
            return Err("The first argument to sum must be a table name".into());
        };
        let Value::Table(table) = self.named(path, name)? else {
            return Err(format!("'{name}' is not a table"));
        };
        let mut total = None;
        let mut contributions = Vec::new();
        let decisions = self.decision_columns(&table);
        for row in &table.rows {
            self.row_values.push(RowScope {
                table: name.clone(),
                values: table
                    .columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect(),
                decisions: decisions
                    .keys()
                    .map(|c| (c.clone(), String::new()))
                    .collect(),
            });
            let value = self.expr(path, &args[1]);
            self.row_values.pop();
            let value = value?;
            if !matches!(
                value,
                Value::Number(_) | Value::Money(..) | Value::Ratio(_) | Value::Duration(_)
            ) {
                return Err(format!(
                    "sum requires numeric, money, ratio, or duration results, found {}",
                    value.type_name()
                ));
            }
            total = Some(if let Some(previous) = total {
                let ratios =
                    matches!(previous, Value::Ratio(_)) && matches!(value, Value::Ratio(_));
                let added = binary("+", previous, value.clone())?;
                if ratios && let Value::Number(n) = added {
                    Value::Ratio(n)
                } else {
                    added
                }
            } else {
                value.clone()
            });
            contributions.push(value);
        }
        total.map(|v| (v, contributions)).ok_or_else(|| {
            "Cannot sum an empty table: add a row to establish its value type".into()
        })
    }

    /// Rows of a table, evaluating calculated cells and checking that each
    /// column keeps one type. Failures point at the offending cell.
    fn table_value(
        &mut self,
        symbol: &Symbol,
        table: &crate::tables::Table,
    ) -> Result<Value, String> {
        let mut types: Vec<Option<&'static str>> = table.types.clone();
        let mut rows = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            let mut values = Vec::with_capacity(row.len());
            for (column, cell) in row.iter().enumerate() {
                let value = match &cell.expression {
                    Some((inner, span)) => {
                        let value = self.eval_at(&symbol.path, inner, *span)?;
                        if matches!(
                            value,
                            Value::Table(_) | Value::Plan(_) | Value::Tasks(_) | Value::Timer(_)
                        ) {
                            let message = format!(
                                "A cell cannot hold a {}; use a scalar value",
                                value.type_name()
                            );
                            self.failure.get_or_insert(EvalFailure {
                                path: symbol.path.clone(),
                                span: *span,
                                message: message.clone(),
                                related: vec![],
                            });
                            return Err(message);
                        }
                        if let Some(expected) = types.get(column).copied().flatten() {
                            if expected != value.type_name() {
                                let message = format!(
                                    "Column '{}' expects {expected}, found {}",
                                    table.columns[column].name,
                                    value.type_name()
                                );
                                self.failure.get_or_insert(EvalFailure {
                                    path: symbol.path.clone(),
                                    span: *span,
                                    message: message.clone(),
                                    related: vec![],
                                });
                                return Err(message);
                            }
                        } else if let Some(slot) = types.get_mut(column) {
                            *slot = Some(value.type_name());
                        }
                        value
                    }
                    None => cell.value.clone().map_err(|e| e.to_string())?,
                };
                values.push(value);
            }
            rows.push(values);
        }
        Ok(Value::Table(std::sync::Arc::new(
            crate::tables::TableValue {
                origin: symbol.clone(),
                columns: table.columns.iter().map(|c| c.name.clone()).collect(),
                rows,
            },
        )))
    }
    /// `rate(EUR, USD)`, `to(money, USD)`, `forecast("Oaxaca", 2026-11-20[, F])`
    /// and `quote(NVDA)`: values from the lookup cache, never fetched here.
    fn lookup(&mut self, path: &Path, name: &str, args: &[Expr]) -> Result<Value, String> {
        let code = |value: Value, what: &str| match value {
            Value::Text(code) => Ok(code),
            other => Err(format!(
                "{what} must be a code such as USD, found {}",
                other.type_name()
            )),
        };
        let currency = |code: &str| {
            Currency::parse(code)
                .ok_or_else(|| format!("'{code}' is not a currency code such as USD"))
        };
        match name {
            "rate" => {
                if args.len() != 2 {
                    return Err("rate expects two currency codes: rate(EUR, USD)".into());
                }
                let from = currency(&code(self.expr(path, &args[0])?, "The first currency")?)?;
                let to = currency(&code(self.expr(path, &args[1])?, "The second currency")?)?;
                if from != to {
                    self.wanted.push(crate::lookups::rate_key(from, to));
                }
                crate::lookups::rate(&self.workspace.lookups, from, to).map(Value::Number)
            }
            "to" => {
                if args.len() != 2 {
                    return Err(
                        "to expects a money value and a currency code: to(hotel, USD)".into(),
                    );
                }
                let Value::Money(amount, from) = self.expr(path, &args[0])? else {
                    return Err("to converts money; the first argument is not money".into());
                };
                let to = currency(&code(self.expr(path, &args[1])?, "The currency")?)?;
                if from != to {
                    self.wanted.push(crate::lookups::rate_key(from, to));
                }
                let rate = crate::lookups::rate(&self.workspace.lookups, from, to)?;
                Ok(Value::Money(amount * rate, to))
            }
            "quote" => {
                if args.len() != 1 {
                    return Err("quote expects a ticker symbol: quote(NVDA)".into());
                }
                let symbol = code(self.expr(path, &args[0])?, "The ticker")?;
                self.wanted.push(crate::lookups::quote_key(&symbol));
                crate::lookups::quote(&self.workspace.lookups, &symbol)
            }
            _ => {
                if !(2..=3).contains(&args.len()) {
                    return Err(
                        "forecast expects a place and a date: forecast(\"Oaxaca\", 2026-11-20)"
                            .into(),
                    );
                }
                let Value::Text(place) = self.expr(path, &args[0])? else {
                    return Err(
                        "The place must be text, e.g. forecast(\"Oaxaca\", 2026-11-20)".into(),
                    );
                };
                let date = self.expr(path, &args[1])?.date()?;
                let fahrenheit = match args.get(2) {
                    Some(unit) => match code(self.expr(path, unit)?, "The unit")?.as_str() {
                        "F" | "FAHRENHEIT" => true,
                        "C" | "CELSIUS" => false,
                        other => {
                            return Err(format!("Unknown temperature unit '{other}'; use F or C"));
                        }
                    },
                    None => false,
                };
                self.wanted.push(crate::lookups::forecast_key(&place, date));
                crate::lookups::forecast(&self.workspace.lookups, &place, date, fahrenheit)
                    .map(Value::Forecast)
            }
        }
    }
    /// Decision columns of a table value: column name to (index, domain).
    fn decision_columns(
        &self,
        table: &crate::tables::TableValue,
    ) -> BTreeMap<String, (usize, crate::tables::Domain)> {
        crate::tables::table(self.workspace, &table.origin)
            .map(|t| {
                t.domains
                    .iter()
                    .enumerate()
                    .filter_map(|(i, d)| d.map(|d| (t.columns[i].name.clone(), (i, d))))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// `sum(table, row expression)` as a linear form: decision columns become
    /// one variable per row, other columns are constants.
    fn linear_sum(
        &mut self,
        path: &Path,
        args: &[Expr],
        vars: &BTreeSet<String>,
    ) -> Result<Linear, String> {
        let Some(Expr::Name(name)) = args.first().map(Expr::bare) else {
            return Err("The first argument to sum must be a table name".into());
        };
        let Value::Table(table) = self.named(path, name)? else {
            return Err(format!("'{name}' is not a table"));
        };
        let decisions = self.decision_columns(&table);
        let mut total = Linear::constant("Any", 0.0);
        for (index, row) in table.rows.iter().enumerate() {
            let mut names = BTreeMap::new();
            for (column, (c, domain)) in &decisions {
                let variable = format!("{name}.{column}[{}]", index + 1);
                if !self.row_variables.iter().any(|v| v.name == variable) {
                    self.row_variables.push(RowVariable {
                        name: variable.clone(),
                        table: table.origin.clone(),
                        column: *c,
                        row: index,
                        domain: *domain,
                    });
                }
                names.insert(column.clone(), variable);
            }
            self.row_values.push(RowScope {
                table: name.clone(),
                values: table
                    .columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect(),
                decisions: names,
            });
            let form = self.linear_expr(path, &args[1], vars);
            self.row_values.pop();
            total = total.add(&form?, 1.0)?;
        }
        Ok(total)
    }
    pub fn sum_contributions(&mut self, path: &Path, source: &str) -> Option<Vec<Value>> {
        let parsed = Parser::parse(source).ok()?;
        let Expr::Call(name, args) = parsed.bare() else {
            return None;
        };
        (name == "sum")
            .then(|| self.sum(path, args).ok().map(|(_, rows)| rows))
            .flatten()
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
        if let Some(start) = stack.iter().position(|k| k == &key) {
            let related: Vec<_> = stack[start..]
                .iter()
                .chain(std::iter::once(&key))
                .filter(|(p, index)| self.workspace.documents[p].tasks[*index].named.is_some())
                .map(|(p, index)| Symbol {
                    path: p.clone(),
                    kind: SymbolKind::Task(*index),
                })
                .collect();
            let names = related
                .iter()
                .map(|s| self.workspace.named(s).name.as_str())
                .collect::<Vec<_>>();
            let message = format!("Task dependency cycle: {}", names.join(" → "));
            let task = &self.workspace.documents[path].tasks[i];
            self.failure = Some(EvalFailure {
                path: path.into(),
                span: task
                    .attributes
                    .get("after")
                    .map(|a| a.value_span)
                    .unwrap_or(task.checkbox),
                message: message.clone(),
                related,
            });
            return Err(message);
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
pub(crate) fn binary(op: &str, a: Value, b: Value) -> Result<Value, String> {
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
            (Money(a, ca), Money(b, cb)) => {
                if ca != cb {
                    return Err(format!(
                        "Cannot compare {ca} with {cb}; convert with to(value, {cb})"
                    ));
                }
                a.partial_cmp(b)
            }
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
    let currency_a = if let Money(_, c) = &a { Some(*c) } else { None };
    let currency_b = if let Money(_, c) = &b { Some(*c) } else { None };
    if let (Some(ca), Some(cb)) = (currency_a, currency_b)
        && ca != cb
    {
        return Err(format!(
            "Cannot combine {ca} and {cb}; convert with to(value, {cb})"
        ));
    }
    let money_a = currency_a.is_some();
    let money_b = currency_b.is_some();
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
    let x = if let Money(n, _) = a {
        Some(n)
    } else {
        a.scalar()
    }
    .ok_or("Unsupported arithmetic types")?;
    let y = if let Money(n, _) = b {
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
    Ok(match currency_a.or(currency_b) {
        Some(currency) => Money(n, currency),
        None => Number(n),
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
