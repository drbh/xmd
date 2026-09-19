//! The typed boundary between Rust and the .wtf modules.
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
use crate::engine::Value;
use crate::error::{EvalError, EvalResult};
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

/// Reads the fields of a record a module returned.
#[derive(Clone, Copy)]
pub struct Fields<'a> {
    fields: &'a BTreeMap<String, Value>,
}
impl<'a> Fields<'a> {
    pub fn new(value: &'a Value) -> EvalResult<Self> {
        Self::expect(value, EvalError::Expected("a record"))
    }
    /// The same, for the call sites that name the record in their own words.
    pub fn expect(value: &'a Value, error: impl Into<EvalError>) -> EvalResult<Self> {
        match value {
            Value::Record(fields) => Ok(Self { fields }),
            _ => Err(error.into()),
        }
    }
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.fields.get(key)
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
    /// The same, with one error for every way it can go wrong.
    pub fn required_or<T: FromValue>(
        &self,
        key: &str,
        error: impl Into<EvalError>,
    ) -> EvalResult<T> {
        self.required(key).map_err(|_| error.into())
    }
    /// Absent or `Null` reads as nothing.
    pub fn optional<T: FromValue>(&self, key: &str) -> EvalResult<Option<T>> {
        match self.fields.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => T::from_value(value).map(Some),
        }
    }
    /// Absent reads as nothing; anything present, `Null` included, must decode.
    pub fn present<T: FromValue>(&self, key: &str) -> EvalResult<Option<T>> {
        self.fields.get(key).map(T::from_value).transpose()
    }
    /// A required field holding a list of `T`.
    pub fn list<T: FromValue>(&self, key: &str) -> EvalResult<Vec<T>> {
        self.required(key)
    }
}

/// Where a timer stands, as `timer.wtf` reads and writes it.
#[derive(Clone, Copy, Debug)]
pub struct TimerRecord {
    pub limit: Option<i64>,
    pub elapsed: i64,
    pub started: Option<DateTime<FixedOffset>>,
    pub idle: bool,
}
impl TimerRecord {
    const LIMIT: &'static str = "limit";
    const ELAPSED: &'static str = "elapsed";
    const STARTED: &'static str = "started";
    const IDLE: &'static str = "idle";
}
impl ToValue for TimerRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::LIMIT.into(), self.limit.to_value()),
            (Self::ELAPSED.into(), self.elapsed.to_value()),
            (Self::STARTED.into(), self.started.to_value()),
            (Self::IDLE.into(), self.idle.to_value()),
        ]))
    }
}
impl FromValue for TimerRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::expect(value, "Timer constructor must return a record")?;
        Ok(Self {
            limit: fields.required_or(Self::LIMIT, "Invalid timer limit")?,
            elapsed: fields.required_or(Self::ELAPSED, "Invalid timer elapsed time")?,
            started: fields.required_or(Self::STARTED, "Invalid timer timestamp")?,
            idle: fields.required_or(Self::IDLE, "Invalid timer state")?,
        })
    }
}

/// One side of a linear constraint, as `plan.wtf` receives it: a constant, the
/// coefficients by variable name, and one value carrying the form's unit.
pub struct FormRecord {
    pub constant: f64,
    pub terms: BTreeMap<String, f64>,
    pub unit: Value,
}
impl FormRecord {
    const CONSTANT: &'static str = "constant";
    const TERMS: &'static str = "terms";
    const UNIT: &'static str = "unit";
}
impl ToValue for FormRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::CONSTANT.into(), self.constant.to_value()),
            (Self::TERMS.into(), self.terms.to_value()),
            (Self::UNIT.into(), self.unit.clone()),
        ]))
    }
}

/// One decision column the plan solves for.
pub struct DecisionRecord {
    pub name: String,
    pub kind: String,
}
impl DecisionRecord {
    const NAME: &'static str = "name";
    const KIND: &'static str = "kind";
}
impl ToValue for DecisionRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::NAME.into(), self.name.to_value()),
            (Self::KIND.into(), self.kind.to_value()),
        ]))
    }
}

