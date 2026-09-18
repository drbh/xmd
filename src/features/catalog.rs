//! Typed, host-independent workspace records. Reading a catalog never performs I/O.
use crate::{
    document::Span,
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
                                    ("op", Self::text(&c.op)),
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
                ("state", Self::text(t.state())),
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
        match self {
            Self::Object(fields) => fields
                .get(key)
                .cloned()
                .ok_or_else(|| format!("Unknown field '{key}'")),
            Self::Null => Ok(Self::Null),
            Self::Scalar(_) => match self.json() {
                serde_json::Value::Object(fields) => fields
                    .get(key)
                    .cloned()
                    .map(Self::from_json)
                    .ok_or_else(|| format!("Unknown field '{key}'")),
                _ => Err(format!("Cannot read field '{key}' from this scalar")),
            },
            Self::Array(values) => values
                .iter()
                .map(|v| v.property(key))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Array),
        }
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
}
impl Record {
    pub fn projected(path: PathBuf, fields: BTreeMap<String, QueryValue>) -> Self {
        Self {
            path,
            fields,
            deferred: None,
        }
    }
    pub fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<QueryValue, String> {
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
                self.fields
                    .insert("type".into(), QueryValue::text(value.type_name()));
                self.fields
                    .insert("display".into(), QueryValue::text(value.display()));
                let value = QueryValue::from_value(value);
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
fn source(ws: &Workspace, path: &Path, span: Span) -> QueryValue {
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
fn base(ws: &Workspace, path: &Path, line: usize, kind: &str, title: &str) -> Record {
    let span = Span::new(line, 0, ws.documents[path].line(line).len());
    let QueryValue::Object(fields) = QueryValue::object([
        ("kind", QueryValue::text(kind)),
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

pub const COLLECTIONS: &[&str] = &[
    "links",
    "tasks",
    "events",
    "stops",
    "entries",
    "values",
    "plans",
    "tables",
    "rows",
    "resources",
    "diagnostics",
    "notes",
    "sections",
    "calculations",
    "references",
    "cells",
];

pub(crate) fn collect(
    ws: &Workspace,
    collection: &str,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
) -> Result<Vec<Record>, String> {
    collect_document(ws, collection, ctx, engine, None)
}
/// The query API and feature modules read the same semantic records.
pub(crate) fn collect_document(
    ws: &Workspace,
    collection: &str,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
) -> Result<Vec<Record>, String> {
    if !COLLECTIONS.contains(&collection) {
        return Err(format!(
            "Unknown collection '{collection}'; expected {}",
            COLLECTIONS.join(", ")
        ));
    }
    let mut records = Vec::new();
    for (path, doc) in &ws.documents {
        if only.is_some_and(|wanted| wanted != path) {
            continue;
        }
        if collection == "links" {
            for link in &doc.links {
                let mut r = base(ws, path, link.span.line, "link", &link.target);
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
        if collection == "sections" {
            for section in &doc.sections {
                let mut r = base(ws, path, section.line, "section", &section.title);
                r.fields
                    .insert("end_line".into(), QueryValue::count(section.end_line));
                r.fields
                    .insert("level".into(), QueryValue::count(section.level));
                records.push(r);
            }
        }
        if collection == "calculations" {
            for calculation in &doc.calculations {
                let mut r = expression_record(
                    ws,
                    path,
                    "calculation",
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
        if collection == "references" {
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
                    "reference",
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
        if collection == "cells" {
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
                            expression_record(ws, path, "cell", expression, cell.span, value);
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
        if collection == "notes" {
            let mut r = base(
                ws,
                path,
                0,
                "note",
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref(),
            );
            r.fields.insert("text".into(), QueryValue::text(&doc.text));
            records.push(r);
        }
        if matches!(collection, "tasks" | "entries") {
            let parents: std::collections::BTreeSet<_> =
                doc.tasks.iter().filter_map(|t| t.parent).collect();
            for (i, task) in doc.tasks.iter().enumerate() {
                let leaf = !parents.contains(&i);
                if collection == "entries" && !leaf {
                    continue;
                }
                let mut r = base(ws, path, task.line, "task", &task.title);
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
                for key in ["due", "scheduled", "at", "estimate"] {
                    if let Some(a) = task.attributes.get(key) {
                        let value = if key == "estimate" {
                            engine.eval(path, &a.value)
                        } else {
                            engine.when(path, &a.value)
                        };
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
                    Err(e) => errors.push(e),
                }
                r.fields
                    .insert("errors".into(), QueryValue::strings(errors));
                records.push(r);
            }
        }
        if matches!(collection, "events" | "entries") {
            for event in &doc.events {
                let mut r = base(ws, path, event.line, "event", &event.title);
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
        if matches!(collection, "stops" | "entries") {
            let dates = crate::itinerary::dates(&doc.days, ctx.today());
            for (day, date) in doc.days.iter().zip(dates) {
                for stop in &day.stops {
                    let mut r = base(ws, path, stop.line, "stop", &crate::itinerary::label(stop));
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
                            .unwrap_or_else(|| {
                                QueryValue::text(stop.time.format("%H:%M").to_string())
                            }),
                    );
                    records.push(r);
                }
            }
        }
        if matches!(collection, "values" | "plans" | "tables" | "rows") {
            for (i, def) in doc.definitions.iter().enumerate() {
                let plan = doc.plans.iter().any(|p| p.definition == i);
                let table = doc.tables.iter().find(|t| t.definition == i);
                if (collection == "plans" && !plan)
                    || (matches!(collection, "tables" | "rows") && table.is_none())
                {
                    continue;
                }
                let symbol = Symbol {
                    path: path.clone(),
                    kind: SymbolKind::Definition(i),
                };
                if collection == "rows" {
                    match engine.symbol(&symbol) {
                        Ok(Value::Table(t)) => {
                            for (row, values) in t.rows.iter().enumerate() {
                                let mut r = base(
                                    ws,
                                    path,
                                    table.unwrap().rows[row][0].span.line,
                                    "row",
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
                        "plan"
                    } else if table.is_some() {
                        "table"
                    } else {
                        "value"
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
        }
        if collection == "resources" {
            let targets = doc.links.iter().map(|l| (l.span, l.target.as_str())).chain(
                doc.definitions
                    .iter()
                    .filter(|d| {
                        !d.expression && crate::resources::Resource::parse(&d.source).is_some()
                    })
                    .map(|d| (d.value_span, d.source.as_str())),
            );
            for (span, target) in targets {
                let mut r = base(ws, path, span.line, "resource", target);
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
        if collection == "diagnostics" {
            for d in crate::diagnostics::collect_native_in(&engine.request(), path, false) {
                let mut r = base(
                    ws,
                    path,
                    d.range.start.line as usize,
                    "diagnostic",
                    &d.message,
                );
                if let Some(QueryValue::Object(s)) = r.fields.get_mut("source") {
                    s.insert("range".into(), QueryValue::from_json(json!(d.range)));
                }
                r.fields
                    .insert("message".into(), QueryValue::text(d.message));
                r.fields.insert(
                    "severity".into(),
                    QueryValue::text(match d.severity {
                        Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
                        Some(lsp_types::DiagnosticSeverity::INFORMATION) => "information",
                        Some(lsp_types::DiagnosticSeverity::HINT) => "hint",
                        _ => "error",
                    }),
                );
                r.fields
                    .insert("code".into(), QueryValue::from_json(json!(d.code)));
                records.push(r);
            }
        }
    }
    Ok(records)
}

fn expression_record(
    ws: &Workspace,
    path: &Path,
    kind: &str,
    expression: &str,
    span: Span,
    value: Result<Value, String>,
) -> Record {
    let mut record = base(ws, path, span.line, kind, expression);
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
