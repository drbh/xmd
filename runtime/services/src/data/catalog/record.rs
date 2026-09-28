//! The lazy `Record` machinery and the parts every catalog record shares.
use super::RecordKind;
use super::value as q;
use chrono::NaiveDate;
use lang::common::Span;
use lang::eval::engine::{Engine, Value};
use lang::eval::resources::ResourcePresenting;
use lang::eval::{RecordFields, Symbol, ToValue, Workspace, record};
use lsp_types::{Position, Range};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

record! {
    /// Where a record was found. Every record carries one under `source`.
    #[derive(Clone, Debug)]
    pub(super) struct SourceRef {
        path: String,
        uri: String,
        line: usize,
        range: Value,
    }
}
impl SourceRef {
    pub(super) fn new(ws: &Workspace, path: &Path, span: Span) -> Self {
        let mut uri = lang::common::file_url(path)
            .map(|u| u.to_string())
            .unwrap_or_default();
        uri.push_str(&format!("#L{}", span.line + 1));
        Self {
            path: path.to_string_lossy().into(),
            uri,
            line: span.line + 1,
            range: q::from_json(json!(span.range(&ws.documents()[path].text))),
        }
    }
}

impl SourceRef {
    pub(super) fn set_range(&mut self, range: Range) {
        self.range = q::from_json(json!(range));
    }
}

record! {
    /// What every record carries, whatever its kind.
    #[derive(Clone, Debug)]
    pub(super) struct Base {
        pub(super) kind: RecordKind,
        pub(super) title: String,
        pub(super) line: usize,
        pub(super) anchor: Value,
        pub(super) source: SourceRef,
        pub(super) errors: Vec<String>,
    }
}
impl Base {
    pub(super) fn new(
        ws: &Workspace,
        path: &Path,
        line: usize,
        kind: RecordKind,
        title: impl Into<String>,
    ) -> Self {
        let doc = &ws.documents()[path];
        Self {
            kind,
            title: title.into(),
            line,
            anchor: q::from_json(json!(doc.line_end(line))),
            source: SourceRef::new(ws, path, Span::new(line, 0, doc.line(line).len())),
            errors: Vec::new(),
        }
    }
}
impl Base {
    /// Where an editor places this record's inlay, when not at the line's end.
    pub(super) fn set_anchor(&mut self, position: Position) {
        self.anchor = q::from_json(json!(position));
    }
}
impl ToValue for RecordKind {
    fn to_value(&self) -> Value {
        q::text(self.as_str())
    }
}

/// The scheduling attributes a task reads, and the fields they land in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum When {
    Due,
    Scheduled,
    At,
    Estimate,
}
impl When {
    pub(super) const ALL: [When; 4] = [Self::Due, Self::Scheduled, Self::At, Self::Estimate];
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Scheduled => "scheduled",
            Self::At => "at",
            Self::Estimate => "estimate",
        }
    }
}
impl ToValue for When {
    fn to_value(&self) -> Value {
        q::text(self.as_str())
    }
}

record! {
    /// Tasks, events and stops share one shape, so a query can sort them
    /// together. The first four are named after [`When`].
    #[derive(Clone, Debug)]
    pub(super) struct Scheduling {
        pub(super) due: Value,
        pub(super) scheduled: Value,
        pub(super) at: Value,
        pub(super) estimate: Value,
        pub(super) at_date: Value,
        pub(super) parent: Value,
        pub(super) tags: Vec<String>,
        pub(super) blocked_by: Vec<String>,
        pub(super) done: bool,
        pub(super) leaf: bool,
    }
}
impl Default for Scheduling {
    fn default() -> Self {
        Self {
            due: Value::Null,
            scheduled: Value::Null,
            at: Value::Null,
            at_date: Value::Null,
            estimate: Value::Null,
            parent: Value::Null,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            done: false,
            leaf: true,
        }
    }
}
impl Scheduling {
    pub(super) fn set(&mut self, when: When, value: Value) {
        match when {
            When::Due => self.due = value,
            When::Scheduled => self.scheduled = value,
            When::At => self.at = value,
            When::Estimate => self.estimate = value,
        }
    }
}

record! {
    /// One `@due`/`@scheduled`/`@at` attribute as the task read it.
    #[derive(Clone, Debug)]
    pub(super) struct ScheduleEntry {
        pub(super) key: When,
        pub(super) value: Value,
        pub(super) error: Value,
    }
}
record! {
    #[derive(Clone, Debug)]
    pub(super) struct ChildTask {
        pub(super) line: usize,
        pub(super) done: bool,
    }
}

record! {
    /// Where a running timer was started, when that is another note's definition.
    #[derive(Clone, Debug)]
    pub(super) struct TimerOrigin {
        pub(super) document: String,
        pub(super) name: String,
    }
}

record! {
    /// Calculations, bracketed references and table cells all project one
    /// evaluated expression; only the surrounding fields differ.
    #[derive(Clone, Debug)]
    pub(super) struct Expression {
        expression: String,
        value: Value,
        type_name: Value => "type",
        display: Value,
    }
}
impl Expression {
    /// Returns the projection and the errors that belong on the record's base.
    pub(super) fn new(expression: &str, value: Result<Value, String>) -> (Self, Vec<String>) {
        match value {
            Ok(v) => (
                Self {
                    expression: expression.into(),
                    type_name: q::text(v.type_name()),
                    display: q::text(v.display()),
                    value: q::query_value(v),
                },
                Vec::new(),
            ),
            Err(e) => (
                Self {
                    expression: expression.into(),
                    value: Value::Null,
                    type_name: Value::Null,
                    display: Value::Null,
                },
                vec![e],
            ),
        }
    }
}

