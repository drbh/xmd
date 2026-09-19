//! Typed, host-independent workspace records. Reading a catalog never performs I/O.
use crate::{
    document::{Document, Span},
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{NaiveDate, TimeZone};
use lsp_types::{Position, Range};
use serde::{Serialize, Serializer};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub use crate::context::Clock as QueryContext;

/// JSON containers around WTF scalars: units survive filtering, sorting and output.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryValue {
    Null,
    Scalar(Value),
    Array(Vec<QueryValue>),
    Object(BTreeMap<String, QueryValue>),
}
impl QueryValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Scalar(Value::Text(value.into()))
    }
    pub fn boolean(value: bool) -> Self {
        Self::Scalar(Value::Bool(value))
    }
    pub fn count(value: usize) -> Self {
        Self::Scalar(Value::Count(value))
    }
    pub fn object<const N: usize>(fields: [(&str, Self); N]) -> Self {
        Self::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
    pub fn strings(values: impl IntoIterator<Item = String>) -> Self {
        Self::Array(values.into_iter().map(Self::text).collect())
    }
    pub fn from_json(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(v) => Self::boolean(v),
            serde_json::Value::Number(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .map(Self::count)
                .unwrap_or_else(|| Self::Scalar(Value::Number(v.as_f64().unwrap()))),
            serde_json::Value::String(v) => Self::text(v),
            serde_json::Value::Array(v) => {
                Self::Array(v.into_iter().map(Self::from_json).collect())
            }
            serde_json::Value::Object(v) => Self::Object(
                v.into_iter()
                    .map(|(k, v)| (k, Self::from_json(v)))
                    .collect(),
            ),
        }
    }
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::List(values) => Self::Array(values.into_iter().map(Self::from_value).collect()),
            Value::Record(fields) => Self::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| (k, Self::from_value(v)))
                    .collect(),
            ),
            Value::Plan(p) => Self::object([
                ("goal", Self::text(p.goal.keyword())),
                ("objective", Self::from_value(p.objective.clone())),
                (
                    "variables",
                    Self::Object(
                        p.variables
                            .iter()
                            .map(|(k, v)| (k.clone(), Self::from_value(v.clone())))
                            .collect(),
                    ),
                ),
                (
                    "constraints",
                    Self::Array(
                        p.constraints
                            .iter()
                            .map(|c| {
                                Self::object([
                                    ("name", Self::text(&c.name)),
                                    ("op", Self::text(c.op.as_str())),
                                    ("lhs", Self::from_value(c.lhs.clone())),
                                    ("rhs", Self::from_value(c.rhs.clone())),
                                    ("slack", Self::from_value(c.slack.clone())),
                                    ("binding", Self::boolean(c.binding)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
            Value::Table(t) => Self::Array(
                t.rows
                    .iter()
                    .map(|row| {
                        Self::Object(
                            t.columns
                                .iter()
                                .cloned()
                                .zip(row.iter().cloned().map(Self::from_value))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            Value::Timer(t) => Self::object([
                ("state", Self::text(t.state().as_str())),
                ("elapsed", Self::Scalar(Value::Duration(t.elapsed))),
                (
                    "limit",
                    t.limit
                        .map(|n| Self::Scalar(Value::Duration(n)))
                        .unwrap_or(Self::Null),
                ),
                (
                    "started",
                    t.started
                        .map(|d| Self::Scalar(Value::DateTime(d)))
                        .unwrap_or(Self::Null),
                ),
            ]),
            Value::Resource(r) => Self::object([("target", Self::text(r.target))]),
            Value::Forecast(f) => Self::object([
                ("high", Self::Scalar(Value::Number(f.high))),
                ("low", Self::Scalar(Value::Number(f.low))),
                ("summary", Self::text(f.summary)),
                (
                    "rain",
                    f.precipitation
                        .map(|n| Self::Scalar(Value::Ratio(n)))
                        .unwrap_or(Self::Null),
                ),
                ("unit", Self::text(if f.fahrenheit { "F" } else { "C" })),
            ]),
            Value::Tasks(tasks) => Self::Array(
                tasks
                    .into_iter()
                    .map(|(p, i)| {
                        Self::object([
                            ("path", Self::text(p.to_string_lossy())),
                            ("task_index", Self::count(i)),
                        ])
                    })
                    .collect(),
            ),
            scalar => Self::Scalar(scalar),
        }
    }
    pub(crate) fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Scalar(v) => v.clone(),
            Self::Array(values) => Value::List(values.iter().map(Self::value).collect()),
            Self::Object(fields) => {
                Value::Record(fields.iter().map(|(k, v)| (k.clone(), v.value())).collect())
            }
        }
    }
    pub fn json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Scalar(v) => match v {
                Value::Function(_) => json!({"type":"function"}),
                Value::Namespace(path) => json!({"type":"namespace", "path":path}),
                Value::Number(n) => json!(n),
                Value::Count(n) => json!(n),
                Value::Text(s) => json!(s),
                Value::Bool(b) => json!(b),
                Value::Money(amount, currency) => {
                    json!({"type":"money","amount":amount,"currency":currency.as_str()})
                }
                Value::Duration(seconds) => json!({"type":"duration","seconds":seconds}),
                Value::Date(d) => json!({"type":"date","value":d.to_string()}),
                Value::DateTime(d) => json!({"type":"datetime","value":d.to_rfc3339()}),
                Value::Ratio(n) => json!({"type":"ratio","value":n}),
                other => Self::from_value(other.clone()).json(),
            },
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(Self::json).collect())
            }
            Self::Object(values) => serde_json::Value::Object(
                values.iter().map(|(k, v)| (k.clone(), v.json())).collect(),
            ),
        }
    }
    pub fn display(&self) -> String {
        match self {
            Self::Scalar(v) => v.display(),
            Self::Null => "null".into(),
            _ => self.json().to_string(),
        }
    }
    pub fn property(&self, key: &str) -> Result<Self, String> {
        self.value().property(key).map(Self::from_value)
    }
}
impl Serialize for QueryValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.json().serialize(serializer)
    }
}

/// Every catalog record is a struct that knows its own field names: these
/// `fields` implementations are the only place those names are written down.
trait Fields {
    fn fields(self) -> BTreeMap<String, QueryValue>;
}
fn entries<const N: usize>(items: [(&str, QueryValue); N]) -> BTreeMap<String, QueryValue> {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Where a record was found. Every record carries one under `source`.
#[derive(Clone, Debug)]
struct SourceRef {
    path: PathBuf,
    uri: String,
    line: usize,
    range: Range,
}
impl SourceRef {
    fn new(ws: &Workspace, path: &Path, span: Span) -> Self {
        let mut uri = crate::paths::file_url(path)
            .map(|u| u.to_string())
            .unwrap_or_default();
        uri.push_str(&format!("#L{}", span.line + 1));
        Self {
            path: path.into(),
            uri,
            line: span.line + 1,
            range: span.range(&ws.documents[path].text),
        }
    }
}
impl Fields for SourceRef {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("path", QueryValue::text(self.path.to_string_lossy())),
            ("uri", QueryValue::text(self.uri)),
            ("line", QueryValue::count(self.line)),
            ("range", QueryValue::from_json(json!(self.range))),
        ])
    }
}

