//! The typed boundary between Rust and the .x.md modules.
//!
//! Timers, plans, itinerary days, link contexts and solver models all cross into
//! module functions as records. Rather than assembling a `BTreeMap` field by
//! field at each call site and picking it apart again with `fields.get(...)`
//! chains on the way back, each of those shapes is a Rust struct here that
//! implements [`ToValue`], [`FromValue`], or both. The field names are the
//! module API, so every record keeps them as associated constants and uses
//! those constants in both directions.
//!
//! The leaf conversions are keyed by Rust type: `i64` is a `Duration`, `usize` a
//! `Count`, `f64` a `Number`, `String` a `Text`, `bool` a `Bool`,
//! `DateTime<FixedOffset>` a `DateTime`, `NaiveDate` a `Date`, and `Value`
//! itself passes through untouched. `Option<T>` is `Null`-or-`T`.
use crate::engine_impl::Value;
use crate::error::{EvalError, EvalResult};
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::collections::BTreeMap;

/// A Rust value that a module can be handed.
pub trait ToValue {
    fn to_value(&self) -> Value;
}
/// A Rust value that can be read back out of what a module returned.
pub(crate) trait FromValue: Sized {
    fn from_value(value: &Value) -> EvalResult<Self>;
}

impl ToValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}
impl FromValue for Value {
    fn from_value(value: &Value) -> EvalResult<Self> {
        Ok(value.clone())
    }
}
impl ToValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}
impl FromValue for bool {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::Bool(v) => Ok(*v),
            _ => Err(EvalError::Expected("a boolean")),
        }
    }
}
impl ToValue for String {
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}
impl FromValue for String {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::Text(v) => Ok(v.clone()),
            _ => Err(EvalError::Expected("text")),
        }
    }
}
/// A whole number of seconds: the `Duration` a module reads and writes.
impl ToValue for i64 {
    fn to_value(&self) -> Value {
        Value::Duration(*self)
    }
}
impl FromValue for i64 {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::Duration(v) => Ok(*v),
            _ => Err(EvalError::Expected("a duration")),
        }
    }
}
impl ToValue for usize {
    fn to_value(&self) -> Value {
        Value::Count(*self)
    }
}
impl FromValue for usize {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::Count(v) => Ok(*v),
            _ => Err(EvalError::Expected("a count")),
        }
    }
}
impl ToValue for f64 {
    fn to_value(&self) -> Value {
        Value::Number(*self)
    }
}
impl FromValue for f64 {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::Number(v) => Ok(*v),
            _ => Err(EvalError::Expected("a number")),
        }
    }
}
impl ToValue for DateTime<FixedOffset> {
    fn to_value(&self) -> Value {
        Value::DateTime(*self)
    }
}
impl FromValue for DateTime<FixedOffset> {
    fn from_value(value: &Value) -> EvalResult<Self> {
        match value {
            Value::DateTime(v) => Ok(*v),
            _ => Err(EvalError::Expected("a timestamp")),
        }
    }
}
impl ToValue for NaiveDate {
    fn to_value(&self) -> Value {
        Value::Date(*self)
    }
}
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
        Value::List(self.iter().map(ToValue::to_value).collect())
    }
}
impl<T: FromValue> FromValue for Vec<T> {
    fn from_value(value: &Value) -> EvalResult<Self> {
        list(value)?.iter().map(T::from_value).collect()
    }
}
impl<T: ToValue> ToValue for BTreeMap<String, T> {
    fn to_value(&self) -> Value {
        Value::Record(
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

fn list(value: &Value) -> EvalResult<&[Value]> {
    match value {
        Value::List(items) => Ok(items),
        _ => Err(EvalError::Expected("a list")),
    }
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
            fn fields(&self) -> ::std::collections::BTreeMap<String, $crate::engine::Value> {
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
            fn to_value(&self) -> $crate::engine::Value {
                $crate::engine::Value::Record($crate::RecordFields::fields(self))
            }
        }
    };
}

/// Reads the fields of a record a module returned.
#[derive(Clone, Copy)]
pub(crate) struct Fields<'a> {
    fields: &'a BTreeMap<String, Value>,
}
impl<'a> Fields<'a> {
    pub(crate) fn new(value: &'a Value) -> EvalResult<Self> {
        Self::expect(value, EvalError::Expected("a record"))
    }
    /// The same, for the call sites that name the record in their own words.
    pub(crate) fn expect(value: &'a Value, error: impl Into<EvalError>) -> EvalResult<Self> {
        match value {
            Value::Record(fields) => Ok(Self { fields }),
            _ => Err(error.into()),
        }
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'a String, &'a Value)> {
        self.fields.iter()
    }
    /// The field must be there and must decode.
    pub(crate) fn required<T: FromValue>(&self, key: &str) -> EvalResult<T> {
        T::from_value(
            self.fields
                .get(key)
                .ok_or_else(|| EvalError::Message(format!("Missing field '{key}'")))?,
        )
    }
    /// The same, with one error for every way it can go wrong.
    pub(crate) fn required_or<T: FromValue>(
        &self,
        key: &str,
        error: impl Into<EvalError>,
    ) -> EvalResult<T> {
        self.required(key).map_err(|_| error.into())
    }
    /// Absent reads as nothing; anything present, `Null` included, must decode.
    pub(crate) fn present<T: FromValue>(&self, key: &str) -> EvalResult<Option<T>> {
        self.fields.get(key).map(T::from_value).transpose()
    }
    /// A required field holding a list of `T`.
    pub(crate) fn list<T: FromValue>(&self, key: &str) -> EvalResult<Vec<T>> {
        self.required(key)
    }
}

record! {
    /// Where a timer stands, as `timer.x.md` reads and writes it.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TimerRecord {
        pub limit: Option<i64>,
        pub elapsed: i64,
        pub started: Option<DateTime<FixedOffset>>,
        pub idle: bool,
    }
}
impl FromValue for TimerRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::expect(value, "Timer constructor must return a record")?;
        Ok(Self {
            limit: fields.required_or("limit", "Invalid timer limit")?,
            elapsed: fields.required_or("elapsed", "Invalid timer elapsed time")?,
            started: fields.required_or("started", "Invalid timer timestamp")?,
            idle: fields.required_or("idle", "Invalid timer state")?,
        })
    }
}