record! {
    /// How a link or resource wants to be shown, asked for only when read.
    #[derive(Clone, Debug)]
    struct Presentation {
        label: String,
        hover: String,
        known: bool,
    }
}

/// Field names the lazy accessors read back off a record they did not build.
pub(super) const NAME: &str = "name";
pub(super) const EXPRESSION: &str = "expression";
pub(super) const PROPERTY: &str = "property";

/// Fields a record only produces when they are read: evaluating a definition, or
/// asking the workspace for a link's presentation or a name's hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum LazyField {
    Presentation,
    Hover,
    Value,
    Type,
    Solution,
    Errors,
    Display,
}
impl LazyField {
    /// `as_str` stays a hand-written `const fn`: several call sites build
    /// `const` arrays from it, which strum's string conversions cannot do.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Presentation => "presentation",
            Self::Hover => "hover",
            Self::Value => "value",
            Self::Type => "type",
            Self::Solution => "solution",
            Self::Errors => "errors",
            Self::Display => "display",
        }
    }
}

/// Definitions are evaluated only when their value, type, solution or errors are read.
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub fields: BTreeMap<String, Value>,
    pub path: PathBuf,
    pub(super) deferred: Option<Symbol>,
    pub(super) resource: Option<lang::eval::resources::Resource>,
}
impl Record {
    pub(crate) fn projected(path: PathBuf, fields: BTreeMap<String, Value>) -> Self {
        Self {
            path,
            fields,
            deferred: None,
            resource: None,
        }
    }
    pub(super) fn typed(path: &Path, record: impl RecordFields) -> Self {
        Self::projected(path.into(), record.fields())
    }
    /// Whether a name binds on this record, counting the lazily produced hover.
    pub(crate) fn has(&self, name: &str) -> bool {
        self.fields.contains_key(name)
            || matches!(name.parse(), Ok(LazyField::Hover)) && self.fields.contains_key(NAME)
    }
    pub(crate) fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<Value, String> {
        match key.parse::<LazyField>() {
            Ok(LazyField::Presentation) => return Ok(self.presentation(engine)),
            Ok(LazyField::Hover) => {
                if let Some(hover) = self.hover(engine) {
                    return Ok(hover);
                }
            }
            Ok(
                LazyField::Value
                | LazyField::Type
                | LazyField::Solution
                | LazyField::Errors
                | LazyField::Display,
            ) => self.evaluate(engine),
            Err(_) => {}
        }
        self.fields
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Unknown field '{key}'"))
    }
    fn presentation(&mut self, engine: &mut Engine<'_>) -> Value {
        self.evaluate(engine);
        let Some(resource) = &self.resource else {
            return Value::Null;
        };
        let view = resource.presentation(engine, &self.path);
        engine.mark_time_dependent(view.time_dependent);
        Value::Record(
            Presentation {
                label: view.label,
                hover: view.hover,
                known: view.known_link,
            }
            .fields(),
        )
    }
    /// A named record hovers as its symbol does; a property reference as itself.
    fn hover(&self, engine: &mut Engine<'_>) -> Option<Value> {
        let Some(Value::Text(name)) = self.fields.get(NAME) else {
            return None;
        };
        let expression = self.fields.get(EXPRESSION).cloned().unwrap_or(Value::Null);
        if self.fields.get(PROPERTY).is_some_and(|v| *v != Value::Null) {
            return Some(expression);
        }
        Some(
            engine
                .workspace()
                .resolve(&self.path, name)
                .map(|symbol| {
                    q::text(crate::language::hover::symbol_hover(
                        &engine.request(),
                        &symbol,
                    ))
                })
                .unwrap_or(expression),
        )
    }
    fn evaluate(&mut self, engine: &mut Engine<'_>) {
        let Some(symbol) = self.deferred.take() else {
            return;
        };
        match engine.symbol(&symbol) {
            Ok(value) => {
                if let Value::Resource(resource) = &value {
                    self.resource = Some(resource.clone());
                }
                self.fields
                    .insert(LazyField::Type.as_str().into(), q::text(value.type_name()));
                self.fields
                    .insert(LazyField::Display.as_str().into(), q::text(value.display()));
                let value = q::query_value(match value {
                    Value::Plan(p) => p.record(engine.workspace()),
                    other => other,
                });
                if self.fields.contains_key(LazyField::Solution.as_str()) {
                    self.fields
                        .insert(LazyField::Solution.as_str().into(), value.clone());
                }
                self.fields.insert(LazyField::Value.as_str().into(), value);
            }
            Err(e) => {
                self.fields.insert(
                    LazyField::Errors.as_str().into(),
                    q::strings([e.to_string()]),
                );
            }
        }
    }
    pub(crate) fn materialize(mut self, engine: &mut Engine<'_>) -> Value {
        self.evaluate(engine);
        Value::Record(self.fields)
    }
}
pub(crate) fn source(ws: &Workspace, path: &Path, span: Span) -> Value {
    Value::Record(SourceRef::new(ws, path, span).fields())
}
pub(super) fn date_field(value: Option<NaiveDate>) -> Value {
    value.map(Value::Date).unwrap_or(Value::Null)
}