/// One named constraint on the way into `plan.solve_model`.
pub struct ConstraintInput {
    pub name: String,
    pub lhs: FormRecord,
    pub rhs: FormRecord,
    pub op: String,
}
impl ConstraintInput {
    const NAME: &'static str = "name";
    const LHS: &'static str = "lhs";
    const RHS: &'static str = "rhs";
    const OP: &'static str = "op";
}
impl ToValue for ConstraintInput {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::NAME.into(), self.name.to_value()),
            (Self::LHS.into(), self.lhs.to_value()),
            (Self::RHS.into(), self.rhs.to_value()),
            (Self::OP.into(), self.op.to_value()),
        ]))
    }
}

/// The whole model `plan.solve_model` is handed.
pub struct PlanInput {
    pub goal: String,
    pub names: Vec<String>,
    pub decisions: Vec<DecisionRecord>,
    pub objective: FormRecord,
    pub constraints: Vec<ConstraintInput>,
}
impl PlanInput {
    const GOAL: &'static str = "goal";
    const NAMES: &'static str = "names";
    const DECISIONS: &'static str = "decisions";
    const OBJECTIVE: &'static str = "objective";
    const CONSTRAINTS: &'static str = "constraints";
}
impl ToValue for PlanInput {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::GOAL.into(), self.goal.to_value()),
            (Self::NAMES.into(), self.names.to_value()),
            (Self::DECISIONS.into(), self.decisions.to_value()),
            (Self::OBJECTIVE.into(), self.objective.to_value()),
            (Self::CONSTRAINTS.into(), self.constraints.to_value()),
        ]))
    }
}

/// What one variable came back as.
pub struct VariableRecord {
    pub name: String,
    pub value: Value,
}
impl VariableRecord {
    const NAME: &'static str = "name";
    const VALUE: &'static str = "value";
}
impl FromValue for VariableRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            name: fields.required::<Value>(Self::NAME)?.display(),
            value: fields.required(Self::VALUE)?,
        })
    }
}

/// How one constraint fared, as `plan.solve_model` reports it.
pub struct ConstraintOutcome {
    pub name: String,
    pub op: String,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
}
impl ConstraintOutcome {
    const NAME: &'static str = "name";
    const OP: &'static str = "op";
    const LHS: &'static str = "lhs";
    const RHS: &'static str = "rhs";
    const SLACK: &'static str = "slack";
    const BINDING: &'static str = "binding";
}
impl FromValue for ConstraintOutcome {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            name: fields.required::<Value>(Self::NAME)?.display(),
            op: fields.required::<Value>(Self::OP)?.display(),
            lhs: fields.required(Self::LHS)?,
            rhs: fields.required(Self::RHS)?,
            slack: fields.required(Self::SLACK)?,
            binding: matches!(fields.required::<Value>(Self::BINDING)?, Value::Bool(true)),
        })
    }
}

/// The solved plan coming back out of `plan.solve_model`.
pub struct SolutionRecord {
    pub variables: Vec<VariableRecord>,
    pub constraints: Vec<ConstraintOutcome>,
    /// The chosen cell per decision column, read by row-variable name.
    pub rows: Value,
    pub objective: Value,
}
impl SolutionRecord {
    const VARIABLES: &'static str = "variables";
    const CONSTRAINTS: &'static str = "constraints";
    const ROWS: &'static str = "rows";
    const OBJECTIVE: &'static str = "objective";
}
impl FromValue for SolutionRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            variables: fields.list(Self::VARIABLES)?,
            constraints: fields.list(Self::CONSTRAINTS)?,
            rows: fields.required(Self::ROWS)?,
            objective: fields.required(Self::OBJECTIVE)?,
        })
    }
}

