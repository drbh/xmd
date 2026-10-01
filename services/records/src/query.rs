//! Queries: one expression over lazy workspace bindings, on the note evaluator.
use crate::{Collection, DiagnosticSource, Records, View, inspection, value as q};
use lang::document::ExprImports;
use lang::eval::Workspace;
use lang::eval::engine::{self, Bindings, Engine, Value};
use lang::syntax::{Expr, Parser};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Reading the notes a workspace imports from disk. `records` does no I/O:
/// a native host supplies this (`native::DiskFiles`), and a browser has none.
pub trait NoteFiles: Send + Sync {
    /// Follow explicit imports, reading every imported note not yet loaded.
    fn load_imports(&self, workspace: &mut Workspace);
    /// Read one note, and what it imports, unless it is already loaded.
    fn include_file(&self, workspace: &mut Workspace, path: &Path) -> Result<(), String>;
}

/// A query is one expression of the language, evaluated with every
/// collection bound by name.
#[derive(Clone, Debug)]
pub struct Query {
    expression: Expr,
}
#[derive(Clone, Debug)]
pub struct QueryResult {
    pub rows: Vec<Value>,
}
impl QueryResult {
    /// The rows as JSON, the shape every host sends a client.
    pub fn json(&self) -> serde_json::Value {
        self.rows.iter().map(engine::value_json).collect()
    }
}

impl Query {
    /// Read the notes this query imports, through a host's `files`.
    pub fn load_imports(
        &self,
        workspace: &mut lang::eval::Workspace,
        only: Option<&Path>,
        files: &dyn crate::NoteFiles,
    ) {
        let context = Self::scope(workspace, only);
        for id in self.expression.note_imports() {
            if let Ok(target) = lang::document::note_path(&context, &id) {
                // Like note imports, report missing dependencies only if evaluation reads them.
                let _ = files.include_file(workspace, &target);
            }
        }
    }
    /// The note a query's own expressions evaluate in.
    fn scope(ws: &lang::eval::Workspace, only: Option<&Path>) -> PathBuf {
        only.map(Path::to_path_buf)
            .unwrap_or_else(|| ws.root().join(lang::common::note_file("__query__")))
    }
    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > 65_536 {
            return Err("Queries are limited to 64 KiB".into());
        }
        Ok(Self {
            expression: Parser::parse(source)?,
        })
    }
}

/// Collections are loaded on demand, so unrelated features and errors are not evaluated.
struct WorkspaceBindings {
    only: Option<PathBuf>,
    records: Arc<Records>,
    diagnostics: DiagnosticSource,
    cache: Mutex<BTreeMap<String, Value>>,
}
impl Bindings for WorkspaceBindings {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<lang::eval::EvalResult<Value>> {
        // A declared collection is bound only while a module declares it.
        let collection = name
            .parse::<Collection>()
            .ok()
            .filter(|c| !c.is_declared() || engine.workspace().modules().declaring(name).is_some());
        if name != "graph" && collection.is_none() {
            return None;
        }
        if let Some(value) = self.cache.lock().expect("query cache poisoned").get(name) {
            return Some(Ok(value.clone()));
        }
        let result = if name == "graph" {
            Ok(inspection::graph(engine.workspace(), self.only.as_deref()))
        } else {
            self.records.view(
                engine,
                self.only.as_deref(),
                collection.expect("checked above"),
                View::Queried,
                self.diagnostics,
            )
        };
        if let Ok(value) = &result {
            self.cache
                .lock()
                .expect("query cache poisoned")
                .insert(name.into(), value.clone());
        }
        Some(result.map_err(Into::into))
    }
}

/// `only` restricts input records to one indexed document while retaining
/// workspace-wide name resolution. Collections are read from `records`, and
/// what they build is left there for the next reader.
pub fn execute(
    request: &lang::eval::RequestContext<'_>,
    records: Arc<Records>,
    query: &Query,
    only: Option<&Path>,
    diagnostics: DiagnosticSource,
) -> Result<QueryResult, String> {
    let ws = request.workspace();
    let mut engine = request.engine();
    if let Some(path) = only
        && !ws.documents().contains_key(path)
    {
        return Err(format!("Document is not indexed: {}", path.display()));
    }
    let context = Query::scope(ws, only);
    let bindings = Arc::new(WorkspaceBindings {
        only: only.map(Path::to_path_buf),
        records,
        diagnostics,
        cache: Mutex::new(BTreeMap::new()),
    });
    let value = engine.bound_expr(&context, &query.expression, bindings)?;
    let rows = match q::query_value(value) {
        Value::List(rows) => Arc::unwrap_or_clone(rows).into_inner(),
        value => vec![value],
    };
    Ok(QueryResult { rows })
}
