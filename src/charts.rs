//! Plain-text charts for hovers, tooltips and labels. Every editor renders
//! these; no client needs image or HTML support.
use crate::engine::Value;

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

/// A compact bar for inline labels: eight cells, no percentage.
pub fn gauge(done: usize, total: usize) -> String {
    gauge_fraction(if total == 0 {
        0.0
    } else {
        done as f64 / total as f64
    })
}
pub fn gauge_fraction(fraction: f64) -> String {
    const WIDTH: usize = 8;
    let filled = (fraction.clamp(0.0, 1.0) * WIDTH as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(WIDTH - filled))
}

/// One block per value, scaled between the minimum and maximum: `▁▂▃▅▇`.
pub fn sparkline(values: &[f64]) -> String {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return String::new();
    }
    let min = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let max = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    finite
        .iter()
        .map(|v| {
            let level = if max > min {
                ((v - min) / (max - min) * (BLOCKS.len() - 1) as f64).round() as usize
            } else {
                BLOCKS.len() / 2
            };
            BLOCKS[level.min(BLOCKS.len() - 1)]
        })
        .collect()
}

/// A numeric magnitude for charting; text, dates and timers have none.
pub fn magnitude(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) | Value::Money(n) | Value::Ratio(n) => Some(*n),
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