/// One decision cell a plan filled in, with the source geometry an editor needs
/// to draw and rewrite it.
pub struct CellRecord {
    pub value: Value,
    pub label: String,
    pub document: String,
    pub source: String,
    pub line: usize,
    pub anchor: Value,
    pub range: Value,
    pub width: usize,
}
impl CellRecord {
    const VALUE: &'static str = "value";
    const LABEL: &'static str = "label";
    const DOCUMENT: &'static str = "document";
    const SOURCE: &'static str = "source";
    const LINE: &'static str = "line";
    const ANCHOR: &'static str = "anchor";
    const RANGE: &'static str = "range";
    const WIDTH: &'static str = "width";
}
impl ToValue for CellRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::VALUE.into(), self.value.clone()),
            (Self::LABEL.into(), self.label.to_value()),
            (Self::DOCUMENT.into(), self.document.to_value()),
            (Self::SOURCE.into(), self.source.to_value()),
            (Self::LINE.into(), self.line.to_value()),
            (Self::ANCHOR.into(), self.anchor.clone()),
            (Self::RANGE.into(), self.range.clone()),
            (Self::WIDTH.into(), self.width.to_value()),
        ]))
    }
}

/// One decision column, with the cells the plan chose for it.
pub struct ColumnRecord {
    pub name: String,
    pub cells: Vec<CellRecord>,
}
impl ColumnRecord {
    const NAME: &'static str = "name";
    const CELLS: &'static str = "cells";
}
impl ToValue for ColumnRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::NAME.into(), self.name.to_value()),
            (Self::CELLS.into(), self.cells.to_value()),
        ]))
    }
}

/// A solved constraint on its way to `plan.wtf`, with the source geometry when
/// the plan it came from is still in the workspace.
pub struct ConstraintRecord {
    pub name: String,
    pub op: String,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
    pub anchor: Option<Value>,
    pub range: Option<Value>,
}
impl ConstraintRecord {
    const NAME: &'static str = "name";
    const OP: &'static str = "op";
    const LHS: &'static str = "lhs";
    const RHS: &'static str = "rhs";
    const SLACK: &'static str = "slack";
    const BINDING: &'static str = "binding";
    const ANCHOR: &'static str = "anchor";
    const RANGE: &'static str = "range";
}
impl ToValue for ConstraintRecord {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::from([
            (Self::NAME.into(), self.name.to_value()),
            (Self::OP.into(), self.op.to_value()),
            (Self::LHS.into(), self.lhs.clone()),
            (Self::RHS.into(), self.rhs.clone()),
            (Self::SLACK.into(), self.slack.clone()),
            (Self::BINDING.into(), self.binding.to_value()),
        ]);
        // Only a plan still present in the workspace has a place in the source.
        if let Some(anchor) = &self.anchor {
            fields.insert(Self::ANCHOR.into(), anchor.clone());
        }
        if let Some(range) = &self.range {
            fields.insert(Self::RANGE.into(), range.clone());
        }
        Value::Record(fields)
    }
}

/// A whole solved plan, as `plan.wtf` renders it. Presentation policy is the
/// module's; this is the typed result plus where it came from.
pub struct PlanRecord {
    pub goal: String,
    pub objective: Value,
    /// Order matters: it is also the variable order the module reports.
    pub variables: Vec<(String, Value)>,
    pub constraints: Vec<ConstraintRecord>,
    pub columns: Vec<ColumnRecord>,
}
impl PlanRecord {
    const GOAL: &'static str = "goal";
    const OBJECTIVE: &'static str = "objective";
    const VARIABLES: &'static str = "variables";
    const VARIABLE_ORDER: &'static str = "variable_order";
    const CONSTRAINTS: &'static str = "constraints";
    const COLUMNS: &'static str = "columns";
}
impl ToValue for PlanRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::GOAL.into(), self.goal.to_value()),
            (Self::OBJECTIVE.into(), self.objective.clone()),
            (
                Self::VARIABLES.into(),
                Value::Record(self.variables.iter().cloned().collect()),
            ),
            (
                Self::VARIABLE_ORDER.into(),
                Value::List(self.variables.iter().map(|(n, _)| n.to_value()).collect()),
            ),
            (Self::CONSTRAINTS.into(), self.constraints.to_value()),
            (Self::COLUMNS.into(), self.columns.to_value()),
        ]))
    }
}

