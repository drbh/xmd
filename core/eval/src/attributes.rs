//! An attribute's value as its declaration reads it: checked by
//! `Engine::attribute`, which diagnostics and attributed records report,
//! unchecked by `Engine::evaluated`, which a checklist's task records carry,
//! and a date or time by `Engine::when`. What a dependencies attribute still
//! waits on is the checklist's (`Engine::unmet`).
use crate::engine::{Engine, Value, date_value, relative_date};
use std::path::Path;
use values::{EvalError, EvalResult};

impl Engine<'_> {
    /// An attribute's value, evaluated in the note's scope as its declaration
    /// says: a date or time, a calendar date, a nonnegative duration, a named
    /// tagged record, the dependencies not met yet (as text, one per condition), any
    /// value, or its text. `item` is the checklist item the line is, when it
    /// is one.
    pub fn attribute(
        &mut self,
        path: &Path,
        declared: &document::Declaration,
        item: Option<usize>,
        attribute: &document::Attribute,
    ) -> EvalResult<Value> {
        use syntax::AttributeValue as Holds;
        let key = &declared.key;
        match declared.value {
            Holds::Duration => match self.eval_at(path, &attribute.value, attribute.value_span)? {
                Value::Duration(seconds) if seconds >= 0 => Ok(Value::Duration(seconds)),
                _ => Err(EvalError::Expected("a nonnegative duration")),
            },
            Holds::Tagged => {
                let value = self.eval_at(path, &attribute.value, attribute.value_span)?;
                // A tagged record of a declared kind its own definition made.
                if values::claimed(&value, &declared.kinds) && syntax::identifier(&attribute.value)
                {
                    Ok(value)
                } else {
                    let kinds: Vec<String> =
                        declared.kinds.iter().map(|k| k.to_lowercase()).collect();
                    Err(format!(
                        "@{key} requires a named {}, e.g. @{key}({})",
                        kinds.join(" or "),
                        declared.example
                    )
                    .into())
                }
            }
            Holds::Date => syntax::stamp(&attribute.value)
                .map(Value::Date)
                .ok_or_else(|| {
                    EvalError::from(format!(
                        "@{key} requires a calendar date, e.g. @{key}({})",
                        declared.example
                    ))
                }),
            Holds::Dependencies => self
                .unmet(path, item, key, &attribute.value)
                .map(|names| Value::list(names.into_iter().map(Value::Text).collect())),
            _ => self.evaluated(path, declared.value, attribute),
        }
    }
    /// An attribute's value read as its kind reads it, unchecked: relative
    /// or evaluated as a date for a time, the date a calendar date writes,
    /// the expression's value for any other expression, else its text.
    pub(crate) fn evaluated(
        &mut self,
        path: &Path,
        holds: syntax::AttributeValue,
        attribute: &document::Attribute,
    ) -> EvalResult<Value> {
        use syntax::AttributeValue as Holds;
        match holds {
            Holds::When => self.when(path, &attribute.value),
            Holds::Date => Ok(syntax::stamp(&attribute.value)
                .map(Value::Date)
                .unwrap_or_else(|| Value::Text(attribute.value.clone()))),
            Holds::Duration | Holds::Tagged | Holds::Dependencies | Holds::Expression => {
                self.eval_at(path, &attribute.value, attribute.value_span)
            }
            Holds::Text => Ok(Value::Text(attribute.value.clone())),
        }
    }
    pub fn when(&mut self, path: &Path, source: &str) -> EvalResult<Value> {
        if let Some(v) = date_value(source) {
            return Ok(v);
        }
        if let Some(v) = relative_date(source, self.request.clock.today()) {
            return Ok(Value::Date(v));
        }
        let value = self.eval(path, source)?;
        self.date(&value)?;
        Ok(value)
    }
}