/// What every record carries, whatever its kind.
#[derive(Clone, Debug)]
struct Base {
    kind: RecordKind,
    title: String,
    line: usize,
    anchor: Position,
    source: SourceRef,
    errors: Vec<String>,
}
impl Base {
    fn new(
        ws: &Workspace,
        path: &Path,
        line: usize,
        kind: RecordKind,
        title: impl Into<String>,
    ) -> Self {
        let doc = &ws.documents[path];
        Self {
            kind,
            title: title.into(),
            line,
            anchor: doc.line_end(line),
            source: SourceRef::new(ws, path, Span::new(line, 0, doc.line(line).len())),
            errors: Vec::new(),
        }
    }
}
impl Fields for Base {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("kind", QueryValue::text(self.kind.as_str())),
            ("title", QueryValue::text(self.title)),
            ("line", QueryValue::count(self.line)),
            ("anchor", QueryValue::from_json(json!(self.anchor))),
            ("source", QueryValue::Object(self.source.fields())),
            ("errors", QueryValue::strings(self.errors)),
        ])
    }
}

/// The scheduling attributes a task reads, and the fields they land in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum When {
    Due,
    Scheduled,
    At,
    Estimate,
}
impl When {
    const ALL: [When; 4] = [Self::Due, Self::Scheduled, Self::At, Self::Estimate];
    fn as_str(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Scheduled => "scheduled",
            Self::At => "at",
            Self::Estimate => "estimate",
        }
    }
}

/// Tasks, events and stops share one shape, so a query can sort them together.
#[derive(Clone, Debug)]
struct Scheduling {
    due: QueryValue,
    scheduled: QueryValue,
    at: QueryValue,
    at_date: QueryValue,
    estimate: QueryValue,
    parent: QueryValue,
    tags: Vec<String>,
    blocked_by: Vec<String>,
    done: bool,
    leaf: bool,
}
impl Default for Scheduling {
    fn default() -> Self {
        Self {
            due: QueryValue::Null,
            scheduled: QueryValue::Null,
            at: QueryValue::Null,
            at_date: QueryValue::Null,
            estimate: QueryValue::Null,
            parent: QueryValue::Null,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            done: false,
            leaf: true,
        }
    }
}
impl Scheduling {
    fn set(&mut self, when: When, value: QueryValue) {
        match when {
            When::Due => self.due = value,
            When::Scheduled => self.scheduled = value,
            When::At => self.at = value,
            When::Estimate => self.estimate = value,
        }
    }
}
impl Fields for Scheduling {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            (When::Due.as_str(), self.due),
            (When::Scheduled.as_str(), self.scheduled),
            (When::At.as_str(), self.at),
            (When::Estimate.as_str(), self.estimate),
            ("at_date", self.at_date),
            ("parent", self.parent),
            ("tags", QueryValue::strings(self.tags)),
            ("blocked_by", QueryValue::strings(self.blocked_by)),
            ("done", QueryValue::boolean(self.done)),
            ("leaf", QueryValue::boolean(self.leaf)),
        ])
    }
}

/// One `@due`/`@scheduled`/`@at` attribute as the task read it.
#[derive(Clone, Debug)]
struct ScheduleEntry {
    key: When,
    value: QueryValue,
    error: QueryValue,
}
impl Fields for ScheduleEntry {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("key", QueryValue::text(self.key.as_str())),
            ("value", self.value),
            ("error", self.error),
        ])
    }
}
#[derive(Clone, Debug)]
struct ChildTask {
    line: usize,
    done: bool,
}
impl Fields for ChildTask {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("line", QueryValue::count(self.line)),
            ("done", QueryValue::boolean(self.done)),
        ])
    }
}
fn list(items: impl IntoIterator<Item = impl Fields>) -> QueryValue {
    QueryValue::Array(
        items
            .into_iter()
            .map(|item| QueryValue::Object(item.fields()))
            .collect(),
    )
}