/// Where a line of an itinerary sits, for every record that names one.
pub struct LineRecord {
    pub line: usize,
    pub raw: String,
    pub line_range: Value,
    pub anchor: Value,
}
impl LineRecord {
    const LINE: &'static str = "line";
    const RAW: &'static str = "raw";
    const LINE_RANGE: &'static str = "line_range";
    const ANCHOR: &'static str = "anchor";
    fn insert(&self, fields: &mut BTreeMap<String, Value>) {
        fields.insert(Self::LINE.into(), self.line.to_value());
        fields.insert(Self::RAW.into(), self.raw.to_value());
        fields.insert(Self::LINE_RANGE.into(), self.line_range.clone());
        fields.insert(Self::ANCHOR.into(), self.anchor.clone());
    }
}
impl ToValue for LineRecord {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        self.insert(&mut fields);
        Value::Record(fields)
    }
}

/// The calendar parts a day line spells out, before any year is carried forward.
pub struct DayParts {
    pub month: usize,
    pub day: usize,
    pub year: Option<f64>,
}
impl DayParts {
    const MONTH: &'static str = "month";
    const DAY: &'static str = "day";
    const YEAR: &'static str = "year";
    fn insert(&self, fields: &mut BTreeMap<String, Value>) {
        fields.insert(Self::MONTH.into(), self.month.to_value());
        fields.insert(Self::DAY.into(), self.day.to_value());
        fields.insert(Self::YEAR.into(), self.year.to_value());
    }
}
impl ToValue for DayParts {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        self.insert(&mut fields);
        Value::Record(fields)
    }
}

/// What kind of stop a marker names.
pub struct KindRecord {
    pub marker: String,
    pub name: String,
}
impl KindRecord {
    const MARKER: &'static str = "marker";
    const NAME: &'static str = "name";
}
impl ToValue for KindRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::MARKER.into(), self.marker.to_value()),
            (Self::NAME.into(), self.name.to_value()),
        ]))
    }
}

/// One `key: value` line hanging off a stop.
pub struct DetailRecord {
    pub line: LineRecord,
    pub key: String,
    pub value: String,
}
impl DetailRecord {
    const KEY: &'static str = "key";
    const VALUE: &'static str = "value";
}
impl ToValue for DetailRecord {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        self.line.insert(&mut fields);
        fields.insert(Self::KEY.into(), self.key.to_value());
        fields.insert(Self::VALUE.into(), self.value.to_value());
        Value::Record(fields)
    }
}

/// One stop of an itinerary day, with every span the editor draws on.
pub struct StopRecord {
    pub line: LineRecord,
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
impl StopRecord {
    const KIND: &'static str = "kind";
    const TIME: &'static str = "time";
    const TWELVE_HOUR: &'static str = "twelve_hour";
    const TITLE: &'static str = "title";
    const TIME_RANGE: &'static str = "time_range";
    const TITLE_RANGE: &'static str = "title_range";
    const RANGE: &'static str = "range";
    const DETAILS: &'static str = "details";
    const NOTES: &'static str = "notes";
}
impl ToValue for StopRecord {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        self.line.insert(&mut fields);
        fields.extend([
            (Self::KIND.into(), self.kind.to_value()),
            (Self::TIME.into(), self.time.to_value()),
            (Self::TWELVE_HOUR.into(), self.twelve_hour.to_value()),
            (Self::TITLE.into(), self.title.to_value()),
            (Self::TIME_RANGE.into(), self.time_range.clone()),
            (Self::TITLE_RANGE.into(), self.title_range.clone()),
            (Self::RANGE.into(), self.range.clone()),
            (Self::DETAILS.into(), self.details.to_value()),
            (Self::NOTES.into(), self.notes.to_value()),
        ]);
        Value::Record(fields)
    }
}

