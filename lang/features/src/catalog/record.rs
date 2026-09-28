//! The lazy `Record` machinery and the parts every catalog record shares.
use super::{QueryValue, RecordKind};
use crate::session_impl::Session;
use chrono::NaiveDate;
use common::Span;
use eval::engine::{Engine, Value};
use eval::resources::ResourcePresenting;
use eval::{Symbol, Workspace};
use lsp_types::{Position, Range};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Every catalog record is a struct that knows its own field names: these
/// `fields` implementations are the only place those names are written down.
pub(super) trait Fields {
    fn fields(self) -> BTreeMap<String, QueryValue>;
}
pub(super) fn entries<const N: usize>(
    items: [(&str, QueryValue); N],
) -> BTreeMap<String, QueryValue> {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
/// A projection built from the record family's own `FIELDS` list, so those
/// names are written once: here for the query, and in `Collection::fields` for
/// the reference. The two arities have to match, which the compiler checks.
pub(super) fn projected<const N: usize>(
    names: [&str; N],
    values: [QueryValue; N],
) -> BTreeMap<String, QueryValue> {
    names
        .into_iter()
        .zip(values)
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// Where a record was found. Every record carries one under `source`.
#[derive(Clone, Debug)]
pub(super) struct SourceRef {
    pub(super) path: PathBuf,
    pub(super) uri: String,
    pub(super) line: usize,
    pub(super) range: Range,
}
impl SourceRef {
    pub(super) const FIELDS: [&'static str; 4] = ["path", "uri", "line", "range"];
    pub(super) fn new(ws: &Workspace, path: &Path, span: Span) -> Self {
        let mut uri = common::file_url(path)
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
        projected(
            Self::FIELDS,
            [
                QueryValue::text(self.path.to_string_lossy()),
                QueryValue::text(self.uri),
                QueryValue::count(self.line),
                QueryValue::from_json(json!(self.range)),
            ],
        )
    }
}

/// What every record carries, whatever its kind.
#[derive(Clone, Debug)]
pub(super) struct Base {
    pub(super) kind: RecordKind,
    pub(super) title: String,
    pub(super) line: usize,
    pub(super) anchor: Position,
    pub(super) source: SourceRef,
    pub(super) errors: Vec<String>,
}
impl Base {
    pub(super) const FIELDS: [&'static str; 6] =
        ["kind", "title", "line", "anchor", "source", "errors"];
    pub(super) fn new(
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
        projected(
            Self::FIELDS,
            [
                QueryValue::text(self.kind.as_str()),
                QueryValue::text(self.title),
                QueryValue::count(self.line),
                QueryValue::from_json(json!(self.anchor)),
                QueryValue::Object(self.source.fields()),
                QueryValue::strings(self.errors),
            ],
        )
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

/// Tasks, events and stops share one shape, so a query can sort them together.
#[derive(Clone, Debug)]
pub(super) struct Scheduling {
    pub(super) due: QueryValue,
    pub(super) scheduled: QueryValue,
    pub(super) at: QueryValue,
    pub(super) at_date: QueryValue,
    pub(super) estimate: QueryValue,
    pub(super) parent: QueryValue,
    pub(super) tags: Vec<String>,
    pub(super) blocked_by: Vec<String>,
    pub(super) done: bool,
    pub(super) leaf: bool,
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
    /// The timeline fields tasks, events and stops share.
    pub(super) const FIELDS: [&'static str; 10] = [
        When::Due.as_str(),
        When::Scheduled.as_str(),
        When::At.as_str(),
        When::Estimate.as_str(),
        "at_date",
        "parent",
        "tags",
        "blocked_by",
        "done",
        "leaf",
    ];
    pub(super) fn set(&mut self, when: When, value: QueryValue) {
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
        projected(
            Self::FIELDS,
            [
                self.due,
                self.scheduled,
                self.at,
                self.estimate,
                self.at_date,
                self.parent,
                QueryValue::strings(self.tags),
                QueryValue::strings(self.blocked_by),
                QueryValue::boolean(self.done),
                QueryValue::boolean(self.leaf),
            ],
        )
    }
}

/// One `@due`/`@scheduled`/`@at` attribute as the task read it.
#[derive(Clone, Debug)]
pub(super) struct ScheduleEntry {
    pub(super) key: When,
    pub(super) value: QueryValue,
    pub(super) error: QueryValue,
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
pub(super) struct ChildTask {
    pub(super) line: usize,
    pub(super) done: bool,
}
impl Fields for ChildTask {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("line", QueryValue::count(self.line)),
            ("done", QueryValue::boolean(self.done)),
        ])
    }
}
pub(super) fn list(items: impl IntoIterator<Item = impl Fields>) -> QueryValue {
    QueryValue::Array(
        items
            .into_iter()
            .map(|item| QueryValue::Object(item.fields()))
            .collect(),
    )
}

/// Where a running timer was started, when that is another note's definition.
#[derive(Clone, Debug)]
pub(super) struct TimerOrigin {
    pub(super) document: String,
    pub(super) name: String,
}
impl Fields for TimerOrigin {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        entries([
            ("document", QueryValue::text(self.document)),
            ("name", QueryValue::text(self.name)),
        ])
    }
}

/// Calculations, bracketed references and table cells all project one evaluated
/// expression; only the surrounding fields differ.
#[derive(Clone, Debug)]
pub(super) struct Expression {
    expression: String,
    value: QueryValue,
    type_name: QueryValue,
    display: QueryValue,
}
impl Expression {
    /// The projection a calculation, reference or cell shares.
    pub(super) const FIELDS: [&'static str; 4] = [
        EXPRESSION,
        LazyField::Value.as_str(),
        LazyField::Type.as_str(),
        LazyField::Display.as_str(),
    ];
    /// Returns the projection and the errors that belong on the record's base.
    pub(super) fn new(expression: &str, value: Result<Value, String>) -> (Self, Vec<String>) {
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
        projected(
            Self::FIELDS,
            [
                QueryValue::text(self.expression),
                self.value,
                self.type_name,
                self.display,
            ],
        )
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
    pub fields: BTreeMap<String, QueryValue>,
    pub path: PathBuf,
    pub(super) deferred: Option<Symbol>,
    pub(super) resource: Option<eval::resources::Resource>,
}
impl Record {
    pub(crate) fn projected(path: PathBuf, fields: BTreeMap<String, QueryValue>) -> Self {
        Self {
            path,
            fields,
            deferred: None,
            resource: None,
        }
    }
    pub(super) fn typed(path: &Path, record: impl Fields) -> Self {
        Self::projected(path.into(), record.fields())
    }
    /// Whether a name binds on this record, counting the lazily produced hover.
    pub(crate) fn has(&self, name: &str) -> bool {
        self.fields.contains_key(name)
            || matches!(name.parse(), Ok(LazyField::Hover)) && self.fields.contains_key(NAME)
    }
    pub(crate) fn field(
        &mut self,
        key: &str,
        engine: &mut Engine<'_>,
    ) -> Result<QueryValue, String> {
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
                self.fields.insert(
                    LazyField::Errors.as_str().into(),
                    QueryValue::strings([e.to_string()]),
                );
            }
        }
    }
    pub(crate) fn materialize(mut self, engine: &mut Engine<'_>) -> QueryValue {
        self.evaluate(engine);
        QueryValue::Object(self.fields)
    }
}
pub(crate) fn source(ws: &Workspace, path: &Path, span: Span) -> QueryValue {
    QueryValue::Object(SourceRef::new(ws, path, span).fields())
}
pub(super) fn date_field(value: Option<NaiveDate>) -> QueryValue {
    value
        .map(|d| QueryValue::Scalar(Value::Date(d)))
        .unwrap_or(QueryValue::Null)
}