#[derive(Clone, Debug)]
struct TaskRecord {
    base: Base,
    scheduling: Scheduling,
    checked: bool,
    name: QueryValue,
    attributes: BTreeMap<String, QueryValue>,
    blocked_error: QueryValue,
    schedule: Vec<ScheduleEntry>,
    children: Vec<ChildTask>,
    timer: QueryValue,
}
impl Fields for TaskRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.scheduling.fields());
        fields.extend(entries([
            ("checked", QueryValue::boolean(self.checked)),
            ("name", self.name),
            ("attributes", QueryValue::Object(self.attributes)),
            ("blocked_error", self.blocked_error),
            ("schedule", list(self.schedule)),
            ("children", list(self.children)),
            ("timer", self.timer),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct EventRecord {
    base: Base,
    scheduling: Scheduling,
}
impl Fields for EventRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.scheduling.fields());
        fields
    }
}

#[derive(Clone, Debug)]
struct StopRecord {
    base: Base,
    scheduling: Scheduling,
}
impl Fields for StopRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.scheduling.fields());
        fields
    }
}

#[derive(Clone, Debug)]
struct DayRecord {
    base: Base,
    value: QueryValue,
}
impl Fields for DayRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([("value", self.value)]));
        fields
    }
}

/// Where a running timer was started, when that is another note's definition.
#[derive(Clone, Debug)]
struct TimerOrigin {
    document: String,
    name: String,
}
impl Fields for TimerOrigin {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("document", QueryValue::text(self.document)),
            ("name", QueryValue::text(self.name)),
        ])
    }
}
#[derive(Clone, Debug)]
struct TimerRecord {
    base: Base,
    name: String,
    definition: bool,
    inlay: bool,
    value: QueryValue,
    origin: Option<TimerOrigin>,
}
impl Fields for TimerRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("name", QueryValue::text(self.name)),
            ("definition", QueryValue::boolean(self.definition)),
            ("inlay", QueryValue::boolean(self.inlay)),
            ("value", self.value),
            (
                "origin",
                self.origin
                    .map(|o| QueryValue::Object(o.fields()))
                    .unwrap_or(QueryValue::Null),
            ),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct LinkRecord {
    base: Base,
    url: String,
}
impl Fields for LinkRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([("url", QueryValue::text(self.url))]));
        fields
    }
}

#[derive(Clone, Debug)]
struct SectionRecord {
    base: Base,
    end_line: usize,
    level: usize,
}
impl Fields for SectionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("end_line", QueryValue::count(self.end_line)),
            ("level", QueryValue::count(self.level)),
        ]));
        fields
    }
}

/// Calculations, bracketed references and table cells all project one evaluated
/// expression; only the surrounding fields differ.
#[derive(Clone, Debug)]
struct Expression {
    expression: String,
    value: QueryValue,
    type_name: QueryValue,
    display: QueryValue,
}
impl Expression {
    /// Returns the projection and the errors that belong on the record's base.
    fn new(expression: &str, value: Result<Value, String>) -> (Self, Vec<String>) {
        match value {
            Ok(v) => (
                Self {
                    expression: expression.into(),
                    type_name: QueryValue::text(v.type_name()),
                    display: QueryValue::text(v.display()),
                    value: QueryValue::from_value(v),
                },
                Vec::new(),
            ),
            Err(e) => (
                Self {
                    expression: expression.into(),
                    value: QueryValue::Null,
                    type_name: QueryValue::Null,
                    display: QueryValue::Null,
                },
                vec![e],
            ),
        }
    }
}
impl Fields for Expression {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            (EXPRESSION, QueryValue::text(self.expression)),
            (LazyField::Value.as_str(), self.value),
            (LazyField::Type.as_str(), self.type_name),
            (LazyField::Display.as_str(), self.display),
        ])
    }
}

#[derive(Clone, Debug)]
struct CalculationRecord {
    base: Base,
    expression: Expression,
    bracketed: bool,
}
impl Fields for CalculationRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.expression.fields());
        fields.extend(entries([(
            "bracketed",
            QueryValue::boolean(self.bracketed),
        )]));
        fields
    }
}

#[derive(Clone, Debug)]
struct ReferenceRecord {
    base: Base,
    expression: Expression,
    name: String,
    property: QueryValue,
}
impl Fields for ReferenceRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.expression.fields());
        fields.extend(entries([
            (NAME, QueryValue::text(self.name)),
            (PROPERTY, self.property),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct CellRecord {
    base: Base,
    expression: Expression,
    computed: bool,
    table: String,
    row: usize,
    column: QueryValue,
}
impl Fields for CellRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.expression.fields());
        fields.extend(entries([
            ("computed", QueryValue::boolean(self.computed)),
            ("table", QueryValue::text(self.table)),
            ("row", QueryValue::count(self.row)),
            ("column", self.column),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct NoteRecord {
    base: Base,
    text: String,
}
impl Fields for NoteRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([("text", QueryValue::text(self.text))]));
        fields
    }
}

/// Values, plans and tables are one definition each. Their value, type, display
/// and (for a plan) solution stay null until the record is evaluated.
#[derive(Clone, Debug)]
struct DefinitionRecord {
    base: Base,
    name: String,
    expression: String,
    computed: bool,
    solution: bool,
}
impl Fields for DefinitionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            (NAME, QueryValue::text(self.name)),
            (EXPRESSION, QueryValue::text(self.expression)),
            (LazyField::Value.as_str(), QueryValue::Null),
            (LazyField::Type.as_str(), QueryValue::Null),
            (LazyField::Display.as_str(), QueryValue::Null),
            ("computed", QueryValue::boolean(self.computed)),
        ]));
        if self.solution {
            fields.insert(LazyField::Solution.as_str().into(), QueryValue::Null);
        }
        fields
    }
}

