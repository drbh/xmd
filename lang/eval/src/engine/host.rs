//! Host objects: the half of [`Value`] the language does not compute.
//!
//! A note computes with language values — numbers, money, durations, dates,
//! text, codes, lists and records. It cannot build a forecast, a checklist, a
//! timer, a table, a plan, an imported note or a link target; those are
//! objects some host owns (the lookup store, the document, the timer module,
//! the solver, the filesystem) and the language only reads from. Every kind of
//! reading is one method of [`HostObject`], so the generic sites —
//! `Value::kind`, `Value::display`, `Value::property`, `QueryValue::from_value`,
//! completion's property list and the symbol hover — dispatch once instead of
//! carrying an arm per object.
//!
//! The impls sit here rather than beside each struct because the split is the
//! point: one file answers "what can a note see of a host object?". Each impl
//! forwards to the object's own inherent methods, and the payload of a variant
//! answers for itself, so a checklist is its `Vec<TaskKey>` — no variant needs
//! a wrapper type it does not already have.
use super::{Engine, TaskKey, Value, ValueType};
use crate::{
    error::{EvalError, EvalResult},
    link_features_impl::LinkFeatures,
    lookups_impl::Forecast,
    plans_impl::PlanValue,
    resources_impl::{Resource, ResourcePresenting},
    tables_impl::TableValue,
    timers_impl::Timer,
};
use std::path::{Path, PathBuf};

