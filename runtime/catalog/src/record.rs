//! The lazy `Record` machinery and the parts every catalog record shares.
use super::RecordKind;
use super::value as q;
use lang::common::Span;
use lang::eval::engine::{Engine, Value};
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
        Self::spanning(path, span.line, span.range(&ws.documents()[path]))
    }
    fn spanning(path: &Path, line: usize, range: Range) -> Self {
        Self {
            path: path.to_string_lossy().into(),
            uri: format!("{}#L{}", note_uri(path), line + 1),
            line: line + 1,
            range: q::range(range),
        }
    }
}

/// The range of a line's text, without its indentation or trailing space.
pub(super) fn text_range(doc: &lang::model::Document, row: usize) -> Value {
    let raw = doc.line(row);
    let start = raw.len() - raw.trim_start().len();
    q::range(Span::new(row, start, raw.trim_end().len().max(start)).range(doc))
}

thread_local! {
    /// The note a record was last found in, and its URI: a collection's
    /// records come from one note at a time, so its URI is spelled once.
    static NOTE_URI: std::cell::RefCell<(PathBuf, String)> = Default::default();
}

/// The URI of the note at `path`.
fn note_uri(path: &Path) -> String {
    NOTE_URI.with(|last| {
        let mut last = last.borrow_mut();
        if last.0 != path || last.1.is_empty() {
            let uri = lang::common::file_url(path)
                .map(|u| u.to_string())
                .unwrap_or_default();
            *last = (path.to_path_buf(), uri);
        }
        last.1.clone()
    })
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
        let range = span.range(&ws.documents()[path]);
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
pub(super) const NAME: &str = "name";
const EXPRESSION: &str = "expression";
const FORM: &str = "form";
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
    /// What the module of the form a definition calls records of it.
    Record,
    Errors,
    Display,
}
impl LazyField {
    pub(super) fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Definitions are evaluated only when their value, type, display, errors or
/// a form's record are read.
///
/// A record remembers what its lazy fields produced, and whether producing
/// each read the clock, so a cached record answers a later reader as the
/// first one was answered, marking that reader's engine the same way.
#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) fields: BTreeMap<String, Value>,
    pub(crate) path: PathBuf,
    pub(super) deferred: Option<Symbol>,
    pub(super) resource: Option<lang::eval::resources::Resource>,
    lazy: Lazy,
}

/// The lazy fields a record has already produced.
#[derive(Clone, Debug, Default)]
struct Lazy {
    /// Whether evaluating the definition read the clock.
    evaluated: bool,
    /// The hover, and whether wording it read the clock more finely than
    /// the date. Unlike the other fields, a hover never marks its reader.
    hover: Option<(Option<Value>, bool)>,
    /// The presentation, and whether making it read the clock.
    presentation: Option<(Value, bool)>,
    full: Option<Value>,
    queried: Option<Value>,
}

