//! Cached lookups: `rate(EUR, USD)`, `to(money, USD)`,
//! `forecast("Oaxaca", 2026-11-20[, F])`, `forecast_range` and `quote(NVDA)`.
//! Values come from the workspace's lookup cache and are never fetched here;
//! every key read, hit or miss, is recorded so a refresh knows what to fetch.
use crate::engine::{Builtin, Currency, Engine, Expr, Value};
use std::path::Path;
use values::{EvalError, EvalResult, Limit};

/// The lookup built-ins, as `features` registers them.
pub(crate) fn call(
    engine: &mut Engine<'_>,
    path: &Path,
    name: Builtin,
    args: &[Expr],
) -> EvalResult<Value> {
    // A code literal is what these calls are written with; text is still
    // accepted, so a computed name works too.
    let code = |value: Value, what: &str| match value {
        Value::Code(code) => Ok(code.to_string()),
        Value::Text(code) => Ok(code),
        other => Err(EvalError::Message(format!(
            "{what} must be a code such as USD, found {}",
            other.type_name()
        ))),
    };
    let currency = |code: &str| {
        Currency::parse(code).ok_or_else(|| {
            EvalError::Message(format!("'{code}' is not a currency code such as USD"))
        })
    };
    match name {
        Builtin::Rate => {
            if args.len() != 2 {
                return Err(EvalError::Message(
                    "rate expects two currency codes: rate(EUR, USD)".into(),
                ));
            }
            let from = currency(&code(engine.expr(path, &args[0])?, "The first currency")?)?;
            let to = currency(&code(engine.expr(path, &args[1])?, "The second currency")?)?;
            rate(engine, from, to).map(Value::Number)
        }
        Builtin::To => {
            if args.len() != 2 {
                return Err(EvalError::Message(
                    "to expects a money value and a currency code: to(hotel, USD)".into(),
                ));
            }
            let Value::Money(amount, from) = engine.expr(path, &args[0])? else {
                return Err(EvalError::Message(
                    "to converts money; the first argument is not money".into(),
                ));
            };
            let to = currency(&code(engine.expr(path, &args[1])?, "The currency")?)?;
            Ok(Value::Money(amount * rate(engine, from, to)?, to))
        }
        Builtin::Quote => {
            if args.len() != 1 {
                return Err(EvalError::Message(
                    "quote expects a ticker symbol: quote(NVDA)".into(),
                ));
            }
            let symbol = code(engine.expr(path, &args[0])?, "The ticker")?;
            engine.wanted.push(values::LookupKey::quote(&symbol));
            values::quote(&engine.workspace().lookups, &symbol)
        }
        _ => {
            let range = name == Builtin::ForecastRange;
            if range && !(3..=4).contains(&args.len()) {
                return Err(EvalError::Message(
                    "forecast_range expects a place, start date, end date, and optional unit: forecast_range(\"Oaxaca\", 2026-11-20, 2026-11-26, F)".into(),
                ));
            }
            if !range && !(2..=3).contains(&args.len()) {
                return Err(EvalError::Message(
                    "forecast expects a place and a date: forecast(\"Oaxaca\", 2026-11-20)".into(),
                ));
            }
            let Value::Text(place) = engine.expr(path, &args[0])? else {
                return Err(EvalError::Message(
                    "The place must be text, e.g. forecast(\"Oaxaca\", 2026-11-20)".into(),
                ));
            };
            let value = engine.expr(path, &args[1])?;
            let date = engine.date(&value)?;
            let end = if range {
                let value = engine.expr(path, &args[2])?;
                engine.date(&value)?
            } else {
                date
            };
            let days = (end - date).num_days();
            if days < 0 {
                return Err(EvalError::Message(
                    "forecast_range end date must be on or after the start date".into(),
                ));
            }
            if days >= 4096 {
                return Err(EvalError::LimitExceeded(Limit::ListItems));
            }
            let fahrenheit = match args.get(if range { 3 } else { 2 }) {
                Some(unit) => match code(engine.expr(path, unit)?, "The unit")?.as_str() {
                    "F" | "FAHRENHEIT" => true,
                    "C" | "CELSIUS" => false,
                    other => {
                        return Err(EvalError::Message(format!(
                            "Unknown temperature unit '{other}'; use F or C"
                        )));
                    }
                },
                None => false,
            };
            let dates: Vec<_> = (0..=days)
                .map(|offset| date + chrono::Duration::days(offset))
                .collect();
            // Discover the entire interval before a missing cache entry
            // can fail evaluation, so one refresh fetches every day.
            engine.wanted.extend(
                dates
                    .iter()
                    .map(|date| values::LookupKey::forecast(&place, *date)),
            );
            let lookups = &engine.workspace().lookups;
            let mut forecasts = dates
                .into_iter()
                .map(|date| {
                    values::forecast(lookups, &place, date, fahrenheit).map(Value::Forecast)
                })
                .collect::<EvalResult<Vec<_>>>()?;
            Ok(if range {
                Value::list(forecasts)
            } else {
                forecasts.remove(0)
            })
        }
    }
}

/// The cached rate from one currency to another, noting that it was read.
fn rate(engine: &mut Engine<'_>, from: Currency, to: Currency) -> EvalResult<f64> {
    if from != to {
        engine.wanted.push(values::LookupKey::rate(from, to));
    }
    values::rate(&engine.workspace().lookups, from, to)
}