record! {
    /// One side of a linear constraint, as `plan.x.md` receives it: a constant,
    /// the coefficients by variable name, and one value carrying the form's unit.
    pub(crate) struct FormRecord {
        pub constant: f64,
        pub terms: BTreeMap<String, f64>,
        pub unit: Value,
    }
}

record! {
    /// One decision column the plan solves for.
    pub(crate) struct DecisionRecord {
        pub name: String,
        pub kind: String,
    }
}

record! {
    /// One named constraint on the way into `plan.solve_model`.
    pub(crate) struct ConstraintInput {
        pub name: String,
        pub lhs: FormRecord,
        pub rhs: FormRecord,
        pub op: String,
    }
}

record! {
    /// The whole model `plan.solve_model` is handed.
    pub(crate) struct PlanInput {
        pub goal: String,
        pub names: Vec<String>,
        pub decisions: Vec<DecisionRecord>,
        pub objective: FormRecord,
        pub constraints: Vec<ConstraintInput>,
    }
}

/// What one variable came back as.
pub(crate) struct VariableRecord {
    pub name: String,
    pub value: Value,
}
impl FromValue for VariableRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            name: fields.required::<Value>("name")?.display(),
            value: fields.required("value")?,
        })
    }
}

/// How one constraint fared, as `plan.solve_model` reports it.
pub(crate) struct ConstraintOutcome {
    pub name: String,
    pub op: String,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
}
impl FromValue for ConstraintOutcome {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            name: fields.required::<Value>("name")?.display(),
            op: fields.required::<Value>("op")?.display(),
            lhs: fields.required("lhs")?,
            rhs: fields.required("rhs")?,
            slack: fields.required("slack")?,
            binding: matches!(fields.required::<Value>("binding")?, Value::Bool(true)),
        })
    }
}

/// The solved plan coming back out of `plan.solve_model`.
pub(crate) struct SolutionRecord {
    pub variables: Vec<VariableRecord>,
    pub constraints: Vec<ConstraintOutcome>,
    /// The chosen cell per decision column, read by row-variable name.
    pub rows: Value,
    pub objective: Value,
}
impl FromValue for SolutionRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            variables: fields.list("variables")?,
            constraints: fields.list("constraints")?,
            rows: fields.required("rows")?,
            objective: fields.required("objective")?,
        })
    }
}

record! {
    /// One decision cell a plan filled in, with the source geometry an editor
    /// needs to draw and rewrite it.
    pub(crate) struct CellRecord {
        pub value: Value,
        pub label: String,
        pub document: String,
        pub source: String,
        pub line: usize,
        pub anchor: Value,
        pub range: Value,
        pub width: usize,
    }
}

record! {
    /// One decision column, with the cells the plan chose for it.
    pub(crate) struct ColumnRecord {
        pub name: String,
        pub cells: Vec<CellRecord>,
    }
}

