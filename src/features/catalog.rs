//! Typed, host-independent workspace records. Reading a catalog never performs I/O.
use crate::{
    document::{Document, Span},
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{NaiveDate, TimeZone};
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
    pub fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<QueryValue, String> {
        if key == "presentation" {
            self.evaluate(engine);
            let Some(resource) = &self.resource else {
                return Ok(QueryValue::Null);
            };
            let view = resource.presentation(
                &self.path,
                &engine.workspace.cache,
                engine.now.to_utc(),
                engine.link_features(),
            );
            engine.time_dependent |= view.time_dependent;
            return Ok(QueryValue::object([
                ("label", QueryValue::text(view.label)),
                ("hover", QueryValue::text(view.hover)),
                ("known", QueryValue::boolean(view.known_link)),
            ]));
        }
        if key == "hover"
            && let Some(QueryValue::Scalar(Value::Text(name))) = self.fields.get("name")
        {
            let expression = self
                .fields
                .get("expression")
                .cloned()
                .unwrap_or(QueryValue::Null);
            if self
                .fields
                .get("property")
                .is_some_and(|v| *v != QueryValue::Null)
            {
                return Ok(expression);
            }
            return Ok(engine
                .workspace
                .resolve(&self.path, name)
                .map(|symbol| QueryValue::text(engine.request().symbol_hover(&symbol)))
                .unwrap_or(expression));
        }
        if matches!(key, "value" | "type" | "solution" | "errors" | "display") {
            self.evaluate(engine);
        }
        self.fields
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Unknown field '{key}'"))
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
                    .insert("type".into(), QueryValue::text(value.type_name()));
                self.fields
                    .insert("display".into(), QueryValue::text(value.display()));
                let value = QueryValue::from_value(match value {
                    Value::Plan(p) => p.record(engine.workspace),
                    other => other,
                });
                if self.fields.contains_key("solution") {
                    self.fields.insert("solution".into(), value.clone());
                }
                self.fields.insert("value".into(), value);
            }
            Err(e) => {
                self.fields
                    .insert("errors".into(), QueryValue::strings([e]));
            }
        }
    }
    pub fn materialize(mut self, engine: &mut Engine<'_>) -> QueryValue {
        self.evaluate(engine);
        QueryValue::Object(self.fields)
    }
}
pub(crate) fn source(ws: &Workspace, path: &Path, span: Span) -> QueryValue {
    let mut uri = crate::paths::file_url(path)
        .map(|u| u.to_string())
        .unwrap_or_default();
    uri.push_str(&format!("#L{}", span.line + 1));
    QueryValue::object([
        ("path", QueryValue::text(path.to_string_lossy())),
        ("uri", QueryValue::text(uri)),
        ("line", QueryValue::count(span.line + 1)),
        (
            "range",
            QueryValue::from_json(json!(span.range(&ws.documents[path].text))),
        ),
    ])
}
fn base(ws: &Workspace, path: &Path, line: usize, kind: RecordKind, title: &str) -> Record {
    let span = Span::new(line, 0, ws.documents[path].line(line).len());
    let QueryValue::Object(fields) = QueryValue::object([
        ("kind", QueryValue::text(kind.as_str())),
        ("title", QueryValue::text(title)),
        ("line", QueryValue::count(line)),
        (
            "anchor",
            QueryValue::from_json(json!(ws.documents[path].line_end(line))),
        ),
        ("source", source(ws, path, span)),
        ("errors", QueryValue::Array(vec![])),
    ]) else {
        unreachable!()
    };
    Record::projected(path.into(), fields)
}
fn schedule(record: &mut Record) {
    for key in ["due", "scheduled", "at", "at_date", "estimate", "parent"] {
        record.fields.insert(key.into(), QueryValue::Null);
    }
    for key in ["tags", "blocked_by"] {
        record.fields.insert(key.into(), QueryValue::Array(vec![]));
    }
    record
        .fields
        .insert("done".into(), QueryValue::boolean(false));
    record
        .fields
        .insert("leaf".into(), QueryValue::boolean(true));
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
                let mut r = base(
                    ws,
                    &row.table.path,
                    cell.span.line,
                    RecordKind::Decision,
                    &doc.definitions[plan.definition].named.name,
                );
                r.fields
                    .insert("value".into(), QueryValue::from_value(value.clone()));
                r.fields.insert(
                    "plan".into(),
                    QueryValue::text(&doc.definitions[plan.definition].named.name),
                );
                r.fields.insert(
                    "anchor".into(),
                    QueryValue::from_json(json!(
                        cell.span.range(&ws.documents[&row.table.path].text).end
                    )),
                );
                records.push(r);
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
        let mut r = base(ws, path, day.line, RecordKind::Day, doc.line(day.line));
        r.fields
            .insert("value".into(), QueryValue::from_value(value));
        records.push(r);
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
        let mut r = base(ws, path, line, RecordKind::Timer, name);
        r.fields.extend([
            ("name".into(), QueryValue::text(name)),
            ("anchor".into(), QueryValue::from_json(json!(anchor))),
            ("definition".into(), QueryValue::boolean(definition)),
            ("inlay".into(), QueryValue::boolean(inlay)),
            ("value".into(), QueryValue::from_value(timer.record())),
            (
                "origin".into(),
                timer
                    .origin
                    .as_ref()
                    .map(|origin| {
                        QueryValue::object([
                            (
                                "document",
                                QueryValue::text(
                                    crate::paths::file_url(&origin.path).unwrap().as_str(),
                                ),
                            ),
                            ("name", QueryValue::text(&ws.named(origin).name)),
                        ])
                    })
                    .unwrap_or(QueryValue::Null),
            ),
        ]);
        records.push(r);
    }
}

