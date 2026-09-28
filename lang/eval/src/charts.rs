//! The `sparkline` built-in: one block per value, scaled between a minimum and
//! maximum, so every editor renders the chart as text. Hover charts are built
//! on it by `format.series` in the stdlib.
use crate::{
    engine_impl::{Value, ValueType},
    error::{EvalError, EvalResult},
};

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn spark_block(value: f64, min: f64, max: f64) -> char {
    if min == max {
        return BLOCKS[BLOCKS.len() / 2];
    }
    let value = value.clamp(min, max);
    let span = max - min;
    let fraction = if span.is_finite() {
        (value - min) / span
    } else {
        // Opposite finite extremes may have an infinite difference.
        (value / 2.0 - min / 2.0) / (max / 2.0 - min / 2.0)
    };
    BLOCKS[((fraction * (BLOCKS.len() - 1) as f64).round() as usize).min(BLOCKS.len() - 1)]
}

/// An explicit inline chart validates its units and retains missing samples,
/// so each position still corresponds to the same day or table row.
pub(crate) fn inline_sparkline(
    values: &[Value],
    bounds: Option<(&Value, &Value)>,
) -> EvalResult<String> {
    let mut unit = None;
    let mut numeric = |value: &Value| {
        let number = magnitude(value).filter(|v| v.is_finite()).ok_or_else(|| {
            EvalError::Message("sparkline expects finite numeric values or null gaps".into())
        })?;
        let kind = match value {
            Value::Count(_) => (ValueType::Number, None),
            Value::Money(_, currency) => (value.kind(), Some(*currency)),
            _ => (value.kind(), None),
        };
        if unit.is_some_and(|unit| unit != kind) {
            return Err(EvalError::Message(
                "sparkline values and bounds must use matching units".into(),
            ));
        }
        unit = Some(kind);
        Ok(number)
    };
    let bounds = bounds
        .map(|(min, max)| Ok::<_, EvalError>((numeric(min)?, numeric(max)?)))
        .transpose()?;
    if bounds.is_some_and(|(min, max)| min >= max) {
        return Err(EvalError::Message(
            "sparkline minimum must be less than its maximum".into(),
        ));
    }
    let samples: Vec<_> = values
        .iter()
        .map(|value| {
            if matches!(value, Value::Null) {
                Ok(None)
            } else {
                numeric(value).map(Some)
            }
        })
        .collect::<EvalResult<_>>()?;
    let (min, max) = bounds.unwrap_or_else(|| {
        samples
            .iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), value| {
                (min.min(*value), max.max(*value))
            })
    });
    Ok(samples
        .into_iter()
        .map(|value| value.map_or('·', |value| spark_block(value, min, max)))
        .collect())
}

/// A numeric magnitude for charting; text, dates and timers have none.
pub(crate) fn magnitude(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) | Value::Money(n, _) | Value::Ratio(n) => Some(*n),
        Value::Duration(s) => Some(*s as f64),
        Value::Count(n) => Some(*n as f64),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}