#[derive(Clone, Debug)]
struct RowRecord {
    base: Base,
    table: String,
    cells: BTreeMap<String, QueryValue>,
}
impl Fields for RowRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("table", QueryValue::text(self.table)),
            ("cells", QueryValue::Object(self.cells)),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct DecisionRecord {
    base: Base,
    value: QueryValue,
    plan: String,
}
impl Fields for DecisionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            (LazyField::Value.as_str(), self.value),
            ("plan", QueryValue::text(self.plan)),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct ResourceRecord {
    base: Base,
    target: String,
    metadata: QueryValue,
}
impl Fields for ResourceRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("target", QueryValue::text(self.target)),
            ("metadata", self.metadata),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct DiagnosticRecord {
    base: Base,
    message: String,
    severity: String,
    code: QueryValue,
}
impl Fields for DiagnosticRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("message", QueryValue::text(self.message)),
            ("severity", QueryValue::text(self.severity)),
            ("code", self.code),
        ]));
        fields
    }
}

/// How a link or resource wants to be shown, asked for only when read.
#[derive(Clone, Debug)]
struct Presentation {
    label: String,
    hover: String,
    known: bool,
}
impl Fields for Presentation {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("label", QueryValue::text(self.label)),
            ("hover", QueryValue::text(self.hover)),
            ("known", QueryValue::boolean(self.known)),
        ])
    }
}

/// Field names the lazy accessors read back off a record they did not build.
const NAME: &str = "name";
const EXPRESSION: &str = "expression";
const PROPERTY: &str = "property";

/// Fields a record only produces when they are read: evaluating a definition, or
/// asking the workspace for a link's presentation or a name's hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    const ALL: [LazyField; 7] = [
        Self::Presentation,
        Self::Hover,
        Self::Value,
        Self::Type,
        Self::Solution,
        Self::Errors,
        Self::Display,
    ];
    fn as_str(self) -> &'static str {
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
impl std::str::FromStr for LazyField {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, ()> {
        Self::ALL
            .into_iter()
            .find(|field| field.as_str() == value)
            .ok_or(())
    }
}