/// One day of an itinerary: its calendar parts, its line, and its stops.
pub struct DayRecord {
    pub parts: DayParts,
    pub line: LineRecord,
    pub weekday: Option<usize>,
    pub weekday_range: Value,
    pub date_range: Value,
    pub places: Option<String>,
    pub forecast: Value,
    pub stops: Vec<StopRecord>,
}
impl DayRecord {
    const WEEKDAY: &'static str = "weekday";
    const WEEKDAY_RANGE: &'static str = "weekday_range";
    const DATE_RANGE: &'static str = "date_range";
    const PLACES: &'static str = "places";
    const FORECAST: &'static str = "forecast";
    const STOPS: &'static str = "stops";
}
impl ToValue for DayRecord {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::new();
        self.parts.insert(&mut fields);
        self.line.insert(&mut fields);
        fields.extend([
            (Self::WEEKDAY.into(), self.weekday.to_value()),
            (Self::WEEKDAY_RANGE.into(), self.weekday_range.clone()),
            (Self::DATE_RANGE.into(), self.date_range.clone()),
            (Self::PLACES.into(), self.places.to_value()),
            (Self::FORECAST.into(), self.forecast.clone()),
            (Self::STOPS.into(), self.stops.to_value()),
        ]);
        Value::Record(fields)
    }
}

/// A URL, split the way a link module reads it.
pub struct UrlRecord {
    pub raw: String,
    pub host: String,
    pub path: String,
    pub scheme: String,
}
impl UrlRecord {
    const RAW: &'static str = "raw";
    const HOST: &'static str = "host";
    const PATH: &'static str = "path";
    const SCHEME: &'static str = "scheme";
}
impl From<&lsp_types::Url> for UrlRecord {
    fn from(url: &lsp_types::Url) -> Self {
        Self {
            raw: url.to_string(),
            host: url.host_str().unwrap_or_default().into(),
            path: url.path().into(),
            scheme: url.scheme().into(),
        }
    }
}
impl ToValue for UrlRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::RAW.into(), self.raw.to_value()),
            (Self::HOST.into(), self.host.to_value()),
            (Self::PATH.into(), self.path.to_value()),
            (Self::SCHEME.into(), self.scheme.to_value()),
        ]))
    }
}

/// Everything a link module's hook is handed about one URL.
pub struct LinkContextRecord {
    pub url: UrlRecord,
    /// False in the browser, where a refresh cannot run a program.
    pub native: bool,
    pub cached: Value,
    pub fetched_at: Option<DateTime<FixedOffset>>,
}
impl LinkContextRecord {
    const URL: &'static str = "url";
    const NATIVE: &'static str = "native";
    const CACHED: &'static str = "cached";
    const FETCHED_AT: &'static str = "fetched_at";
}
impl ToValue for LinkContextRecord {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            (Self::URL.into(), self.url.to_value()),
            (Self::NATIVE.into(), self.native.to_value()),
            (Self::CACHED.into(), self.cached.clone()),
            (Self::FETCHED_AT.into(), self.fetched_at.to_value()),
        ]))
    }
}

/// What a link module's `refresh` hook asked the host to run. The request is
/// data: nothing here is executed until the user asks for a refresh.
pub struct RefreshRecord {
    pub title: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub format: Option<String>,
}
impl RefreshRecord {
    const TITLE: &'static str = "title";
    const PROGRAM: &'static str = "program";
    const ARGS: &'static str = "args";
    const ENV: &'static str = "env";
    const FORMAT: &'static str = "format";
}
impl FromValue for RefreshRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            program: fields.required(Self::PROGRAM)?,
            title: fields.present(Self::TITLE)?,
            args: fields.list(Self::ARGS)?,
            env: fields.present(Self::ENV)?,
            format: fields.present(Self::FORMAT)?,
        })
    }
}
