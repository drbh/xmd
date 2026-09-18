//! Small, pure additions to the shared expression language.
use super::engine::{Expr, Value};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub(crate) params: Vec<String>,
    pub(crate) body: Expr,
    pub(crate) path: PathBuf,
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
    )
}

pub fn builtin(name: &str, args: &[Value]) -> Result<Value, String> {
    use Value::*;
    Ok(match (name, args) {
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
            List(text.split(separator).map(|s| Text(s.into())).collect())
        }
        ("join", [List(items), Text(separator)]) => Text(
            items
                .iter()
                .map(|v| match v {
                    Text(s) => Ok(s.as_str()),
                    _ => Err("join requires a list of text"),
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(separator),
        ),
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