/// Definitions are evaluated only when their value, type, solution or errors are read.
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub fields: BTreeMap<String, QueryValue>,
    pub path: PathBuf,
    deferred: Option<Symbol>,
    resource: Option<crate::resources::Resource>,
}
impl Record {
    pub fn projected(path: PathBuf, fields: BTreeMap<String, QueryValue>) -> Self {
        Self {
            path,
            fields,
            deferred: None,
            resource: None,
        }
    }
    fn typed(path: &Path, record: impl Fields) -> Self {
        Self::projected(path.into(), record.fields())
    }
    /// Whether a name binds on this record, counting the lazily produced hover.
    pub fn has(&self, name: &str) -> bool {
        self.fields.contains_key(name)
            || matches!(name.parse(), Ok(LazyField::Hover)) && self.fields.contains_key(NAME)
    }
    pub fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<QueryValue, String> {
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
            Err(()) => {}
        }
        self.fields
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Unknown field '{key}'"))
    }
    fn presentation(&mut self, engine: &mut Engine<'_>) -> QueryValue {
        self.evaluate(engine);
        let Some(resource) = &self.resource else {
            return QueryValue::Null;
        };
        let view = resource.presentation(
            &self.path,
            &engine.workspace.cache,
            engine.now.to_utc(),
            engine.link_features(),
        );
        engine.time_dependent |= view.time_dependent;
        QueryValue::Object(
            Presentation {
                label: view.label,
                hover: view.hover,
                known: view.known_link,
            }
            .fields(),
        )
    }
    /// A named record hovers as its symbol does; a property reference as itself.
    fn hover(&self, engine: &mut Engine<'_>) -> Option<QueryValue> {
        let Some(QueryValue::Scalar(Value::Text(name))) = self.fields.get(NAME) else {
            return None;
        };
        let expression = self
            .fields
            .get(EXPRESSION)
            .cloned()
            .unwrap_or(QueryValue::Null);
        if self
            .fields
            .get(PROPERTY)
            .is_some_and(|v| *v != QueryValue::Null)
        {
            return Some(expression);
        }
        Some(
            engine
                .workspace
                .resolve(&self.path, name)
                .map(|symbol| QueryValue::text(engine.request().symbol_hover(&symbol)))
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
                self.fields.insert(
                    LazyField::Type.as_str().into(),
                    QueryValue::text(value.type_name()),
                );
                self.fields.insert(
                    LazyField::Display.as_str().into(),
                    QueryValue::text(value.display()),
                );
                let value = QueryValue::from_value(match value {
                    Value::Plan(p) => p.record(engine.workspace),
                    other => other,
                });
                if self.fields.contains_key(LazyField::Solution.as_str()) {
                    self.fields
                        .insert(LazyField::Solution.as_str().into(), value.clone());
                }
                self.fields.insert(LazyField::Value.as_str().into(), value);
            }
            Err(e) => {
                self.fields
                    .insert(LazyField::Errors.as_str().into(), QueryValue::strings([e]));
            }
        }
    }
    pub fn materialize(mut self, engine: &mut Engine<'_>) -> QueryValue {
        self.evaluate(engine);
        QueryValue::Object(self.fields)
    }
}
pub(crate) fn source(ws: &Workspace, path: &Path, span: Span) -> QueryValue {
    QueryValue::Object(SourceRef::new(ws, path, span).fields())
}
fn date_field(value: Option<NaiveDate>) -> QueryValue {
    value
        .map(|d| QueryValue::Scalar(Value::Date(d)))
        .unwrap_or(QueryValue::Null)
}
/// The `kind` field every catalog record carries. Queries and .wtf modules
/// match on these names, so they are part of the workspace's data contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecordKind {
    Task,
    Event,
    Stop,
    Day,
    Timer,
    Link,
    Section,
    Calculation,
    Reference,
    Cell,
    Note,
    Value,
    Plan,
    Table,
    Row,
    Decision,
    Resource,
    Diagnostic,
}
impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Event => "event",
            Self::Stop => "stop",
            Self::Day => "day",
            Self::Timer => "timer",
            Self::Link => "link",
            Self::Section => "section",
            Self::Calculation => "calculation",
            Self::Reference => "reference",
            Self::Cell => "cell",
            Self::Note => "note",
            Self::Value => "value",
            Self::Plan => "plan",
            Self::Table => "table",
            Self::Row => "row",
            Self::Decision => "decision",
            Self::Resource => "resource",
            Self::Diagnostic => "diagnostic",
        }
    }
}
impl std::fmt::Display for RecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A named set of workspace records. Queries bind these names, and feature
/// modules declare the ones they read in `module.inputs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Collection {
    Ast,
    Days,
    Timers,
    Links,
    Tasks,
    Events,
    Stops,
    Entries,
    Values,
    Plans,
    Decisions,
    Tables,
    Rows,
    Resources,
    Diagnostics,
    Notes,
    Sections,
    Calculations,
    References,
    Cells,
}
impl Collection {
    /// Declaration order is the order the query API lists them in its errors.
    pub const ALL: &'static [Collection] = &[
        Collection::Ast,
        Collection::Days,
        Collection::Timers,
        Collection::Links,
        Collection::Tasks,
        Collection::Events,
        Collection::Stops,
        Collection::Entries,
        Collection::Values,
        Collection::Plans,
        Collection::Decisions,
        Collection::Tables,
        Collection::Rows,
        Collection::Resources,
        Collection::Diagnostics,
        Collection::Notes,
        Collection::Sections,
        Collection::Calculations,
        Collection::References,
        Collection::Cells,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ast => "ast",
            Self::Days => "days",
            Self::Timers => "timers",
            Self::Links => "links",
            Self::Tasks => "tasks",
            Self::Events => "events",
            Self::Stops => "stops",
            Self::Entries => "entries",
            Self::Values => "values",
            Self::Plans => "plans",
            Self::Decisions => "decisions",
            Self::Tables => "tables",
            Self::Rows => "rows",
            Self::Resources => "resources",
            Self::Diagnostics => "diagnostics",
            Self::Notes => "notes",
            Self::Sections => "sections",
            Self::Calculations => "calculations",
            Self::References => "references",
            Self::Cells => "cells",
        }
    }
    /// Every collection name, in declaration order, for error messages.
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
impl std::str::FromStr for Collection {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| format!("Unknown input collection: {value}"))
    }
}
impl std::fmt::Display for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The query API and feature modules read the same semantic records. Each
/// collection is one record builder below; this match is their index.
pub(crate) fn collect_document(
    ws: &Workspace,
    collection: Collection,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
    module_diagnostics: bool,
) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    match collection {
        // Two collections span documents instead of visiting them in turn.
        Collection::Ast => {
            for path in ws
                .documents
                .keys()
                .filter(|p| only.is_none_or(|only| only == p.as_path()))
            {
                records.extend(crate::features::inspection::ast(ws, path));
            }
            return Ok(records);
        }
        Collection::Decisions => {
            decisions(ws, engine, only, &mut records);
            return Ok(records);
        }
        _ => {}
    }
    for (path, doc) in &ws.documents {
        if only.is_some_and(|wanted| wanted != path) {
            continue;
        }
        match collection {
            Collection::Ast | Collection::Decisions => unreachable!("handled above"),
            Collection::Days => days(ws, path, doc, engine, &mut records)?,
            Collection::Timers => timers(ws, path, doc, engine, &mut records),
            Collection::Links => links(ws, path, doc, &mut records),
            Collection::Sections => sections(ws, path, doc, &mut records),
            Collection::Calculations => calculations(ws, path, doc, engine, &mut records),
            Collection::References => references(ws, path, doc, engine, &mut records),
            Collection::Cells => cells(ws, path, doc, engine, &mut records),
            Collection::Notes => notes(ws, path, doc, &mut records),
            Collection::Tasks => tasks(ws, path, doc, ctx, engine, false, &mut records),
            Collection::Events => events(ws, path, doc, ctx, engine, &mut records),
            Collection::Stops => stops(ws, path, doc, ctx, &mut records),
            Collection::Entries => {
                tasks(ws, path, doc, ctx, engine, true, &mut records);
                events(ws, path, doc, ctx, engine, &mut records);
                stops(ws, path, doc, ctx, &mut records);
            }
            Collection::Values | Collection::Plans | Collection::Tables | Collection::Rows => {
                definitions(ws, path, doc, collection, engine, &mut records)?
            }
            Collection::Resources => resources(ws, path, doc, &mut records),
            Collection::Diagnostics => {
                diagnostics(ws, path, engine, module_diagnostics, &mut records)
            }
        }
    }
    Ok(records)
}

