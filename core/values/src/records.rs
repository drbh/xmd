//! The typed boundary between Rust and the .xmd modules.
//!
//! Form definitions, query records, link contexts and solver models all cross into
//! module functions as records. Rather than assembling a `BTreeMap` field by
//! field at each call site and picking it apart again with `fields.get(...)`
//! chains on the way back, each of those shapes is a Rust struct, declared
//! with [`record!`] beside the feature that owns it, that implements
//! [`ToValue`], [`FromValue`], or both. The field names are the module API, so
//! every record keeps them as associated constants and uses those constants in
//! both directions. This is the vocabulary they are declared in.
//!
//! The leaf conversions are keyed by Rust type: `i64` is a `Duration`, `usize` a
//! `Count`, `f64` a `Number`, `String` a `Text`, `bool` a `Bool`,
//! `DateTime<FixedOffset>` a `DateTime`, `NaiveDate` a `Date`, and `Value`
//! itself passes through untouched. `Option<T>` is `Null`-or-`T`.
use crate::error::{EvalError, EvalResult};
use crate::value::{Value, record};
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::collections::BTreeMap;

/// A Rust value that a module can be handed.
pub trait ToValue {
    fn to_value(&self) -> Value;
}
/// A Rust value that can be read back out of what a module returned.
pub trait FromValue: Sized {
    fn from_value(value: &Value) -> EvalResult<Self>;
}

impl ToValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}
/// A leaf: one Rust type, the `Value` variant it crosses as, and what a
/// module was expected to return in its place.
macro_rules! leaf {
    ($ty:ty, $variant:ident $(, $expected:literal)?) => {
        impl ToValue for $ty {
            fn to_value(&self) -> Value {
                Value::$variant(self.clone())
            }
        }
        $(impl FromValue for $ty {
            fn from_value(value: &Value) -> EvalResult<Self> {
                match value {
                    Value::$variant(v) => Ok(v.clone()),
                    _ => Err(EvalError::Expected($expected)),
                }
            }
        })?
    };
}
leaf!(bool, Bool);
leaf!(String, Text, "text");
// A whole number of seconds: the `Duration` a module reads and writes.
leaf!(i64, Duration);
leaf!(usize, Count);
leaf!(f64, Number);
leaf!(DateTime<FixedOffset>, DateTime);
leaf!(NaiveDate, Date);
/// `Null` is the absent one: nothing else stands in for a missing value.
impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        match self {
            Some(v) => v.to_value(),
            None => Value::Null,
        }
    }
}
impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::Null => Ok(None),
            _ => T::from_value(value).map(Some),
        }
    }
}
impl<T: ToValue> ToValue for Vec<T> {
    fn to_value(&self) -> Value {
        Value::list(self.iter().map(ToValue::to_value).collect())
    }
}
impl<T: FromValue> FromValue for Vec<T> {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::List(items) => items.iter().map(T::from_value).collect(),
            _ => Err(EvalError::Expected("a list")),
        }
    }
}
impl<T: ToValue> ToValue for BTreeMap<String, T> {
    fn to_value(&self) -> Value {
        Value::record(
            self.iter()
                .map(|(k, v)| (k.clone(), v.to_value()))
                .collect(),
        )
    }
}
impl<T: FromValue> FromValue for BTreeMap<String, T> {
    fn from_value(value: &Value) -> EvalResult<Self> {
        Fields::new(value)?
            .iter()
            .map(|(k, v)| Ok((k.clone(), T::from_value(v)?)))
            .collect()
    }
}

/// JSON a module or a cache hands back, as module values. Every JSON number
/// reads as a `Number`, whole or not, because module code does arithmetic on
/// it; the `records` crate's own reading of query JSON keeps whole numbers as `Count`
/// instead. [`json()`](crate::json) goes the other way and writes a whole, non-negative
/// `Number` as an integer, so a round trip does not grow a `.0`.
pub fn from_json(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(v) => Value::Bool(*v),
        serde_json::Value::Number(v) => Value::Number(v.as_f64().unwrap_or_default()),
        serde_json::Value::String(v) => Value::Text(v.clone()),
        serde_json::Value::Array(v) => Value::list(v.iter().map(from_json).collect()),
        serde_json::Value::Object(v) => record(v.iter().map(|(k, v)| (k.as_str(), from_json(v)))),
    }
}

/// A source range or position as a module reads it: a record of `Number`s.
pub fn geometry(value: impl serde::Serialize) -> Value {
    from_json(&serde_json::json!(value))
}

/// A Rust struct a module or a query reads as a record.
pub trait RecordFields {
    /// Every field by name, flattened ones included.
    fn fields(&self) -> BTreeMap<String, Value>;
}

/// A record whose keys are only known at run time, such as the collections a
/// module asked for; flattened into a declared record like any other.
impl RecordFields for BTreeMap<String, Value> {
    fn fields(&self) -> BTreeMap<String, Value> {
        self.clone()
    }
}

/// Declare a record once: the struct and its [`ToValue`]. A
/// field's key is its Rust name unless it names one (`type_: String =>
/// "type"`), and a field written `..base: Base` is flattened in, its own fields
/// joining this record's.
#[macro_export]
macro_rules! record {
    (@key $field:ident $key:literal) => { $key };
    (@key $field:ident) => { stringify!($field) };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(.. $flat:ident: $flat_ty:ty,)*
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident: $ty:ty $(=> $key:literal)?,)*
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(pub(crate) $flat: $flat_ty,)*
            $($(#[$field_meta])* $field_vis $field: $ty,)*
        }
        impl $crate::RecordFields for $name {
            fn fields(&self) -> ::std::collections::BTreeMap<String, $crate::Value> {
                let mut fields = ::std::collections::BTreeMap::new();
                $(fields.extend($crate::RecordFields::fields(&self.$flat));)*
                $(
                    fields.insert(
                        $crate::record!(@key $field $($key)?).to_string(),
                        $crate::ToValue::to_value(&self.$field),
                    );
                )*
                fields
            }
        }
        impl $crate::ToValue for $name {
            fn to_value(&self) -> $crate::Value {
                $crate::Value::record($crate::RecordFields::fields(self))
            }
        }
    };
}

/// Reads the fields of a record a module returned.
#[derive(Clone, Copy)]
pub struct Fields<'a> {
    fields: &'a BTreeMap<String, Value>,
}
impl<'a> Fields<'a> {
    pub fn new(value: &'a Value) -> EvalResult<Self> {
        match value {
            Value::Record(fields) => Ok(Self { fields }),
            _ => Err(EvalError::Expected("a record")),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&'a String, &'a Value)> {
        self.fields.iter()
    }
    /// The field must be there and must decode.
    pub fn required<T: FromValue>(&self, key: &str) -> EvalResult<T> {
        T::from_value(
            self.fields
                .get(key)
                .ok_or_else(|| EvalError::Message(format!("Missing field '{key}'")))?,
        )
    }
    /// Absent reads as nothing; anything present, `Null` included, must decode.
    pub fn present<T: FromValue>(&self, key: &str) -> EvalResult<Option<T>> {
        self.fields.get(key).map(T::from_value).transpose()
    }
}
