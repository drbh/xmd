//! Plain-text charts for hovers, tooltips and labels. Every editor renders
//! these; no client needs image or HTML support.
use crate::{
    engine_impl::{Value, ValueType},
    error::{EvalError, EvalResult},
};

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A horizontal progress bar such as `██████░░░░ 60%`.
pub fn bar(done: usize, total: usize) -> String {
    bar_fraction(if total == 0 {
        0.0
    } else {
        done as f64 / total as f64
    })
}
pub fn bar_fraction(fraction: f64) -> String {
    const WIDTH: usize = 10;
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = (fraction * WIDTH as f64).round() as usize;
    format!(
        "{}{} {}%",
        "█".repeat(filled),
        "░".repeat(WIDTH - filled),
        (fraction * 100.0).round() as i64
    )
}

/// One block per value, scaled between the minimum and maximum: `▁▂▃▅▇`.
pub(crate) fn sparkline(values: &[f64]) -> String {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return String::new();
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    finite.iter().map(|v| spark_block(*v, min, max)).collect()
}

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

/// Sparkline plus range for a series of scalar values, or `None` when fewer
/// than two values are chartable.
pub fn series(values: &[Value]) -> Option<String> {
    let numeric: Vec<&Value> = values.iter().filter(|v| magnitude(v).is_some()).collect();
    if numeric.len() < 2 {
        return None;
    }
    let magnitudes: Vec<f64> = numeric.iter().map(|v| magnitude(v).unwrap()).collect();
    let extreme = |pick: fn(f64, f64) -> bool| {
        numeric
            .iter()
            .zip(&magnitudes)
            .reduce(|a, b| if pick(*b.1, *a.1) { b } else { a })
            .map(|(v, _)| v.display())
            .unwrap()
    };
    let min = extreme(|candidate, current| candidate < current);
    let max = extreme(|candidate, current| candidate > current);
    Some(format!("`{}` {} → {}", sparkline(&magnitudes), min, max))
}