fn decisions(
    ws: &Workspace,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
    records: &mut Vec<Record>,
) {
    for (plan_path, doc) in &ws.documents {
        for plan in &doc.plans {
            let symbol = Symbol {
                path: plan_path.clone(),
                kind: SymbolKind::Definition(plan.definition),
            };
            let Ok(Value::Plan(value)) = engine.symbol(&symbol) else {
                continue;
            };
            for (row, value) in &value.rows {
                if only.is_some_and(|path| path != row.table.path) {
                    continue;
                }
                let Some(cell) = crate::tables::table(ws, &row.table)
                    .and_then(|t| t.rows.get(row.row))
                    .and_then(|r| r.get(row.column))
                else {
                    continue;
                };
                let name = &doc.definitions[plan.definition].named.name;
                let mut base = Base::new(
                    ws,
                    &row.table.path,
                    cell.span.line,
                    RecordKind::Decision,
                    name,
                );
                base.anchor = cell.span.range(&ws.documents[&row.table.path].text).end;
                records.push(Record::typed(
                    &row.table.path,
                    DecisionRecord {
                        base,
                        value: QueryValue::from_value(value.clone()),
                        plan: name.clone(),
                    },
                ));
            }
        }
    }
}

fn days(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) -> Result<(), String> {
    let dates = crate::itinerary::try_dates(&ws.modules, &doc.days, engine.today)?;
    for (day, date) in doc.days.iter().zip(dates) {
        let mut value = crate::itinerary::day_record(day, doc);
        if let Some(date) = date
            && let Some((places, _)) = &day.places
            && let Some(place) = crate::lookups::day_place(places)
            && let Some(lookup) =
                crate::lookups::LookupKey::forecast(&place, date).lookup(&ws.lookups)
            && let Value::Record(fields) = &mut value
        {
            fields.insert(
                "forecast".into(),
                crate::modules::record([
                    ("place".into(), Value::Text(place)),
                    (
                        "display".into(),
                        Value::Text(
                            crate::lookups::forecast_from(&lookup.value, false)
                                .map(|f| f.display())
                                .unwrap_or_else(|e| e),
                        ),
                    ),
                    ("source".into(), Value::Text(lookup.source.clone())),
                    (
                        "fetched_at".into(),
                        Value::DateTime(lookup.fetched_at.fixed_offset()),
                    ),
                ]),
            );
        }
        records.push(Record::typed(
            path,
            DayRecord {
                base: Base::new(ws, path, day.line, RecordKind::Day, doc.line(day.line)),
                value: QueryValue::from_value(value),
            },
        ));
    }
    Ok(())
}

fn timers(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    let occurrences = doc
        .definitions
        .iter()
        .map(|d| {
            (
                d.named.name.as_str(),
                d.named.span.line,
                d.end.range(&doc.text).start,
                true,
                d.expression,
            )
        })
        .chain(doc.references.iter().map(|r| {
            let end = if r.bracket {
                r.end() + doc.line(r.span.line)[r.end()..].find(']').unwrap_or(0) + 1
            } else {
                r.end()
            };
            (
                &*r.name,
                r.span.line,
                Span::new(r.span.line, end, end).range(&doc.text).end,
                false,
                r.bracket && r.property.is_none(),
            )
        }));
    for (name, line, anchor, definition, inlay) in occurrences {
        let Ok(Value::Timer(timer)) = engine.named(path, name) else {
            continue;
        };
        let mut base = Base::new(ws, path, line, RecordKind::Timer, name);
        base.anchor = anchor;
        records.push(Record::typed(
            path,
            TimerRecord {
                base,
                name: name.into(),
                definition,
                inlay,
                value: QueryValue::from_value(timer.record()),
                origin: timer.origin.as_ref().map(|origin| TimerOrigin {
                    document: crate::paths::file_url(&origin.path).unwrap().into(),
                    name: ws.named(origin).name.clone(),
                }),
            },
        ));
    }
}

fn links(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for link in &doc.links {
        let mut base = Base::new(ws, path, link.span.line, RecordKind::Link, &link.target);
        base.source = SourceRef::new(ws, path, link.span);
        base.anchor = link.span.range(&doc.text).end;
        let mut r = Record::typed(
            path,
            LinkRecord {
                base,
                url: link.target.clone(),
            },
        );
        r.resource = Some(crate::resources::Resource {
            target: link.target.clone(),
            origin: Some(path.into()),
        });
        records.push(r);
    }
}

fn sections(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for section in &doc.sections {
        records.push(Record::typed(
            path,
            SectionRecord {
                base: Base::new(ws, path, section.line, RecordKind::Section, &section.title),
                end_line: section.end_line,
                level: section.level,
            },
        ));
    }
}

fn calculations(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for calculation in &doc.calculations {
        let (mut base, expression, resource) = expression_base(
            ws,
            path,
            RecordKind::Calculation,
            &calculation.source,
            calculation.span,
            engine.eval_at(path, &calculation.source, calculation.span),
        );
        let end = calculation.span.end + usize::from(calculation.bracketed);
        base.anchor = Span::new(calculation.span.line, end, end)
            .range(&doc.text)
            .end;
        let mut r = Record::typed(
            path,
            CalculationRecord {
                base,
                expression,
                bracketed: calculation.bracketed,
            },
        );
        r.resource = resource;
        records.push(r);
    }
}

fn references(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for reference in doc.references.iter().filter(|r| r.bracket) {
        let source = reference.expression();
        let end = reference.end()
            + doc.line(reference.span.line)[reference.end()..]
                .find(']')
                .unwrap_or(0)
            + 1;
        let (mut base, expression, resource) = expression_base(
            ws,
            path,
            RecordKind::Reference,
            &source,
            reference.span,
            engine.eval(path, &source),
        );
        base.anchor = Span::new(reference.span.line, end, end)
            .range(&doc.text)
            .end;
        let mut r = Record::typed(
            path,
            ReferenceRecord {
                base,
                expression,
                name: reference.name.clone(),
                property: reference
                    .property
                    .as_ref()
                    .map(QueryValue::text)
                    .unwrap_or(QueryValue::Null),
            },
        );
        r.resource = resource;
        records.push(r);
    }
}