fn links(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for link in &doc.links {
        let mut r = base(ws, path, link.span.line, RecordKind::Link, &link.target);
        r.resource = Some(crate::resources::Resource {
            target: link.target.clone(),
            origin: Some(path.into()),
        });
        r.fields
            .insert("url".into(), QueryValue::text(&link.target));
        r.fields
            .insert("source".into(), source(ws, path, link.span));
        r.fields.insert(
            "anchor".into(),
            QueryValue::from_json(json!(link.span.range(&doc.text).end)),
        );
        records.push(r);
    }
}

fn sections(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for section in &doc.sections {
        let mut r = base(ws, path, section.line, RecordKind::Section, &section.title);
        r.fields
            .insert("end_line".into(), QueryValue::count(section.end_line));
        r.fields
            .insert("level".into(), QueryValue::count(section.level));
        records.push(r);
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
        let mut r = expression_record(
            ws,
            path,
            RecordKind::Calculation,
            &calculation.source,
            calculation.span,
            engine.eval_at(path, &calculation.source, calculation.span),
        );
        let end = calculation.span.end + usize::from(calculation.bracketed);
        r.fields.insert(
            "anchor".into(),
            QueryValue::from_json(json!(
                Span::new(calculation.span.line, end, end)
                    .range(&doc.text)
                    .end
            )),
        );
        r.fields.insert(
            "bracketed".into(),
            QueryValue::boolean(calculation.bracketed),
        );
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
        let expression = reference.expression();
        let end = reference.end()
            + doc.line(reference.span.line)[reference.end()..]
                .find(']')
                .unwrap_or(0)
            + 1;
        let mut r = expression_record(
            ws,
            path,
            RecordKind::Reference,
            &expression,
            reference.span,
            engine.eval(path, &expression),
        );
        r.fields.insert(
            "anchor".into(),
            QueryValue::from_json(json!(
                Span::new(reference.span.line, end, end)
                    .range(&doc.text)
                    .end
            )),
        );
        r.fields
            .insert("name".into(), QueryValue::text(&reference.name));
        r.fields.insert(
            "property".into(),
            reference
                .property
                .as_ref()
                .map(QueryValue::text)
                .unwrap_or(QueryValue::Null),
        );
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
                let expression = cell
                    .expression
                    .as_ref()
                    .map(|(s, _)| s.as_str())
                    .unwrap_or(&cell.source);
                let mut r =
                    expression_record(ws, path, RecordKind::Cell, expression, cell.span, value);
                r.fields.insert(
                    "anchor".into(),
                    QueryValue::from_json(json!(cell.span.range(&doc.text).end)),
                );
                r.fields
                    .insert("computed".into(), QueryValue::boolean(cell.calculated()));
                r.fields.insert(
                    "table".into(),
                    QueryValue::text(&doc.definitions[table.definition].named.name),
                );
                r.fields.insert("row".into(), QueryValue::count(row));
                r.fields.insert(
                    "column".into(),
                    table
                        .columns
                        .get(column)
                        .map(|c| QueryValue::text(&c.name))
                        .unwrap_or(QueryValue::Null),
                );
                records.push(r);
            }
        }
    }
}

