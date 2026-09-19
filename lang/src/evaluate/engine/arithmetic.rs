//! Operators: what `+`, `<=` or `&&` mean between two values, and the operator
//! vocabulary the lexer and the parser share.
use super::Value;
use crate::error::{CurrencyOp, EvalError, EvalResult, Limit, Overflow};

/// An operator as written, including the ones only the parser gives meaning to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    /// `!`, a prefix operator only.
    Not,
    /// `=>`, between a lambda's parameters and its body.
    Arrow,
    /// `|`, the query pipeline separator; expressions have no use for it.
    Pipe,
    /// Lexed, but not part of the language: the parser rejects it where it stands.
    Unsupported(&'static str),
}
impl Operator {
    /// The operator a lexeme spells, for the finite set of spellings the lexer
    /// can produce.
    pub(super) fn lex(s: &str) -> Option<Self> {
        Some(match s {
            "+" => Self::Add,
            "-" => Self::Subtract,
            "*" => Self::Multiply,
            "/" => Self::Divide,
            "==" => Self::Equal,
            "!=" => Self::NotEqual,
            "<" => Self::Less,
            "<=" => Self::LessEqual,
            ">" => Self::Greater,
            ">=" => Self::GreaterEqual,
            "&&" => Self::And,
            "||" => Self::Or,
            "!" => Self::Not,
            "=>" => Self::Arrow,
            "|" => Self::Pipe,
            "=" => Self::Unsupported("="),
            "&" => Self::Unsupported("&"),
            "+=" => Self::Unsupported("+="),
            "-=" => Self::Unsupported("-="),
            "*=" => Self::Unsupported("*="),
            "/=" => Self::Unsupported("/="),
            "&=" => Self::Unsupported("&="),
            "|=" => Self::Unsupported("|="),
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            Self::And => "&&",
            Self::Or => "||",
            Self::Not => "!",
            Self::Arrow => "=>",
            Self::Pipe => "|",
            Self::Unsupported(s) => s,
        }
    }
    /// The binary operation this spelling denotes, if any.
    pub fn binary(self) -> Option<BinaryOp> {
        Some(match self {
            Self::Add => BinaryOp::Add,
            Self::Subtract => BinaryOp::Subtract,
            Self::Multiply => BinaryOp::Multiply,
            Self::Divide => BinaryOp::Divide,
            Self::Equal => BinaryOp::Equal,
            Self::NotEqual => BinaryOp::NotEqual,
            Self::Less => BinaryOp::Less,
            Self::LessEqual => BinaryOp::LessEqual,
            Self::Greater => BinaryOp::Greater,
            Self::GreaterEqual => BinaryOp::GreaterEqual,
            Self::And => BinaryOp::And,
            Self::Or => BinaryOp::Or,
            _ => return None,
        })
    }
    /// The prefix operation this spelling denotes, if any.
    pub fn unary(self) -> Option<UnaryOp> {
        Some(match self {
            Self::Subtract => UnaryOp::Negate,
            Self::Add => UnaryOp::Plus,
            Self::Not => UnaryOp::Not,
            _ => return None,
        })
    }
}
impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// An operation between two values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}
impl BinaryOp {
    pub fn as_str(self) -> &'static str {
        self.operator().as_str()
    }
    pub fn operator(self) -> Operator {
        match self {
            Self::Add => Operator::Add,
            Self::Subtract => Operator::Subtract,
            Self::Multiply => Operator::Multiply,
            Self::Divide => Operator::Divide,
            Self::Equal => Operator::Equal,
            Self::NotEqual => Operator::NotEqual,
            Self::Less => Operator::Less,
            Self::LessEqual => Operator::LessEqual,
            Self::Greater => Operator::Greater,
            Self::GreaterEqual => Operator::GreaterEqual,
            Self::And => Operator::And,
            Self::Or => Operator::Or,
        }
    }
    /// Binding power: a higher number binds tighter.
    pub fn precedence(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Equal | Self::NotEqual => 3,
            Self::Less | Self::LessEqual | Self::Greater | Self::GreaterEqual => 4,
            Self::Add | Self::Subtract => 5,
            Self::Multiply | Self::Divide => 6,
        }
    }
}
impl std::fmt::Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// The comparison at the top of a plan constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    LessEqual,
    GreaterEqual,
    Equal,
}
impl Comparison {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LessEqual => "<=",
            Self::GreaterEqual => ">=",
            Self::Equal => "==",
        }
    }
    /// The comparison an operator makes, for the three a constraint allows.
    pub fn from_op(op: BinaryOp) -> Option<Self> {
        match op {
            BinaryOp::LessEqual => Some(Self::LessEqual),
            BinaryOp::GreaterEqual => Some(Self::GreaterEqual),
            BinaryOp::Equal => Some(Self::Equal),
            _ => None,
        }
    }
}
impl std::str::FromStr for Comparison {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "<=" => Ok(Self::LessEqual),
            ">=" => Ok(Self::GreaterEqual),
            "==" => Ok(Self::Equal),
            _ => Err("Linear comparison must be <=, >=, or ==".into()),
        }
    }
}
impl std::fmt::Display for Comparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// A prefix operation on one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Plus,
    Not,
}
impl UnaryOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negate => "-",
            Self::Plus => "+",
            Self::Not => "!",
        }
    }
}
impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
pub(crate) fn binary(op: BinaryOp, a: Value, b: Value) -> EvalResult<Value> {
    use Value::*;
    if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
        let equal = a
            .scalar()
            .zip(b.scalar())
            .map(|(a, b)| a == b)
            .unwrap_or(a == b);
        return Ok(Bool(equal == (op == BinaryOp::Equal)));
    }
    if let (Bool(a), Bool(b)) = (&a, &b)
        && matches!(op, BinaryOp::And | BinaryOp::Or)
    {
        return match op {
            BinaryOp::And => Ok(Bool(*a && *b)),
            BinaryOp::Or => Ok(Bool(*a || *b)),
            _ => Err(EvalError::Message("Invalid boolean operator".into())),
        };
    }
    if matches!(
        op,
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
    ) {
        if a == Null || b == Null {
            return Ok(Bool(false));
        }
        let cmp = match (&a, &b) {
            (Text(a), Text(b)) => a.partial_cmp(b),
            (Bool(a), Bool(b)) => a.partial_cmp(b),
            (Date(a), Date(b)) => a.partial_cmp(b),
            (DateTime(a), DateTime(b)) => a.partial_cmp(b),
            (Duration(a), Duration(b)) => a.partial_cmp(b),
            (Money(a, ca), Money(b, cb)) => {
                if ca != cb {
                    return Err(EvalError::CurrencyMismatch {
                        op: CurrencyOp::Compare,
                        left: *ca,
                        right: *cb,
                    });
                }
                a.partial_cmp(b)
            }
            _ => a
                .scalar()
                .zip(b.scalar())
                .and_then(|(a, b)| a.partial_cmp(&b)),
        }
        .ok_or(EvalError::Message(
            "Cannot compare these value types".into(),
        ))?;
        return Ok(Bool(match op {
            BinaryOp::Less => cmp.is_lt(),
            BinaryOp::LessEqual => cmp.is_le(),
            BinaryOp::Greater => cmp.is_gt(),
            _ => cmp.is_ge(),
        }));
    }
    match (op, &a, &b) {
        (BinaryOp::Subtract, Date(a), Date(b)) => return Ok(Duration((*a - *b).num_seconds())),
        (BinaryOp::Add | BinaryOp::Subtract, Date(a), Duration(m)) => {
            if m % 86400 != 0 {
                return Err(EvalError::Message(
                    "A date requires whole-day durations; use a date/time for hours".into(),
                ));
            }
            let delta = chrono::Duration::try_seconds(*m)
                .ok_or(EvalError::Overflowed(Overflow::Duration))?;
            return if op == BinaryOp::Add {
                a.checked_add_signed(delta)
            } else {
                a.checked_sub_signed(delta)
            }
            .map(Date)
            .ok_or(EvalError::Overflowed(Overflow::Date));
        }
        (BinaryOp::Add | BinaryOp::Subtract, DateTime(a), Duration(m)) => {
            let delta = chrono::Duration::try_seconds(*m)
                .ok_or(EvalError::Overflowed(Overflow::Duration))?;
            return if op == BinaryOp::Add {
                a.checked_add_signed(delta)
            } else {
                a.checked_sub_signed(delta)
            }
            .map(DateTime)
            .ok_or(EvalError::Overflowed(Overflow::DateTime));
        }
        (BinaryOp::Subtract, DateTime(a), DateTime(b)) => {
            return Ok(Duration((*a - *b).num_seconds()));
        }
        (BinaryOp::Add | BinaryOp::Subtract, Duration(a), Duration(b)) => {
            return if op == BinaryOp::Add {
                a.checked_add(*b)
            } else {
                a.checked_sub(*b)
            }
            .map(Duration)
            .ok_or(EvalError::Overflowed(Overflow::Duration));
        }
        (BinaryOp::Divide, Duration(a), Duration(b)) => {
            return if *b == 0 {
                Err(EvalError::DivisionByZero)
            } else {
                Ok(Ratio(*a as f64 / *b as f64))
            };
        }
        (BinaryOp::Add, Text(a), Text(b)) => {
            if a.len().saturating_add(b.len()) > 1_048_576 {
                return Err(EvalError::LimitExceeded(Limit::Text));
            }
            return Ok(Text(format!("{a}{b}")));
        }
        _ => {}
    }
    let currency_a = if let Money(_, c) = &a { Some(*c) } else { None };
    let currency_b = if let Money(_, c) = &b { Some(*c) } else { None };
    if let (Some(ca), Some(cb)) = (currency_a, currency_b)
        && ca != cb
    {
        return Err(EvalError::CurrencyMismatch {
            op: CurrencyOp::Combine,
            left: ca,
            right: cb,
        });
    }
    let money_a = currency_a.is_some();
    let money_b = currency_b.is_some();
    let counts = matches!((&a, &b), (Count(_), Count(_)));
    if matches!(op, BinaryOp::Multiply | BinaryOp::Divide) {
        let scaled = match (&a, &b) {
            (Duration(m), v) => v.scalar().map(|n| {
                if op == BinaryOp::Multiply {
                    *m as f64 * n
                } else {
                    *m as f64 / n
                }
            }),
            (v, Duration(m)) if op == BinaryOp::Multiply => v.scalar().map(|n| *m as f64 * n),
            _ => None,
        };
        if let Some(m) = scaled {
            if !m.is_finite() || m.fract() != 0.0 || m.abs() >= i64::MAX as f64 {
                return Err(EvalError::Message(
                    "Duration must fit in whole seconds".into(),
                ));
            }
            return Ok(Duration(m as i64));
        }
    }
    let x = if let Money(n, _) = a {
        Some(n)
    } else {
        a.scalar()
    }
    .ok_or(EvalError::UnsupportedArithmetic)?;
    let y = if let Money(n, _) = b {
        Some(n)
    } else {
        b.scalar()
    }
    .ok_or(EvalError::UnsupportedArithmetic)?;
    let n = match op {
        BinaryOp::Add => x + y,
        BinaryOp::Subtract => x - y,
        BinaryOp::Multiply => x * y,
        BinaryOp::Divide => {
            if y == 0.0 {
                return Err(EvalError::DivisionByZero);
            }
            x / y
        }
        _ => return Err(EvalError::Message(format!("Unknown operator {op}"))),
    };
    if !n.is_finite() {
        return Err(EvalError::Overflowed(Overflow::Number));
    }
    if op == BinaryOp::Multiply && money_a && money_b {
        return Err(EvalError::Message(
            "Cannot multiply two money values".into(),
        ));
    }
    if op == BinaryOp::Divide && !money_a && money_b {
        return Err(EvalError::Message("Cannot divide a scalar by money".into()));
    }
    if op == BinaryOp::Divide && (money_a && money_b || counts) {
        return Ok(Ratio(n));
    }
    Ok(match currency_a.or(currency_b) {
        Some(currency) => Money(n, currency),
        None => Number(n),
    })
}