fn cells(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for table in &doc.tables {
        for (row, cells) in table.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let value = match &cell.expression {
                    Some((source, span)) => engine.eval_at(path, source, *span),
                    None => cell.value.clone(),
                };
                let source = cell
                    .expression
                    .as_ref()
                    .map(|(s, _)| s.as_str())
                    .unwrap_or(&cell.source);
                let (mut base, expression, resource) =
                    expression_base(ws, path, RecordKind::Cell, source, cell.span, value);
                base.anchor = cell.span.range(&doc.text).end;
                let mut r = Record::typed(
                    path,
                    CellRecord {
                        base,
                        expression,
                        computed: cell.calculated(),
                        table: doc.definitions[table.definition].named.name.clone(),
                        row,
                        column: table
                            .columns
                            .get(column)
                            .map(|c| QueryValue::text(&c.name))
                            .unwrap_or(QueryValue::Null),
                    },
                );
                r.resource = resource;
                records.push(r);
            }
        }
    }
}

fn notes(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    records.push(Record::typed(
        path,
        NoteRecord {
            base: Base::new(
                ws,
                path,
                0,
                RecordKind::Note,
                path.file_name().unwrap_or_default().to_string_lossy(),
            ),
            text: doc.text.clone(),
        },
    ));
}

fn tasks(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    leaves_only: bool,
    records: &mut Vec<Record>,
) {
    let parents: std::collections::BTreeSet<_> =
        doc.tasks.iter().filter_map(|t| t.parent).collect();
    for (i, task) in doc.tasks.iter().enumerate() {
        let leaf = !parents.contains(&i);
        if leaves_only && !leaf {
            continue;
        }
        let mut record = TaskRecord {
            base: Base::new(ws, path, task.line, RecordKind::Task, &task.title),
            scheduling: Scheduling {
                leaf,
                done: engine.task_done(path, i),
                parent: task
                    .parent
                    .map(|i| source(ws, path, doc.tasks[i].checkbox))
                    .unwrap_or(QueryValue::Null),
                tags: task.tags.clone(),
                ..Scheduling::default()
            },
            checked: task.checked,
            name: task
                .named
                .as_ref()
                .map(|n| QueryValue::text(&n.name))
                .unwrap_or(QueryValue::Null),
            attributes: task
                .attributes
                .iter()
                .map(|(k, a)| (k.clone(), QueryValue::text(&a.value)))
                .collect(),
            blocked_error: QueryValue::Null,
            schedule: Vec::new(),
            children: doc
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, child)| child.parent == Some(i))
                .map(|(index, child)| ChildTask {
                    line: child.line,
                    done: engine.task_done(path, index),
                })
                .collect(),
            timer: QueryValue::Null,
        };
        let mut errors = Vec::new();
        for when in When::ALL {
            let Some(attribute) = task.attributes.get(when.as_str()) else {
                continue;
            };
            let value = if when == When::Estimate {
                engine.eval(path, &attribute.value)
            } else {
                engine.when(path, &attribute.value)
            };
            if when != When::Estimate {
                record.schedule.push(ScheduleEntry {
                    key: when,
                    value: value
                        .as_ref()
                        .ok()
                        .and_then(|v| ctx.date(v))
                        .map(|v| QueryValue::Scalar(Value::Date(v)))
                        .unwrap_or(QueryValue::Null),
                    error: value
                        .as_ref()
                        .err()
                        .map(QueryValue::text)
                        .unwrap_or(QueryValue::Null),
                });
            }
            match value {
                Ok(Value::Duration(s)) if when == When::Estimate && s >= 0 => {
                    record
                        .scheduling
                        .set(when, QueryValue::Scalar(Value::Duration(s)));
                }
                Ok(v) if when != When::Estimate && ctx.date(&v).is_some() => {
                    let date = ctx.date(&v);
                    let at = when == When::At;
                    record.scheduling.set(
                        when,
                        if at {
                            QueryValue::from_value(v)
                        } else {
                            date_field(date)
                        },
                    );
                    if at {
                        record.scheduling.at_date = date_field(date);
                    }
                }
                Ok(_) => errors.push(format!(
                    "@{}: expected {}",
                    when.as_str(),
                    if when == When::Estimate {
                        "a nonnegative duration"
                    } else {
                        "a date or timestamp"
                    }
                )),
                Err(e) => errors.push(format!("@{}: {e}", when.as_str())),
            }
        }
        // A repeating task with no explicit due date is due today.
        if record.scheduling.due == QueryValue::Null && task.attributes.contains_key("every") {
            record.scheduling.due = date_field(Some(ctx.today()));
        }
        match engine.blocked(path, i) {
            Ok(v) => record.scheduling.blocked_by = v,
            Err(e) => {
                record.blocked_error = QueryValue::text(&e);
                errors.push(e);
            }
        }
        record.timer = match task
            .attributes
            .get("timer")
            .and_then(|a| engine.eval(path, &a.value).ok())
        {
            Some(Value::Timer(timer)) => QueryValue::from_value(timer.record()),
            _ => QueryValue::Null,
        };
        record.base.errors = errors;
        records.push(Record::typed(path, record));
    }
}

fn events(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for event in &doc.events {
        let mut base = Base::new(ws, path, event.line, RecordKind::Event, &event.title);
        let mut scheduling = Scheduling::default();
        match engine.when(path, &event.attributes[When::At.as_str()].value) {
            Ok(v) if ctx.date(&v).is_some() => {
                scheduling.at_date = date_field(ctx.date(&v));
                scheduling.at = QueryValue::from_value(v);
            }
            other => {
                base.errors = vec![
                    other
                        .err()
                        .unwrap_or_else(|| "@at requires a date or timestamp".into()),
                ];
            }
        }
        records.push(Record::typed(path, EventRecord { base, scheduling }));
    }
}