/// A solved constraint on its way to `plan.x.md`, with the source geometry when
/// the plan it came from is still in the workspace.
pub(crate) struct ConstraintRecord {
    pub name: String,
    pub op: String,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
    pub anchor: Option<Value>,
    pub range: Option<Value>,
}
impl ToValue for ConstraintRecord {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::from([
            ("name".into(), self.name.to_value()),
            ("op".into(), self.op.to_value()),
            ("lhs".into(), self.lhs.clone()),
            ("rhs".into(), self.rhs.clone()),
            ("slack".into(), self.slack.clone()),
            ("binding".into(), self.binding.to_value()),
        ]);
        // Only a plan still present in the workspace has a place in the source,
        // and a plan elsewhere has no such keys at all rather than null ones.
        if let Some(anchor) = &self.anchor {
            fields.insert("anchor".into(), anchor.clone());
        }
        if let Some(range) = &self.range {
            fields.insert("range".into(), range.clone());
        }
        Value::Record(fields)
    }
}

/// A whole solved plan, as `plan.x.md` renders it. Presentation policy is the
/// module's; this is the typed result plus where it came from.
pub(crate) struct PlanRecord {
    pub goal: String,
    pub objective: Value,
    /// Order matters: it is also the variable order the module reports.
    pub variables: Vec<(String, Value)>,
    pub constraints: Vec<ConstraintRecord>,
    pub columns: Vec<ColumnRecord>,
}
impl ToValue for PlanRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            ("goal".into(), self.goal.to_value()),
            ("objective".into(), self.objective.clone()),
            (
                "variables".into(),
                Value::Record(self.variables.iter().cloned().collect()),
            ),
            (
                "variable_order".into(),
                Value::List(self.variables.iter().map(|(n, _)| n.to_value()).collect()),
            ),
            ("constraints".into(), self.constraints.to_value()),
            ("columns".into(), self.columns.to_value()),
        ]))
    }
}

record! {
    /// Where a line of an itinerary sits, for every record that names one.
    pub(crate) struct LineRecord {
        pub line: usize,
        pub raw: String,
        pub line_range: Value,
        pub anchor: Value,
    }
}

record! {
    /// The calendar parts a day line spells out, before any year is carried
    /// forward.
    pub(crate) struct DayParts {
        pub month: usize,
        pub day: usize,
        pub year: Option<f64>,
    }
}

record! {
    /// What kind of stop a marker names.
    pub(crate) struct KindRecord {
        pub marker: String,
        pub name: String,
    }
}

record! {
    /// One `key: value` line hanging off a stop.
    pub(crate) struct DetailRecord {
        ..line: LineRecord,
        pub key: String,
        pub value: String,
    }
}

record! {
    /// One stop of an itinerary day, with every span the editor draws on.
    pub(crate) struct StopRecord {
        ..line: LineRecord,
        pub kind: Option<KindRecord>,
        pub time: i64,
        pub twelve_hour: bool,
        pub title: String,
        pub time_range: Value,
        pub title_range: Value,
        pub range: Value,
        pub details: Vec<DetailRecord>,
        pub notes: Vec<LineRecord>,
    }
}

record! {
    /// One day of an itinerary: its calendar parts, its line, and its stops.
    pub(crate) struct DayRecord {
        ..parts: DayParts,
        ..line: LineRecord,
        pub weekday: Option<usize>,
        pub weekday_range: Value,
        pub date_range: Value,
        pub places: Option<String>,
        pub forecast: Value,
        pub stops: Vec<StopRecord>,
    }
}

record! {
    /// A URL, split the way a link module reads it.
    pub(crate) struct UrlRecord {
        pub raw: String,
        pub host: String,
        pub path: String,
        pub scheme: String,
    }
}
impl From<&url::Url> for UrlRecord {
    fn from(url: &url::Url) -> Self {
        Self {
            raw: url.to_string(),
            host: url.host_str().unwrap_or_default().into(),
            path: url.path().into(),
            scheme: url.scheme().into(),
        }
    }
}

record! {
    /// Everything a link module's hook is handed about one URL.
    pub(crate) struct LinkContextRecord {
        pub url: UrlRecord,
        /// False in the browser, where a refresh cannot run a program.
        pub native: bool,
        pub cached: Value,
        pub fetched_at: Option<DateTime<FixedOffset>>,
    }
}

/// What a link module's `refresh` hook asked the host to run. The request is
/// data: nothing here is executed until the user asks for a refresh.
pub(crate) struct RefreshRecord {
    pub title: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub format: Option<String>,
}
impl FromValue for RefreshRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            program: fields.required("program")?,
            title: fields.present("title")?,
            args: fields.list("args")?,
            env: fields.present("env")?,
            format: fields.present("format")?,
        })
    }
}
