//! Small, pure additions to the shared expression language.
use super::engine::{Expr, Value};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub(crate) params: Vec<String>,
    pub(crate) body: Expr,
    pub(crate) path: PathBuf,
    pub(crate) source: Option<(PathBuf, crate::document::Span)>,
    pub(crate) captured: BTreeMap<String, Value>,
}

pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "map"
            | "filter"
            | "fold"
            | "get"
            | "length"
            | "text"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "split"
            | "join"
            | "lower"
            | "upper"
            | "replace"
            | "slice"
            | "concat"
            | "trim"
            | "type"
            | "floor"
            | "round"
            | "repeat"
            | "format_date"
            | "error"
    )
}

pub fn builtin(name: &str, args: &[Value]) -> Result<Value, String> {
    use Value::*;
    Ok(match (name, args) {
        ("type", [value]) => Text(value.type_name().into()),
        ("error", [Text(message)]) => return Err(message.clone()),
        ("trim", [Text(text)]) => Text(text.trim().into()),
        ("floor" | "round", [value]) => {
            let number = match value {
                Number(n) | Ratio(n) => *n,
                Count(n) => *n as f64,
                _ => return Err(format!("{name} requires a number")),
            };
            Number(if name == "floor" {
                number.floor()
            } else {
                number.round()
            })
        }
        ("concat", lists) => {
            let mut result = Vec::new();
            for list in lists {
                let List(items) = list else {
                    return Err("concat requires lists".into());
                };
                if result.len().saturating_add(items.len()) > 8192 {
                    return Err("List exceeds the collection size limit".into());
                }
                result.extend(items.clone());
            }
            List(result)
        }
        ("slice", [value, start, end]) => {
            let start = index(start)?;
            let end = index(end)?;
            if start > end {
                return Err("slice start must not exceed end".into());
            }
            match value {
                Text(text) => Text(text.chars().skip(start).take(end - start).collect()),
                List(items) => List(items[start.min(items.len())..end.min(items.len())].to_vec()),
                _ => return Err("slice requires text or a list".into()),
            }
        }
        ("repeat", [Text(text), count]) => {
            let count = index(count)?;
            if text.len().saturating_mul(count) > 1_048_576 || count > 8192 {
                return Err("Repeated text exceeds the size limit".into());
            }
            Text(text.repeat(count))
        }
        ("format_date", [value, Text(format)]) => {
            if chrono::format::StrftimeItems::new(format)
                .any(|i| matches!(i, chrono::format::Item::Error))
            {
                return Err("Invalid date format".into());
            }
            Text(match value {
                DateTime(d) => d.format(format).to_string(),
                Date(d) => {
                    // Reject time/offset specifiers for dates rather than panicking in Display.
                    let mut result = String::new();
                    std::fmt::write(&mut result, format_args!("{}", d.format(format)))
                        .map_err(|_| "Format needs a time or timezone")?;
                    result
                }
                _ => return Err("format_date requires a date or timestamp".into()),
            })
        }
        ("get", [Record(fields), Text(key)]) => fields.get(key).cloned().unwrap_or(Null),
        ("get", [List(items), index]) => {
            let index = match index {
                Count(n) => *n,
                Number(n) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => *n as usize,
                _ => return Err("List index must be a nonnegative integer".into()),
            };
            items.get(index).cloned().unwrap_or(Null)
        }
        ("get", [Null, _]) => Null,
        ("length", [List(items)]) => Count(items.len()),
        ("length", [Record(fields)]) => Count(fields.len()),
        ("length", [Text(text)]) => Count(text.chars().count()),
        ("text", [value]) => Text(value.display()),
        ("contains", [Text(text), Text(part)]) => Bool(text.contains(part)),
        ("contains", [List(items), value]) => Bool(items.contains(value)),
        ("starts_with", [Text(text), Text(part)]) => Bool(text.starts_with(part)),
        ("ends_with", [Text(text), Text(part)]) => Bool(text.ends_with(part)),
        ("split", [Text(text), Text(separator)]) => {
            let parts = text.split(separator).take(8193).collect::<Vec<_>>();
            if parts.len() > 8192 {
                return Err("List exceeds the collection size limit".into());
            }
            List(parts.into_iter().map(|s| Text(s.into())).collect())
        }
        ("join", [List(items), Text(separator)]) => {
            let parts = items
                .iter()
                .map(|v| match v {
                    Text(s) => Ok(s.as_str()),
                    _ => Err("join requires a list of text"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let size = parts.iter().map(|s| s.len()).sum::<usize>().saturating_add(
                parts
                    .len()
                    .saturating_sub(1)
                    .saturating_mul(separator.len()),
            );
            if size > 1_048_576 {
                return Err("Text exceeds 1 MiB".into());
            }
            Text(parts.join(separator))
        }
        ("lower", [Text(text)]) => Text(text.to_lowercase()),
        ("upper", [Text(text)]) => Text(text.to_uppercase()),
        ("replace", [Text(text), Text(from), Text(to)]) => {
            let count = text.matches(from).count();
            if count.saturating_mul(to.len()).saturating_add(text.len()) > 1_048_576 {
                return Err("Text exceeds 1 MiB".into());
            }
            Text(text.replace(from, to))
        }
        _ => return Err(format!("Invalid arguments for {name}")),
    })
}

pub(crate) fn check_size(value: &Value) -> Result<(), String> {
    let mut pending = vec![value];
    let mut count = 0usize;
    let mut bytes = 0usize;
    while let Some(value) = pending.pop() {
        count += 1;
        match value {
            Value::List(items) => pending.extend(items),
            Value::Record(fields) => {
                pending.extend(fields.values());
                bytes += fields.keys().map(String::len).sum::<usize>();
            }
            Value::Text(text) => bytes += text.len(),
            _ => (),
        }
        if count + pending.len() > 8192 || bytes > 1_048_576 {
            return Err("Value exceeds the collection or text size limit".into());
        }
    }
    Ok(())
}

fn index(value: &Value) -> Result<usize, String> {
    match value {
        Value::Count(n) => Ok(*n),
        Value::Number(n)
            if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n < usize::MAX as f64 =>
        {
            Ok(*n as usize)
        }
        _ => Err("Expected a nonnegative integer".into()),
    }
}
