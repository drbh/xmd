//! Tagged records: a record a module built that a note sees as a kind of its
//! own, named by the module. `tagged(kind, fields, display, hover?)` makes
//! one; it reads its fields like a record, names its kind in `type`, hovers
//! and errors, shows `display` wherever a value is shown, adds `hover`
//! (Markdown) to a symbol's hover, and is the plain record in a query or its
//! JSON. Like every object, it has no source form, so a list of them shows as
//! a collection. The language gives no kind a meaning: any name that reads as
//! a kind and is none of the language's own ([`ValueType::taggable`]) is the
//! module's to choose.
//!
//! A record built with an `origin` field that is null asks to learn where it
//! was made: the definition whose whole expression is the call that built it
//! claims it ([`claim`]), filling `origin` with that definition, so a module
//! can offer a control that rewrites it.
use crate::error::{EvalError, EvalResult};
use crate::value::{HostObject, Value};
use common::ValueType;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tagged {
    kind: Arc<str>,
    fields: Arc<crate::Measured<BTreeMap<String, Value>>>,
    display: String,
    hover: Option<String>,
}

impl Tagged {
    /// A value holding a tagged record of the kind named `kind`, which must be
    /// one a module may choose ([`ValueType::taggable`]).
    pub(crate) fn value(
        kind: &str,
        fields: &Value,
        display: &Value,
        hover: Option<&Value>,
    ) -> EvalResult<Value> {
        if !ValueType::taggable(kind) {
            return Err(EvalError::Message(format!(
                "tagged expects a kind of its own: a capitalized name, letters, \
                 numbers and underscores, that no built-in kind has, not '{kind}'"
            )));
        }
        let hover = match hover {
            None => None,
            Some(Value::Text(hover)) => Some(hover.clone()),
            Some(_) => {
                return Err(EvalError::Message(
                    "tagged expects its hover as text".into(),
                ));
            }
        };
        let (Value::Record(fields), Value::Text(display)) = (fields, display) else {
            return Err(EvalError::Message(
                "tagged expects a kind, a record and its display text".into(),
            ));
        };
        Ok(Value::Host(Arc::new(Self {
            kind: kind.into(),
            fields: fields.clone(),
            display: display.clone(),
            hover,
        })))
    }
}

/// The field a tagged record names where it was made in.
const ORIGIN: &str = "origin";

/// `value` with its `origin` filled by `origin()`, when it is a tagged record
/// that asks to learn where it was made (its `origin` is null) and `origin()`
/// names a place; `None` for every other value, which stays as it is.
/// `origin` is only called for a record that asks.
pub fn claim(value: &Value, origin: impl FnOnce() -> Option<Value>) -> Option<Value> {
    let tagged = value.downcast::<Tagged>()?;
    if tagged.fields.get(ORIGIN) != Some(&Value::Null) {
        return None;
    }
    let mut claimed = tagged.clone();
    Arc::make_mut(&mut claimed.fields).insert(ORIGIN.into(), origin()?);
    Some(Value::Host(Arc::new(claimed)))
}

/// Whether `value` is a tagged record of one of `kinds` that a definition
/// claimed ([`claim`]): one whose `origin` names where it was made.
pub fn claimed(value: &Value, kinds: &[String]) -> bool {
    value.downcast::<Tagged>().is_some_and(|tagged| {
        kinds.iter().any(|kind| **kind == *tagged.kind)
            && tagged
                .fields
                .get(ORIGIN)
                .is_some_and(|origin| *origin != Value::Null)
    })
}

impl HostObject for Tagged {
    fn kind(&self) -> ValueType {
        ValueType::Tagged
    }
    fn type_name(&self) -> &str {
        &self.kind
    }
    fn display(&self) -> String {
        self.display.clone()
    }
    fn property(&self, key: &str) -> EvalResult<Value> {
        self.fields.get(key).cloned().ok_or_else(|| {
            EvalError::Message(format!(
                "Unknown {} property '{key}'",
                self.kind.to_lowercase()
            ))
        })
    }
    fn fields(&self) -> Vec<String> {
        self.fields.keys().cloned().collect()
    }
    fn query(&self) -> Option<Value> {
        Some(Value::Record(self.fields.clone()))
    }
    fn hover(&self) -> Option<String> {
        self.hover.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any kind a module chooses names itself in `type` and its errors, and
    /// a definition claims it; a built-in kind's name, or one that does not
    /// read as a kind, is refused.
    #[test]
    fn a_module_chooses_the_kind() {
        let fields = crate::record([("origin", Value::Null), ("n", Value::Count(1))]);
        let display = Value::Text("one".into());
        let value = Tagged::value("Alarm", &fields, &display, None).unwrap();
        assert_eq!(value.kind(), ValueType::Tagged);
        assert_eq!(value.type_name(), "Alarm");
        assert_eq!(value.property("n"), Ok(Value::Count(1)));
        assert_eq!(
            value.property("x").unwrap_err().to_string(),
            "Unknown alarm property 'x'"
        );
        assert!(!claimed(&value, &["Alarm".into()]));
        let made = claim(&value, || Some(Value::Text("here".into()))).unwrap();
        assert!(claimed(&made, &["Alarm".into()]));
        assert!(!claimed(&made, &["Lap".into()]));
        for kind in ["Number", "Tagged", "alarm", "", "A-b"] {
            assert!(
                Tagged::value(kind, &fields, &display, None).is_err(),
                "{kind}"
            );
        }
    }
}