impl Record {
    pub(crate) fn typed(path: &Path, record: impl RecordFields) -> Self {
        Self::built(path, record.fields())
    }
    /// A record a module built, kept as it returned it.
    pub(crate) fn built(path: &Path, fields: BTreeMap<String, Value>) -> Self {
        Self {
            path: path.into(),
            fields,
            deferred: None,
            resource: None,
            lazy: Lazy::default(),
        }
    }
    fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<Value, String> {
        match key.parse::<LazyField>() {
            Ok(LazyField::Presentation) => return Ok(self.presentation(engine)),
            Ok(LazyField::Hover) => {
                if let Some(hover) = self.hover(engine) {
                    return Ok(hover);
                }
            }
            Ok(LazyField::Value | LazyField::Type | LazyField::Errors | LazyField::Display) => {
                self.evaluate(engine)
            }
            // Only a form's definition has a record to evaluate for.
            Ok(LazyField::Record) if self.calls_form() => self.evaluate(engine),
            _ => {}
        }
        self.fields
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Unknown field '{key}'"))
    }
    /// Whether the record is a definition that calls a form.
    fn calls_form(&self) -> bool {
        self.fields
            .get(FORM)
            .is_some_and(|form| *form != Value::Null)
    }
    fn presentation(&mut self, engine: &mut Engine<'_>) -> Value {
        self.evaluate(engine);
        let (value, time_dependent) = self
            .lazy
            .presentation
            .get_or_insert_with(|| match &self.resource {
                None => (Value::Null, false),
                Some(resource) => {
                    // An engine of its own, so the clock it reads is this
                    // presentation's alone.
                    let mut own = engine.request().engine();
                    let view = resource.presentation(&mut own, &self.path);
                    let value = Value::record(
                        Presentation {
                            label: view.label,
                            hover: view.hover,
                            known: view.known_link,
                        }
                        .fields(),
                    );
                    (value, own.time_dependent() || view.time_dependent)
                }
            })
            .clone();
        engine.mark_time_dependent(time_dependent);
        value
    }
    /// A named record hovers as its symbol does; a property reference as itself.
    fn hover(&mut self, engine: &Engine<'_>) -> Option<Value> {
        self.lazy
            .hover
            .get_or_insert_with(|| {
                let Some(Value::Text(name)) = self.fields.get(NAME) else {
                    return (None, false);
                };
                let expression = self.fields.get(EXPRESSION).cloned().unwrap_or(Value::Null);
                if self.fields.get(PROPERTY).is_some_and(|v| *v != Value::Null) {
                    return (Some(expression), false);
                }
                match engine.workspace().resolve(&self.path, name) {
                    Ok(symbol) => {
                        let hover = analysis::symbol_hover(&engine.request(), &symbol);
                        (Some(q::text(hover.text)), hover.reads_clock)
                    }
                    Err(_) => (Some(expression), false),
                }
            })
            .0
            .clone()
    }
    /// Whether the hover this record has worded read the clock more finely
    /// than the date.
    pub(crate) fn hover_reads_clock(&self) -> bool {
        self.lazy.hover.as_ref().is_some_and(|(_, read)| *read)
    }
    /// Evaluate the definition behind the record, once, and mark `engine`
    /// with whether that read the clock.
    fn evaluate(&mut self, engine: &mut Engine<'_>) {
        if let Some(symbol) = self.deferred.take() {
            let mut own = engine.request().engine();
            self.evaluate_with(&mut own, &symbol);
            self.lazy.evaluated = own.time_dependent();
        }
        engine.mark_time_dependent(self.lazy.evaluated);
    }
    fn evaluate_with(&mut self, engine: &mut Engine<'_>, symbol: &Symbol) {
        match engine.symbol(symbol) {
            Ok(value) => {
                if let Value::Resource(resource) = &value {
                    self.resource = Some(resource.clone());
                }
                self.fields
                    .insert(LazyField::Type.as_str().into(), q::text(value.type_name()));
                self.fields
                    .insert(LazyField::Display.as_str().into(), q::text(value.display()));
                if self.calls_form()
                    && let Some(about) = engine.about(symbol)
                {
                    self.fields.insert(
                        LazyField::Record.as_str().into(),
                        q::query_value(about.record),
                    );
                }
                self.fields
                    .insert(LazyField::Value.as_str().into(), q::query_value(value));
            }
            Err(e) => {
                self.fields.insert(
                    LazyField::Errors.as_str().into(),
                    Value::list(vec![q::text(e.to_string())]),
                );
            }
        }
    }
    /// Every field, the definition evaluated: what a feature module reads
    /// when it does not narrow its inputs.
    pub(super) fn full(&mut self, engine: &mut Engine<'_>) -> Value {
        self.evaluate(engine);
        self.lazy
            .full
            .get_or_insert_with(|| Value::record(self.fields.clone()))
            .clone()
    }
    /// The record as `view` reads it.
    pub(crate) fn read(
        &mut self,
        view: crate::View<'_>,
        engine: &mut Engine<'_>,
    ) -> Result<Value, String> {
        match view {
            crate::View::Full => Ok(self.full(engine)),
            crate::View::Fields(keys) => self.narrowed(keys, engine),
            crate::View::Queried => Ok(self.queried(engine)),
        }
    }
    /// Only the named fields, each read as [`Record::field`] reads it.
    fn narrowed(&mut self, keys: &[String], engine: &mut Engine<'_>) -> Result<Value, String> {
        keys.iter()
            .map(|key| Ok((key.clone(), self.field(key, engine)?)))
            .collect::<Result<_, String>>()
            .map(Value::record)
    }
    /// The record a query reads: every field, and a named record's hover.
    fn queried(&mut self, engine: &mut Engine<'_>) -> Value {
        let hover = self.hover(engine);
        self.evaluate(engine);
        self.lazy
            .queried
            .get_or_insert_with(|| {
                let mut fields = self.fields.clone();
                if let Some(hover) = hover {
                    fields.insert(LazyField::Hover.as_str().into(), hover);
                }
                Value::record(fields)
            })
            .clone()
    }
}