fn stops(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    ctx: QueryContext,
    records: &mut Vec<Record>,
) {
    let dates = crate::itinerary::dates(&ws.modules, &doc.days, ctx.today());
    for (day, date) in doc.days.iter().zip(dates) {
        for stop in &day.stops {
            let at = date.and_then(|d| {
                ctx.now
                    .offset()
                    .from_local_datetime(&d.and_time(stop.time))
                    .single()
            });
            records.push(Record::typed(
                path,
                StopRecord {
                    base: Base::new(
                        ws,
                        path,
                        stop.line,
                        RecordKind::Stop,
                        crate::itinerary::label(&ws.modules, stop),
                    ),
                    scheduling: Scheduling {
                        at_date: date_field(date),
                        at: at
                            .map(|d| QueryValue::Scalar(Value::DateTime(d)))
                            .unwrap_or_else(|| {
                                QueryValue::text(stop.time.format("%H:%M").to_string())
                            }),
                        ..Scheduling::default()
                    },
                },
            ));
        }
    }
}

/// Values, plans, tables and table rows all project one definition.
fn definitions(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    collection: Collection,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) -> Result<(), String> {
    for (i, def) in doc.definitions.iter().enumerate() {
        let plan = doc.plans.iter().any(|p| p.definition == i);
        let table = doc.tables.iter().find(|t| t.definition == i);
        if (collection == Collection::Plans && !plan)
            || (matches!(collection, Collection::Tables | Collection::Rows) && table.is_none())
        {
            continue;
        }
        let symbol = Symbol {
            path: path.to_path_buf(),
            kind: SymbolKind::Definition(i),
        };
        if collection == Collection::Rows {
            match engine.symbol(&symbol) {
                Ok(Value::Table(t)) => {
                    for (row, values) in t.rows.iter().enumerate() {
                        records.push(Record::typed(
                            path,
                            RowRecord {
                                base: Base::new(
                                    ws,
                                    path,
                                    table.unwrap().rows[row][0].span.line,
                                    RecordKind::Row,
                                    &def.named.name,
                                ),
                                table: def.named.name.clone(),
                                cells: t
                                    .columns
                                    .iter()
                                    .cloned()
                                    .zip(values.iter().cloned().map(QueryValue::from_value))
                                    .collect(),
                            },
                        ));
                    }
                }
                Err(e) => {
                    return Err(format!(
                        "{}:{}: {e}",
                        path.display(),
                        def.named.span.line + 1
                    ));
                }
                _ => return Err("Expected a table".into()),
            }
            continue;
        }
        let mut base = Base::new(
            ws,
            path,
            def.named.span.line,
            if plan {
                RecordKind::Plan
            } else if table.is_some() {
                RecordKind::Table
            } else {
                RecordKind::Value
            },
            &def.named.name,
        );
        base.anchor = def.end.range(&doc.text).end;
        let mut r = Record::typed(
            path,
            DefinitionRecord {
                base,
                name: def.named.name.clone(),
                expression: def.source.clone(),
                computed: def.expression,
                solution: plan,
            },
        );
        r.deferred = Some(symbol);
        records.push(r);
    }
    Ok(())
}

fn resources(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    let targets = doc.links.iter().map(|l| (l.span, l.target.as_str())).chain(
        doc.definitions
            .iter()
            .filter(|d| !d.expression && crate::resources::Resource::parse(&d.source).is_some())
            .map(|d| (d.value_span, d.source.as_str())),
    );
    for (span, target) in targets {
        let mut base = Base::new(ws, path, span.line, RecordKind::Resource, target);
        base.source = SourceRef::new(ws, path, span);
        records.push(Record::typed(
            path,
            ResourceRecord {
                base,
                target: target.into(),
                metadata: ws
                    .cache
                    .get(target)
                    .map(|m| QueryValue::from_json(json!(m)))
                    .unwrap_or(QueryValue::Null),
            },
        ));
    }
}

fn diagnostics(
    ws: &Workspace,
    path: &Path,
    engine: &mut Engine<'_>,
    module_diagnostics: bool,
    records: &mut Vec<Record>,
) {
    let request = engine.request();
    let diagnostics = if module_diagnostics {
        crate::diagnostics::collect(&request, path, false)
    } else {
        crate::diagnostics::collect_native(&request, path, false)
    };
    for d in diagnostics {
        let mut base = Base::new(
            ws,
            path,
            d.range.start.line as usize,
            RecordKind::Diagnostic,
            &d.message,
        );
        base.source.range = d.range;
        records.push(Record::typed(
            path,
            DiagnosticRecord {
                base,
                message: d.message,
                severity: crate::diagnostics::severity_name(d.severity).into(),
                code: QueryValue::from_json(json!(d.code)),
            },
        ));
    }
}

/// A calculation, bracketed reference or table cell: one expression, evaluated.
fn expression_base(
    ws: &Workspace,
    path: &Path,
    kind: RecordKind,
    source: &str,
    span: Span,
    value: Result<Value, String>,
) -> (Base, Expression, Option<crate::resources::Resource>) {
    let mut base = Base::new(ws, path, span.line, kind, source);
    base.source = SourceRef::new(ws, path, span);
    let resource = match &value {
        Ok(Value::Resource(resource)) => Some(resource.clone()),
        _ => None,
    };
    let (expression, errors) = Expression::new(source, value);
    base.errors = errors;
    (base, expression, resource)
}
