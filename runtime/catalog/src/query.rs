//! Functional queries over lazy workspace bindings. Legacy pipelines share the evaluator.
use crate::{Collection, DiagnosticSource, Record, inspection, value as q};
use lang::eval::Workspace;
use lang::eval::engine::{self, Bindings, Engine, Expr, Lexeme, Operator, Parser, Value};
use lang::eval::functional;
use lang::model::ExprImports;
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Reading the notes a workspace imports from disk. `catalog` does no I/O:
/// a native host supplies this (`host::DiskFiles`), and a browser has none.
pub trait NoteFiles: Send + Sync {
    /// Follow explicit imports, reading every imported note not yet loaded.
    fn load_imports(&self, workspace: &mut Workspace);
    /// Read one note, and what it imports, unless it is already loaded.
    fn include_file(&self, workspace: &mut Workspace, path: &Path) -> Result<(), String>;
}

#[derive(Clone, Debug)]
pub struct Query {
    sources: Vec<Collection>,
    stages: Vec<Stage>,
    expression: Option<Expr>,
}
#[derive(Clone, Debug)]
enum Stage {
    Where(Expr),
    Select(Expr),
    Sort(Vec<(Expr, bool)>),
    Limit(usize),
    Count,
    Sum(Expr),
    Group(Expr),
}
#[derive(Clone, Debug)]
enum Item {
    Record(Record),
    Value(Value, PathBuf),
}
impl Item {
    fn path(&self) -> &std::path::Path {
        match self {
            Self::Record(r) => &r.path,
            Self::Value(_, p) => p,
        }
    }
    fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<Value, String> {
        match self {
            Self::Record(r) => r.field(key, engine),
            Self::Value(v, _) => v
                .property(key)
                .map(q::query_value)
                .map_err(|e| e.to_string()),
        }
    }
    fn materialize(self, engine: &mut Engine<'_>) -> Value {
        match self {
            Self::Record(r) => r.materialize(engine),
            Self::Value(v, _) => v,
        }
    }
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

/// Only the compatibility pipeline syntax lives here; literals, comments and
/// delimiters come from the language lexer and every expression uses its parser.
fn split(source: &str, separator: char) -> Result<Vec<&str>, String> {
    let separates = |kind: &Lexeme| match kind {
        Lexeme::Op(Operator::Pipe) => separator == '|',
        Lexeme::Comma => separator == ',',
        _ => false,
    };
    let mut parts = Vec::new();
    let mut start = 0;
    let mut stack = Vec::new();
    for token in engine::lex(source)? {
        match token.kind {
            Lexeme::Left => stack.push(')'),
            Lexeme::OpenList => stack.push(']'),
            Lexeme::OpenRecord => stack.push('}'),
            Lexeme::Right | Lexeme::CloseList | Lexeme::CloseRecord => {
                if stack.pop() != source[token.start..token.end].chars().next() {
                    return Err(format!("Unmatched delimiter at byte {}", token.start));
                }
            }
            ref kind if stack.is_empty() && separates(kind) => {
                parts.push(source[start..token.start].trim());
                start = token.end;
            }
            _ => (),
        }
        if stack.len() > 64 {
            return Err("Query nesting exceeds 64 levels".into());
        }
    }
    if !stack.is_empty() {
        return Err("Unclosed delimiter in query".into());
    }
    parts.push(source[start..].trim());
    if parts.iter().any(|s| s.is_empty()) {
        return Err(format!("Empty expression around '{separator}'"));
    }
    Ok(parts)
}
fn expression(source: &str) -> Result<Expr, String> {
    Parser::parse(source)
}
impl Stage {
    fn parse(source: &str) -> Result<Self, String> {
        let tokens = engine::lex(source)?;
        let first = tokens.first().ok_or("Empty pipeline stage")?;
        let last = tokens.last().unwrap();
        let Lexeme::Name(op) = &first.kind else {
            return Err("Expected a pipeline stage name".into());
        };
        let argument = source[first.end..last.end].trim();
        Ok(match op.as_str() {
            "where" => Self::Where(expression(argument)?),
            "select" => Self::Select(expression(argument)?),
            "sort" => Self::Sort(
                split(argument, ',')?
                    .into_iter()
                    .map(|key| {
                        let tokens = engine::lex(key)?;
                        let last = tokens.last().ok_or("Empty sort key")?;
                        let descending = matches!(&last.kind, Lexeme::Name(n) if n == "desc");
                        let direction = tokens.len() > 1
                            && matches!(&last.kind, Lexeme::Name(n) if n == "desc" || n == "asc");
                        Ok((
                            expression(if direction { &key[..last.start] } else { key })?,
                            direction && descending,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            ),
            "limit" => Self::Limit(
                argument
                    .parse()
                    .map_err(|_| "limit requires a nonnegative integer")?,
            ),
            "count" if argument.is_empty() => Self::Count,
            "sum" => Self::Sum(expression(argument)?),
            "group" => Self::Group(expression(argument)?),
            _ => {
                return Err(format!(
                    "Unknown stage '{op}'; use where, select, sort, limit, count, sum or group"
                ));
            }
        })
    }
}
impl Stage {
    fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Where(e) | Self::Select(e) | Self::Sum(e) | Self::Group(e) => vec![e],
            Self::Sort(keys) => keys.iter().map(|(e, _)| e).collect(),
            Self::Limit(_) | Self::Count => Vec::new(),
        }
    }
    /// Aggregates produce rows scoped to `context`, the query's own note.
    fn apply(
        &self,
        items: Vec<Item>,
        engine: &mut Engine<'_>,
        context: &Path,
    ) -> Result<Vec<Item>, String> {
        Ok(match self {
            Self::Where(expr) => items
                .into_iter()
                .filter_map(
                    |mut item| match eval(expr, &mut item, engine).and_then(boolean) {
                        Ok(true) => Some(Ok(item)),
                        Ok(false) => None,
                        Err(e) => Some(Err(e)),
                    },
                )
                .collect::<Result<_, _>>()?,
            Self::Select(expr) => items
                .into_iter()
                .map(|mut item| {
                    let path = item.path().to_path_buf();
                    eval(expr, &mut item, engine).map(|value| Item::Value(value, path))
                })
                .collect::<Result<_, _>>()?,
            Self::Limit(n) => items.into_iter().take(*n).collect(),
            Self::Count => vec![Item::Value(Value::Count(items.len()), context.into())],
            Self::Sum(expr) => {
                let values = items
                    .into_iter()
                    .map(|mut item| eval(expr, &mut item, engine))
                    .collect::<Result<Vec<_>, _>>()?;
                vec![Item::Value(sum(values)?, context.into())]
            }
            Self::Group(expr) => {
                let mut groups: BTreeMap<String, (Value, Vec<Value>)> = BTreeMap::new();
                for mut item in items {
                    let key = eval(expr, &mut item, engine)?;
                    let identity = match &key {
                        Value::Number(n) => n.to_string(),
                        Value::Count(n) => n.to_string(),
                        _ => engine::value_json(&key).to_string(),
                    };
                    groups
                        .entry(identity)
                        .or_insert_with(|| (key, Vec::new()))
                        .1
                        .push(item.materialize(engine));
                }
                groups
                    .into_values()
                    .map(|(key, rows)| {
                        let group = [("key".into(), key), ("rows".into(), Value::List(rows))];
                        Item::Value(Value::Record(group.into()), context.into())
                    })
                    .collect()
            }
            Self::Sort(keys) => sort(items, keys, engine)?,
        })
    }
}
/// Every key must compare with itself and with the column's first non-null
/// value before sorting starts, so an unsortable key fails even for one row.
fn sort(
    items: Vec<Item>,
    keys: &[(Expr, bool)],
    engine: &mut Engine<'_>,
) -> Result<Vec<Item>, String> {
    let mut keyed = Vec::new();
    for mut item in items {
        let values = keys
            .iter()
            .map(|(e, _)| eval(e, &mut item, engine))
            .collect::<Result<Vec<_>, _>>()?;
        for value in &values {
            compare(value, value)?;
        }
        keyed.push((values, item));
    }
    for column in 0..keys.len() {
        let mut values = keyed.iter().map(|(values, _)| &values[column]);
        if let Some(first) = values.clone().find(|v| **v != Value::Null) {
            values.try_for_each(|v| compare(first, v).map(drop))?;
        }
    }
    let order = |a: &[Value], b: &[Value]| -> Result<Ordering, String> {
        for ((a, b), (_, desc)) in a.iter().zip(b).zip(keys) {
            let ordering = compare(a, b)?;
            let ordering = if *desc && *a != Value::Null && *b != Value::Null {
                ordering.reverse()
            } else {
                ordering
            };
            if ordering != Ordering::Equal {
                return Ok(ordering);
            }
        }
        Ok(Ordering::Equal)
    };
    let mut failure = None;
    keyed.sort_by(|(a, _), (b, _)| {
        order(a, b).unwrap_or_else(|e| {
            failure = Some(e);
            Ordering::Equal
        })
    });
    match failure {
        Some(e) => Err(e),
        None => Ok(keyed.into_iter().map(|(_, item)| item).collect()),
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
        let mut requests = std::collections::BTreeSet::new();
        if let Some(expr) = &self.expression {
            requests.extend(
                expr.note_imports()
                    .into_iter()
                    .map(|id| (context.clone(), id)),
            );
        }
        // Pipeline rows evaluate in their source note; aggregate rows use the query scope.
        let mut scopes = vec![context];
        if only.is_none() && !self.sources.is_empty() {
            scopes.extend(workspace.documents().keys().cloned());
        }
        let expressions = self.stages.iter().flat_map(Stage::expressions);
        for id in expressions.flat_map(ExprImports::note_imports) {
            requests.extend(scopes.iter().map(|path| (path.clone(), id.clone())));
        }
        for (path, id) in requests {
            if let Ok(target) = lang::model::note_path(&path, &id) {
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
        let parts = split(source, '|')?;
        if parts.len() > 129 {
            return Err("Queries are limited to 128 stages".into());
        }
        let mut input = None;
        let names = if let Some(body) = parts[0]
            .strip_prefix("union(")
            .and_then(|s| s.strip_suffix(')'))
        {
            split(body, ',')?
        } else if parts[0].parse::<Collection>().is_ok() {
            vec![parts[0]]
        } else {
            input = Some(expression(parts[0])?);
            vec![]
        };
        let sources = names
            .into_iter()
            .map(|name| {
                name.parse::<Collection>().map_err(|_| {
                    format!(
                        "Unknown collection '{name}'; expected {}",
                        Collection::names()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let stages = parts[1..]
            .iter()
            .enumerate()
            .map(|(i, part)| Stage::parse(part).map_err(|e| format!("Stage {}: {e}", i + 1)))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            sources,
            stages,
            expression: input,
        })
    }
}
fn boolean(v: Value) -> Result<bool, String> {
    if let Value::Bool(v) = v {
        Ok(v)
    } else {
        Err("Expected a boolean predicate".into())
    }
}
fn compare(a: &Value, b: &Value) -> Result<Ordering, String> {
    functional::compare(a, b).map_err(|e| e.to_string())
}
fn sum(values: impl IntoIterator<Item = Value>) -> Result<Value, String> {
    functional::sum(values)
        .map(q::query_value)
        .map_err(|e| e.to_string())
}

struct RowBindings(Mutex<Item>);
impl Bindings for RowBindings {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<lang::eval::EvalResult<Value>> {
        let mut item = self.0.lock().expect("query row poisoned");
        let exists = match &*item {
            Item::Record(record) => record.has(name),
            Item::Value(value, _) => value.property(name).is_ok(),
        };
        exists.then(|| item.field(name, engine).map_err(Into::into))
    }
}
/// The row moves into its bindings for the evaluation, so fields it fills in
/// lazily stay filled for the next stage, and moves back out after.
fn eval(expr: &Expr, item: &mut Item, engine: &mut Engine<'_>) -> Result<Value, String> {
    let path = item.path().to_path_buf();
    let row = std::mem::replace(item, Item::Value(Value::Null, PathBuf::new()));
    let bindings = Arc::new(RowBindings(Mutex::new(row)));
    let result = engine.bound_expr(&path, expr, bindings.clone());
    // A value that captured the bindings still shares them; copy the row out then.
    *item = match Arc::try_unwrap(bindings) {
        Ok(bindings) => bindings.0.into_inner().expect("query row poisoned"),
        Err(shared) => shared.0.lock().expect("query row poisoned").clone(),
    };
    result.map(q::query_value).map_err(|e| e.to_string())
}

/// Collections are loaded on demand, so unrelated features and errors are not evaluated.
struct WorkspaceBindings {
    only: Option<PathBuf>,
    diagnostics: DiagnosticSource,
    cache: Mutex<BTreeMap<String, Value>>,
}
impl Bindings for WorkspaceBindings {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<lang::eval::EvalResult<Value>> {
        let collection = name.parse::<Collection>();
        if name != "graph" && collection.is_err() {
            return None;
        }
        if let Some(value) = self.cache.lock().expect("query cache poisoned").get(name) {
            return Some(Ok(value.clone()));
        }
        let result = if name == "graph" {
            Ok(inspection::graph(engine.workspace(), self.only.as_deref()))
        } else {
            crate::collect(
                engine.workspace(),
                collection.expect("checked above"),
                engine,
                self.only.as_deref(),
                self.diagnostics,
            )
            .map(|records| {
                Value::List(records.into_iter().map(|r| r.materialize(engine)).collect())
            })
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
/// workspace-wide name resolution.
pub fn execute(
    request: &lang::eval::RequestContext<'_>,
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
    let mut items = Vec::new();
    if let Some(expr) = &query.expression {
        let bindings = Arc::new(WorkspaceBindings {
            only: only.map(Path::to_path_buf),
            diagnostics,
            cache: Mutex::new(BTreeMap::new()),
        });
        let value = engine.bound_expr(&context, expr, bindings)?;
        let rows = match q::query_value(value) {
            Value::List(rows) => rows,
            value => vec![value],
        };
        items.extend(
            rows.into_iter()
                .map(|row| Item::Value(row, context.clone())),
        );
    }
    for source in &query.sources {
        items.extend(
            crate::collect(ws, *source, &mut engine, only, diagnostics)?
                .into_iter()
                .map(Item::Record),
        );
    }
    for (index, stage) in query.stages.iter().enumerate() {
        items = stage
            .apply(items, &mut engine, &context)
            .map_err(|e| format!("Stage {}: {e}", index + 1))?;
    }
    Ok(QueryResult {
        rows: items
            .into_iter()
            .map(|item| item.materialize(&mut engine))
            .collect(),
    })
}
