//! The lazy `Record` machinery and the parts every catalog record shares.
use super::RecordKind;
use super::value as q;
use lang::common::Span;
use lang::eval::engine::{Engine, Value};
use lang::eval::plans::PlanValue;
use lang::eval::resources::ResourcePresenting;
use lang::eval::{RecordFields, Symbol, Workspace, record};
use lsp_types::{Position, Range};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

record! {
    /// Where a record was found. Every record carries one under `source`.
    #[derive(Clone, Debug)]
    pub(crate) struct SourceRef {
        path: String,
        uri: String,
        line: usize,
        range: Value,
    }
}
impl SourceRef {
    pub(crate) fn new(ws: &Workspace, path: &Path, span: Span) -> Self {
        Self::spanning(path, span.line, span.range(&ws.documents()[path].text))
    }
    fn spanning(path: &Path, line: usize, range: Range) -> Self {
        let mut uri = lang::common::file_url(path)
            .map(|u| u.to_string())
            .unwrap_or_default();
        uri.push_str(&format!("#L{}", line + 1));
        Self {
            path: path.to_string_lossy().into(),
            uri,
            line: line + 1,
            range: q::range(range),
        }
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
    /// A record found at `span`. Its inlay sits at `anchor`, or else at the end
    /// of its line.
    pub(super) fn at(
        ws: &Workspace,
        path: &Path,
        kind: RecordKind,
        title: impl Into<String>,
        span: Span,
        anchor: Option<Position>,
    ) -> Self {
        let range = span.range(&ws.documents()[path].text);
        Self::spanning(ws, path, kind, title, span.line, range, anchor)
    }
    /// A record that is its whole line, with its inlay at the line's end.
    pub(super) fn line(
        ws: &Workspace,
        path: &Path,
        kind: RecordKind,
        title: impl Into<String>,
        row: usize,
    ) -> Self {
        Self::at(
            ws,
            path,
            kind,
            title,
            ws.documents()[path].line_span(row),
            None,
        )
    }
    /// The same, for a range already in editor coordinates.
    pub(super) fn spanning(
        ws: &Workspace,
        path: &Path,
        kind: RecordKind,
        title: impl Into<String>,
        line: usize,
        range: Range,
        anchor: Option<Position>,
    ) -> Self {
        let anchor = anchor.unwrap_or_else(|| ws.documents()[path].line_end(line));
        Self {
            kind,
            title: title.into(),
            line,
            anchor: q::position(anchor),
            source: SourceRef::spanning(path, line, range),
            errors: Vec::new(),
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
const NAME: &str = "name";
const EXPRESSION: &str = "expression";
const PROPERTY: &str = "property";

/// Fields a record only produces when they are read: evaluating a definition, or
/// asking the workspace for a link's presentation or a name's hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
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
    pub(super) fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Definitions are evaluated only when their value, type, solution or errors are read.
#[derive(Clone, Debug)]
pub struct Record {
    pub fields: BTreeMap<String, Value>,
    pub path: PathBuf,
    pub(super) deferred: Option<Symbol>,
    pub(super) resource: Option<lang::eval::resources::Resource>,
}
impl Record {
    pub(crate) fn typed(path: &Path, record: impl RecordFields) -> Self {
        Self {
            path: path.into(),
            fields: record.fields(),
            deferred: None,
            resource: None,
        }
    }
    pub fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<Value, String> {
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
                .map(|symbol| q::text(analysis::symbol_hover(&engine.request(), &symbol)))
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
                let value = q::query_value(match value.downcast::<PlanValue>() {
                    Some(p) => p.record(engine.workspace()),
                    None => value,
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
                    Value::List(vec![q::text(e.to_string())]),
                );
            }
        }
    }
    pub fn materialize(mut self, engine: &mut Engine<'_>) -> Value {
        self.evaluate(engine);
        Value::Record(self.fields)
    }
    /// The record a query reads: every field, and a named record's hover.
    pub(crate) fn queried(mut self, engine: &mut Engine<'_>) -> Value {
        if let Some(hover) = self.hover(engine) {
            self.fields.insert(LazyField::Hover.as_str().into(), hover);
        }
        self.materialize(engine)
    }
}