fn notes(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    let mut r = base(
        ws,
        path,
        0,
        RecordKind::Note,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    );
    r.fields.insert("text".into(), QueryValue::text(&doc.text));
    records.push(r);
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
        let mut r = base(ws, path, task.line, RecordKind::Task, &task.title);
        schedule(&mut r);
        r.fields.insert("leaf".into(), QueryValue::boolean(leaf));
        r.fields
            .insert("checked".into(), QueryValue::boolean(task.checked));
        r.fields.insert(
            "done".into(),
            QueryValue::boolean(engine.task_done(path, i)),
        );
        r.fields.insert(
            "name".into(),
            task.named
                .as_ref()
                .map(|n| QueryValue::text(&n.name))
                .unwrap_or(QueryValue::Null),
        );
        r.fields.insert(
            "parent".into(),
            task.parent
                .map(|i| source(ws, path, doc.tasks[i].checkbox))
                .unwrap_or(QueryValue::Null),
        );
        r.fields
            .insert("tags".into(), QueryValue::strings(task.tags.clone()));
        r.fields.insert(
            "attributes".into(),
            QueryValue::Object(
                task.attributes
                    .iter()
                    .map(|(k, a)| (k.clone(), QueryValue::text(&a.value)))
                    .collect(),
            ),
        );
        let mut errors = Vec::new();
        let mut schedule_values = Vec::new();
        for key in ["due", "scheduled", "at", "estimate"] {
            if let Some(a) = task.attributes.get(key) {
                let value = if key == "estimate" {
                    engine.eval(path, &a.value)
                } else {
                    engine.when(path, &a.value)
                };
                if key != "estimate" {
                    schedule_values.push(QueryValue::object([
                        ("key", QueryValue::text(key)),
                        (
                            "value",
                            value
                                .as_ref()
                                .ok()
                                .and_then(|v| ctx.date(v))
                                .map(|v| QueryValue::Scalar(Value::Date(v)))
                                .unwrap_or(QueryValue::Null),
                        ),
                        (
                            "error",
                            value
                                .as_ref()
                                .err()
                                .map(QueryValue::text)
                                .unwrap_or(QueryValue::Null),
                        ),
                    ]));
                }
                match value {
                    Ok(Value::Duration(s)) if key == "estimate" && s >= 0 => {
                        r.fields
                            .insert(key.into(), QueryValue::Scalar(Value::Duration(s)));
                    }
                    Ok(v) if key != "estimate" && ctx.date(&v).is_some() => {
                        let date = ctx.date(&v);
                        r.fields.insert(
                            key.into(),
                            if key == "at" {
                                QueryValue::from_value(v)
                            } else {
                                date_field(date)
                            },
                        );
                        if key == "at" {
                            r.fields.insert("at_date".into(), date_field(date));
                        }
                    }
                    Ok(_) => errors.push(format!(
                        "@{key}: expected {}",
                        if key == "estimate" {
                            "a nonnegative duration"
                        } else {
                            "a date or timestamp"
                        }
                    )),
                    Err(e) => errors.push(format!("@{key}: {e}")),
                }
            }
        }
        if r.fields["due"] == QueryValue::Null && task.attributes.contains_key("every") {
            r.fields.insert("due".into(), date_field(Some(ctx.today())));
        }
        match engine.blocked(path, i) {
            Ok(v) => {
                r.fields.insert("blocked_by".into(), QueryValue::strings(v));
            }
            Err(e) => {
                r.fields
                    .insert("blocked_error".into(), QueryValue::text(&e));
                errors.push(e);
            }
        }
        r.fields
            .entry("blocked_error".into())
            .or_insert(QueryValue::Null);
        r.fields
            .insert("schedule".into(), QueryValue::Array(schedule_values));
        r.fields.insert(
            "children".into(),
            QueryValue::Array(
                doc.tasks
                    .iter()
                    .enumerate()
                    .filter(|(_, child)| child.parent == Some(i))
                    .map(|(index, child)| {
                        QueryValue::object([
                            ("line", QueryValue::count(child.line)),
                            ("done", QueryValue::boolean(engine.task_done(path, index))),
                        ])
                    })
                    .collect(),
            ),
        );
        let timer = task
            .attributes
            .get("timer")
            .and_then(|a| engine.eval(path, &a.value).ok());
        r.fields.insert(
            "timer".into(),
            match timer {
                Some(Value::Timer(timer)) => QueryValue::from_value(timer.record()),
                _ => QueryValue::Null,
            },
        );
        r.fields
            .insert("errors".into(), QueryValue::strings(errors));
        records.push(r);
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
        let mut r = base(ws, path, event.line, RecordKind::Event, &event.title);
        schedule(&mut r);
        match engine.when(path, &event.attributes["at"].value) {
            Ok(v) if ctx.date(&v).is_some() => {
                r.fields.insert("at_date".into(), date_field(ctx.date(&v)));
                r.fields.insert("at".into(), QueryValue::from_value(v));
            }
            other => {
                r.fields.insert(
                    "errors".into(),
                    QueryValue::strings([other
                        .err()
                        .unwrap_or_else(|| "@at requires a date or timestamp".into())]),
                );
            }
        }
        records.push(r);
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
            let mut r = base(
                ws,
                path,
                stop.line,
                RecordKind::Stop,
                &crate::itinerary::label(&ws.modules, stop),
            );
            schedule(&mut r);
            r.fields.insert("at_date".into(), date_field(date));
            let at = date.and_then(|d| {
                ctx.now
                    .offset()
                    .from_local_datetime(&d.and_time(stop.time))
                    .single()
            });
            r.fields.insert(
                "at".into(),
                at.map(|d| QueryValue::Scalar(Value::DateTime(d)))
                    .unwrap_or_else(|| QueryValue::text(stop.time.format("%H:%M").to_string())),
            );
            records.push(r);
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
                        let mut r = base(
                            ws,
                            path,
                            table.unwrap().rows[row][0].span.line,
                            RecordKind::Row,
                            &def.named.name,
                        );
                        r.fields
                            .insert("table".into(), QueryValue::text(&def.named.name));
                        r.fields.insert(
                            "cells".into(),
                            QueryValue::Object(
                                t.columns
                                    .iter()
                                    .cloned()
                                    .zip(values.iter().cloned().map(QueryValue::from_value))
                                    .collect(),
                            ),
                        );
                        records.push(r);
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
        let mut r = base(
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
        r.fields
            .insert("name".into(), QueryValue::text(&def.named.name));
        r.fields
            .insert("expression".into(), QueryValue::text(&def.source));
        r.fields.insert("value".into(), QueryValue::Null);
        r.fields.insert("type".into(), QueryValue::Null);
        r.fields.insert("display".into(), QueryValue::Null);
        r.fields
            .insert("computed".into(), QueryValue::boolean(def.expression));
        r.fields.insert(
            "anchor".into(),
            QueryValue::from_json(json!(def.end.range(&doc.text).end)),
        );
        if plan {
            r.fields.insert("solution".into(), QueryValue::Null);
        }
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
        let mut r = base(ws, path, span.line, RecordKind::Resource, target);
        r.fields.insert("source".into(), source(ws, path, span));
        r.fields.insert("target".into(), QueryValue::text(target));
        r.fields.insert(
            "metadata".into(),
            ws.cache
                .get(target)
                .map(|m| QueryValue::from_json(json!(m)))
                .unwrap_or(QueryValue::Null),
        );
        records.push(r);
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
        let mut r = base(
            ws,
            path,
            d.range.start.line as usize,
            RecordKind::Diagnostic,
            &d.message,
        );
        if let Some(QueryValue::Object(s)) = r.fields.get_mut("source") {
            s.insert("range".into(), QueryValue::from_json(json!(d.range)));
        }
        r.fields
            .insert("message".into(), QueryValue::text(d.message));
        r.fields.insert(
            "severity".into(),
            QueryValue::text(crate::diagnostics::severity_name(d.severity)),
        );
        r.fields
            .insert("code".into(), QueryValue::from_json(json!(d.code)));
        records.push(r);
    }
}

fn expression_record(
    ws: &Workspace,
    path: &Path,
    kind: RecordKind,
    expression: &str,
    span: Span,
    value: Result<Value, String>,
) -> Record {
    let mut record = base(ws, path, span.line, kind, expression);
    if let Ok(Value::Resource(resource)) = &value {
        record.resource = Some(resource.clone());
    }
    record
        .fields
        .insert("source".into(), source(ws, path, span));
    record
        .fields
        .insert("expression".into(), QueryValue::text(expression));
    let (value, kind, display, errors) = match value {
        Ok(v) => {
            let kind = QueryValue::text(v.type_name());
            let display = QueryValue::text(v.display());
            (
                QueryValue::from_value(v),
                kind,
                display,
                QueryValue::Array(vec![]),
            )
        }
        Err(e) => (
            QueryValue::Null,
            QueryValue::Null,
            QueryValue::Null,
            QueryValue::strings([e]),
        ),
    };
    record.fields.extend([
        ("value".into(), value),
        ("type".into(), kind),
        ("display".into(), display),
        ("errors".into(), errors),
    ]);
    record
}