pub trait HostObject {
    /// The kind a note sees. A timer names itself by what it was built with.
    fn kind(&self) -> ValueType;
    /// The human-readable label a hover, inlay or query result shows.
    fn display(&self) -> String;
    /// Fields the object answers on its own. A namespace's members, a
    /// resource's link data and a checklist's counts need the engine, so they
    /// stay in [`Engine`] and fall through to this error.
    fn property(&self, key: &str) -> EvalResult<Value> {
        Err(EvalError::UnknownField {
            key: key.into(),
            on: Some(self.kind()),
        })
    }
    /// The names completion offers after `value.`; `links` is what a resource
    /// target's registered features add to its own.
    fn fields(&self, links: LinkFeatures<'_>) -> Vec<String> {
        let _ = links;
        vec![]
    }
    /// The object as plain language values, which is the shape a query record
    /// and its JSON hold. `None` keeps the value itself as the scalar: a
    /// namespace has no shape but its own.
    fn query(&self) -> Option<Value> {
        None
    }
    /// Hover detail beyond the name, kind and display line every symbol gets.
    fn hover(&self, engine: &mut Engine<'_>, path: &Path) -> Option<String> {
        let _ = (engine, path);
        None
    }
}
impl Value {
    /// The object behind a host value, or `None` for a value the language
    /// computes. This is the one place every variant is sorted into the two
    /// halves, so it is spelled out rather than using a wildcard: a new kind
    /// has to say which half it belongs to, and `kind` and `display` may then
    /// treat anything left over as impossible.
    pub fn host(&self) -> Option<&dyn HostObject> {
        match self {
            Self::Forecast(forecast) => Some(forecast),
            Self::Tasks(tasks) => Some(tasks),
            Self::Namespace(note) => Some(note),
            Self::Resource(resource) => Some(resource),
            Self::Timer(timer) => Some(&**timer),
            Self::Table(table) => Some(&**table),
            Self::Plan(plan) => Some(&**plan),
            Self::Null
            | Self::List(_)
            | Self::Record(_)
            | Self::Function(_)
            | Self::Number(_)
            | Self::Count(_)
            | Self::Money(..)
            | Self::Ratio(_)
            | Self::Duration(_)
            | Self::Date(_)
            | Self::DateTime(_)
            | Self::Bool(_)
            | Self::Text(_)
            | Self::Code(_) => None,
        }
    }
}
fn record<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Record(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn optional(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}
impl HostObject for Forecast {
    fn kind(&self) -> ValueType {
        ValueType::Forecast
    }
    fn display(&self) -> String {
        Forecast::display(self)
    }
    fn property(&self, key: &str) -> EvalResult<Value> {
        Forecast::property(self, key)
    }
    fn query(&self) -> Option<Value> {
        Some(record([
            ("high", Value::Number(self.high)),
            ("low", Value::Number(self.low)),
            ("summary", Value::Text(self.summary.clone())),
            ("rain", optional(self.precipitation.map(Value::Ratio))),
            (
                "unit",
                Value::Text(if self.fahrenheit { "F" } else { "C" }.into()),
            ),
        ]))
    }
}
/// A checklist: the tasks under one heading, which only the counting built-ins
/// and `@after` read, and which the engine resolves against the document.
impl HostObject for Vec<TaskKey> {
    fn kind(&self) -> ValueType {
        ValueType::Checklist
    }
    fn display(&self) -> String {
        format!("{} tasks", self.len())
    }
    fn query(&self) -> Option<Value> {
        Some(Value::List(
            self.iter()
                .map(|(path, index)| {
                    record([
                        ("path", Value::Text(path.to_string_lossy().into())),
                        ("task_index", Value::Count(*index)),
                    ])
                })
                .collect(),
        ))
    }
    fn hover(&self, engine: &mut Engine<'_>, _path: &Path) -> Option<String> {
        let done = self
            .iter()
            .filter(|(path, index)| engine.task_done(path, *index))
            .count();
        Some(
            engine
                .call_module(
                    "task",
                    "checklist",
                    vec![Value::Count(done), Value::Count(self.len())],
                )
                .map(|v| v.display())
                .unwrap_or_else(|e| format!("\n\n{e}")),
        )
    }
}
/// An imported note. Its members are the note's definitions, which the engine
/// resolves lazily by name, so it answers no fields of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Namespace(pub PathBuf);
impl Namespace {
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl HostObject for Namespace {
    fn kind(&self) -> ValueType {
        ValueType::Namespace
    }
    fn display(&self) -> String {
        format!("import(\"{}\")", self.0.display())
    }
}
impl HostObject for Resource {
    fn kind(&self) -> ValueType {
        ValueType::Resource
    }
    fn display(&self) -> String {
        self.target.clone()
    }
    fn fields(&self, links: LinkFeatures<'_>) -> Vec<String> {
        let mut names = vec!["url".to_owned()];
        names.extend(links.property_names(&self.target));
        if !self.target.starts_with("http") && !self.target.starts_with("geo:") {
            names.push("exists".into());
        }
        names
    }
    fn query(&self) -> Option<Value> {
        Some(record([("target", Value::Text(self.target.clone()))]))
    }
    fn hover(&self, engine: &mut Engine<'_>, path: &Path) -> Option<String> {
        Some(format!("\n\n{}", self.presentation(engine, path).hover))
    }
}
impl HostObject for Timer {
    fn kind(&self) -> ValueType {
        match self.limit {
            Some(_) => ValueType::Countdown,
            None => ValueType::Stopwatch,
        }
    }
    fn display(&self) -> String {
        Timer::display(self)
    }
    fn property(&self, key: &str) -> EvalResult<Value> {
        Timer::property(self, key)
    }
    fn fields(&self, _links: LinkFeatures<'_>) -> Vec<String> {
        let mut names = vec!["elapsed", "running", "done", "state"];
        if self.limit.is_some() {
            names.extend(["remaining", "duration"]);
        }
        names.into_iter().map(str::to_owned).collect()
    }
    fn query(&self) -> Option<Value> {
        Some(record([
            ("state", Value::Text(self.state().as_str().into())),
            ("elapsed", Value::Duration(self.elapsed)),
            ("limit", optional(self.limit.map(Value::Duration))),
            ("started", optional(self.started.map(Value::DateTime))),
        ]))
    }
    fn hover(&self, _engine: &mut Engine<'_>, _path: &Path) -> Option<String> {
        Some(Timer::hover(self))
    }
}
impl HostObject for TableValue {
    fn kind(&self) -> ValueType {
        ValueType::Table
    }
    fn display(&self) -> String {
        format!("{} rows · {} columns", self.rows.len(), self.columns.len())
    }
    fn query(&self) -> Option<Value> {
        Some(Value::List(
            self.rows
                .iter()
                .map(|row| {
                    Value::Record(
                        self.columns
                            .iter()
                            .cloned()
                            .zip(row.iter().cloned())
                            .collect(),
                    )
                })
                .collect(),
        ))
    }
}
impl HostObject for PlanValue {
    fn kind(&self) -> ValueType {
        ValueType::Plan
    }
    fn display(&self) -> String {
        self.objective.display()
    }
    fn property(&self, key: &str) -> EvalResult<Value> {
        PlanValue::property(self, key)
    }
    fn fields(&self, _links: LinkFeatures<'_>) -> Vec<String> {
        self.property_names()
    }
    fn query(&self) -> Option<Value> {
        Some(record([
            ("goal", Value::Text(self.goal.keyword().into())),
            ("objective", self.objective.clone()),
            (
                "variables",
                Value::Record(self.variables.iter().cloned().collect()),
            ),
            (
                "constraints",
                Value::List(
                    self.constraints
                        .iter()
                        .map(|c| {
                            record([
                                ("name", Value::Text(c.name.clone())),
                                ("op", Value::Text(c.op.as_str().into())),
                                ("lhs", c.lhs.clone()),
                                ("rhs", c.rhs.clone()),
                                ("slack", c.slack.clone()),
                                ("binding", Value::Bool(c.binding)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]))
    }
}
