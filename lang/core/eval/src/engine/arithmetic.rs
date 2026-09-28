//! What `+`, `<=` or `&&` compute between two values. The operator vocabulary
//! itself, shared with the lexer and parser, lives in `syntax::operators`.
use super::Value;
use crate::error::{CurrencyOp, EvalError, EvalResult, Limit, Overflow};
pub use syntax::Operator;
pub(crate) use syntax::{BinaryOp, Comparison, UnaryOp};

pub(crate) fn binary(op: BinaryOp, a: Value, b: Value) -> EvalResult<Value> {
    use Value::*;
    // An operator sees a code as the text it spells, so `USD == "USD"` holds
    // and a code concatenates like any other text.
    let (a, b) = (a.plain(), b.plain());
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
