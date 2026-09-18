//! Functional queries over lazy workspace bindings. Legacy pipelines share the evaluator.
pub use crate::catalog::{QueryContext, QueryValue};
use crate::{
    catalog::{self, Collection, QueryValue as Q, Record},
    engine::{self, Bindings, Engine, Expr, Lexeme, Parser, Value},
    evaluate::functional,
};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

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
    Value(Q, PathBuf),
}
impl Item {
    fn path(&self) -> &std::path::Path {
        match self {
            Self::Record(r) => &r.path,
            Self::Value(_, p) => p,
        }
    }
    fn field(&mut self, key: &str, engine: &mut Engine<'_>) -> Result<Q, String> {
        match self {
            Self::Record(r) => r.field(key, engine),
            Self::Value(v, _) => v.property(key),
        }
    }
    fn materialize(self, engine: &mut Engine<'_>) -> Q {
        match self {
            Self::Record(r) => r.materialize(engine),
            Self::Value(v, _) => v,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct QueryResult {
    pub rows: Vec<Q>,
}

/// Only the compatibility pipeline syntax lives here; literals, comments and
/// delimiters come from the language lexer and every expression uses its parser.
fn split(source: &str, separator: char) -> Result<Vec<&str>, String> {
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
            Lexeme::Op(op)
                if separator == '|' && op == crate::engine::Operator::Pipe && stack.is_empty() =>
            {
                parts.push(source[start..token.start].trim());
                start = token.end;
            }
            Lexeme::Comma if separator == ',' && stack.is_empty() => {
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
impl Query {
    #[cfg(feature = "native")]
    pub(crate) fn load_imports(
        &self,
        workspace: &mut crate::workspace::Workspace,
        only: Option<&Path>,
    ) {
        let context = only
            .map(Path::to_path_buf)
            .unwrap_or_else(|| workspace.root().join("__query__.wtf"));
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
            scopes.extend(workspace.documents.keys().cloned());
        }
        let mut expressions = Vec::new();
        for stage in &self.stages {
            match stage {
                Stage::Where(e) | Stage::Select(e) | Stage::Sum(e) | Stage::Group(e) => {
                    expressions.push(e)
                }
                Stage::Sort(keys) => expressions.extend(keys.iter().map(|(e, _)| e)),
                _ => (),
            }
        }
        for id in expressions.into_iter().flat_map(Expr::note_imports) {
            requests.extend(scopes.iter().map(|path| (path.clone(), id.clone())));
        }
        for (path, id) in requests {
            if let Ok(target) = crate::model::imports::note_path(&path, &id) {
                // Like note imports, report missing dependencies only if evaluation reads them.
                let _ = workspace.include_file(&target);
            }
        }
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
fn boolean(v: Q) -> Result<bool, String> {
    if let Q::Scalar(Value::Bool(v)) = v {
        Ok(v)
    } else {
        Err("Expected a boolean predicate".into())
    }
}
fn compare(a: &Q, b: &Q) -> Result<Ordering, String> {
    functional::compare(&a.value(), &b.value())
}
fn sum(values: impl IntoIterator<Item = Q>) -> Result<Q, String> {
    functional::sum(values.into_iter().map(|v| v.value())).map(Q::from_value)
}

struct RowBindings(Mutex<Item>);
impl Bindings for RowBindings {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<Result<Value, String>> {
        let mut item = self.0.lock().expect("query row poisoned");
        let exists = match &*item {
            Item::Record(record) => {
                record.fields.contains_key(name)
                    || name == "hover" && record.fields.contains_key("name")
            }
            Item::Value(value, _) => value.property(name).is_ok(),
        };
        exists.then(|| item.field(name, engine).map(|v| v.value()))
    }
}
fn eval(expr: &Expr, item: &mut Item, engine: &mut Engine<'_>) -> Result<Q, String> {
    let bindings = Arc::new(RowBindings(Mutex::new(item.clone())));
    let result = engine.bound_expr(item.path(), expr, bindings.clone());
    *item = bindings.0.lock().expect("query row poisoned").clone();
    result.map(Q::from_value)
}

/// Collections are loaded on demand, so unrelated features and errors are not evaluated.
struct WorkspaceBindings {
    only: Option<PathBuf>,
    cache: Mutex<BTreeMap<String, Value>>,
}
impl Bindings for WorkspaceBindings {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<Result<Value, String>> {
        let collection = name.parse::<Collection>();
        if name != "graph" && collection.is_err() {
            return None;
        }
        if let Some(value) = self.cache.lock().expect("query cache poisoned").get(name) {
            return Some(Ok(value.clone()));
        }
        let result = if name == "graph" {
            Ok(crate::features::inspection::graph(engine.workspace, self.only.as_deref()).value())
        } else {
            catalog::collect_document(
                engine.workspace,
                collection.expect("checked above"),
                QueryContext::new(engine.now),
                engine,
                self.only.as_deref(),
                true,
            )
            .map(|records| {
                Value::List(
                    records
                        .into_iter()
                        .map(|r| r.materialize(engine).value())
                        .collect(),
                )
            })
        };
        if let Ok(value) = &result {
            self.cache
                .lock()
                .expect("query cache poisoned")
                .insert(name.into(), value.clone());
        }
        Some(result)
    }
}

/// `only` restricts input records to one indexed document while retaining
/// workspace-wide name resolution.
pub(crate) fn execute(
    request: &crate::RequestContext<'_>,
    query: &Query,
    only: Option<&Path>,
) -> Result<QueryResult, String> {
    let ws = request.workspace();
    let ctx = &request.clock();
    let mut engine = request.engine();
    if let Some(path) = only
        && !ws.documents.contains_key(path)
    {
        return Err(format!("Document is not indexed: {}", path.display()));
    }
    let context = only
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ws.root().join("__query__.wtf"));
    let mut items = Vec::new();
    if let Some(expr) = &query.expression {
        let bindings = Arc::new(WorkspaceBindings {
            only: only.map(Path::to_path_buf),
            cache: Mutex::new(BTreeMap::new()),
        });
        let path = &context;
        let value = engine.bound_expr(path, expr, bindings)?;
        let rows = match Q::from_value(value) {
            Q::Array(rows) => rows,
            value => vec![value],
        };
        items.extend(rows.into_iter().map(|row| Item::Value(row, path.into())));
    }
    for source in &query.sources {
        items.extend(
            catalog::collect_document(ws, *source, *ctx, &mut engine, only, true)?
                .into_iter()
                .map(Item::Record),
        );
    }
    for (index, stage) in query.stages.iter().enumerate() {
        items = (|| {
            Ok(match stage {
                Stage::Where(expr) => {
                    let mut selected = Vec::new();
                    for mut item in items {
                        if boolean(eval(expr, &mut item, &mut engine)?)? {
                            selected.push(item);
                        }
                    }
                    selected
                }
                Stage::Select(expr) => {
                    let mut selected = Vec::new();
                    for mut item in items {
                        let path = item.path().to_path_buf();
                        let value = eval(expr, &mut item, &mut engine)?;
                        selected.push(Item::Value(value, path));
                    }
                    selected
                }
                Stage::Limit(n) => items.into_iter().take(*n).collect(),
                Stage::Count => vec![Item::Value(Q::count(items.len()), context.clone())],
                Stage::Sum(expr) => {
                    let values = items
                        .iter_mut()
                        .map(|item| eval(expr, item, &mut engine))
                        .collect::<Result<Vec<_>, _>>()?;
                    vec![Item::Value(sum(values)?, context.clone())]
                }
                Stage::Group(expr) => {
                    let mut groups: BTreeMap<String, (Q, Vec<Q>)> = BTreeMap::new();
                    for mut item in items {
                        let key = eval(expr, &mut item, &mut engine)?;
                        let identity = match &key {
                            Q::Scalar(Value::Number(n)) => n.to_string(),
                            Q::Scalar(Value::Count(n)) => n.to_string(),
                            _ => key.json().to_string(),
                        };
                        groups
                            .entry(identity)
                            .or_insert_with(|| (key, Vec::new()))
                            .1
                            .push(item.materialize(&mut engine));
                    }
                    groups
                        .into_values()
                        .map(|(key, rows)| {
                            Item::Value(
                                Q::object([("key", key), ("rows", Q::Array(rows))]),
                                context.clone(),
                            )
                        })
                        .collect()
                }
                Stage::Sort(keys) => {
                    let mut keyed = Vec::new();
                    for mut item in items {
                        let values = keys
                            .iter()
                            .map(|(e, _)| eval(e, &mut item, &mut engine))
                            .collect::<Result<Vec<_>, _>>()?;
                        // Reject unsortable keys even for a single result.
                        for value in &values {
                            compare(value, value)?;
                        }
                        keyed.push((values, item));
                    }
                    // Establish a common comparable type before invoking the sort comparator.
                    for column in 0..keys.len() {
                        if let Some(first) = keyed
                            .iter()
                            .map(|(values, _)| &values[column])
                            .find(|v| **v != Q::Null)
                        {
                            for (values, _) in &keyed {
                                compare(first, &values[column])?;
                            }
                        }
                    }
                    let mut failure = None;
                    keyed.sort_by(|(a, _), (b, _)| {
                        for ((a, b), (_, desc)) in a.iter().zip(b).zip(keys) {
                            let ordering = match compare(a, b) {
                                Ok(v) => v,
                                Err(e) => {
                                    failure = Some(e);
                                    return Ordering::Equal;
                                }
                            };
                            let ordering = if *desc && *a != Q::Null && *b != Q::Null {
                                ordering.reverse()
                            } else {
                                ordering
                            };
                            if ordering != Ordering::Equal {
                                return ordering;
                            }
                        }
                        Ordering::Equal
                    });
                    if let Some(e) = failure {
                        return Err(e);
                    }
                    keyed.into_iter().map(|(_, item)| item).collect()
                }
            })
        })()
        .map_err(|e: String| format!("Stage {}: {e}", index + 1))?;
    }
    Ok(QueryResult {
        rows: items
            .into_iter()
            .map(|item| item.materialize(&mut engine))
            .collect(),
    })
}
