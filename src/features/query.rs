//! Compiled collection pipelines over the shared workspace and evaluator.
pub use crate::catalog::{QueryContext, QueryValue};
use crate::{
    catalog::{self, QueryValue as Q, Record},
    engine::{self, Engine, Expr, Parser, Value},
    workspace::Workspace,
};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub const TODAY: &str = include_str!("../../queries/today.wq");
pub const WEEK: &str = include_str!("../../queries/week.wq");
pub const TASKS: &str = include_str!("../../queries/tasks.wq");
pub const CHECK: &str = include_str!("../../queries/check.wq");

#[derive(Clone, Debug)]
pub struct Query {
    sources: Vec<String>,
    stages: Vec<Stage>,
}
#[derive(Clone, Debug)]
enum Stage {
    Where(Expr),
    Select(Projection),
    Sort(Vec<(Expr, bool)>),
    Limit(usize),
    Count,
    Sum(Expr),
    Group(Expr),
}
#[derive(Clone, Debug)]
enum Projection {
    Object(Vec<(String, Expr)>),
    Value(Expr),
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

/// Split structural separators without breaking quoted strings, nested calls or ||.
fn split(source: &str, separator: char) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut stack = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match c {
            '(' | '{' | '[' => {
                stack.push(c);
                if stack.len() > 64 {
                    return Err("Query nesting exceeds 64 levels".into());
                }
            }
            ')' | '}' | ']' => {
                let expected = match c {
                    ')' => '(',
                    '}' => '{',
                    _ => '[',
                };
                if stack.pop() != Some(expected) {
                    return Err(format!("Unmatched '{c}' at byte {i}"));
                }
            }
            _ => {}
        }
        if c == separator && stack.is_empty() {
            if c == '|'
                && (source.as_bytes().get(i.wrapping_sub(1)) == Some(&b'|')
                    || source.as_bytes().get(i + 1) == Some(&b'|'))
            {
                continue;
            }
            parts.push(source[start..i].trim());
            start = i + c.len_utf8();
        }
    }
    if quoted || !stack.is_empty() {
        return Err("Unclosed string or delimiter in query".into());
    }
    parts.push(source[start..].trim());
    if parts.iter().any(|s| s.is_empty()) {
        return Err(format!("Empty expression around '{separator}'"));
    }
    Ok(parts)
}
fn expression(source: &str) -> Result<Expr, String> {
    if engine::lex(source)?.len() > 512 {
        return Err("An expression may contain at most 512 tokens".into());
    }
    let parsed = Parser::parse(source)?;
    // Parenthesis depth alone does not bound a long left-associative expression.
    let mut pending = vec![(&parsed, 0)];
    while let Some((expr, depth)) = pending.pop() {
        if depth > 64 {
            return Err("Expression depth exceeds 64 levels".into());
        }
        match expr {
            Expr::Spanned(_, _, inner) => pending.push((inner, depth)),
            Expr::Unary(_, inner) | Expr::Property(inner, _) => pending.push((inner, depth + 1)),
            Expr::Binary(_, left, right) => {
                pending.push((left, depth + 1));
                pending.push((right, depth + 1));
            }
            Expr::Call(_, args) | Expr::List(args) => {
                pending.extend(args.iter().map(|arg| (arg, depth + 1)))
            }
            Expr::Record(fields) => pending.extend(fields.iter().map(|(_, e)| (e, depth + 1))),
            Expr::Lambda(_, body) => pending.push((body, depth + 1)),
            Expr::Apply(f, args) => {
                pending.push((f, depth + 1));
                pending.extend(args.iter().map(|arg| (arg, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(parsed)
}
fn projection(source: &str) -> Result<Projection, String> {
    let Some(body) = source.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return expression(source).map(Projection::Value);
    };
    if body.trim().is_empty() {
        return Ok(Projection::Object(vec![]));
    }
    let mut names = BTreeSet::new();
    let mut fields = Vec::new();
    for field in split(body, ',')? {
        let (name, source) = field
            .split_once(':')
            .map(|(n, s)| (n.trim(), s.trim()))
            .unwrap_or((field, field));
        if !crate::document::identifier(name) {
            return Err(format!("Invalid projection name '{name}'; use an alias"));
        }
        if !names.insert(name) {
            return Err(format!("Duplicate projection field '{name}'"));
        }
        fields.push((name.into(), expression(source)?));
    }
    Ok(Projection::Object(fields))
}
impl Query {
    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > 65_536 {
            return Err("Queries are limited to 64 KiB".into());
        }
        let parts = split(source, '|')?;
        if let Some(name) = parts[0].strip_prefix('@') {
            let view = match name {
                "today" => TODAY,
                "week" => WEEK,
                "tasks" => TASKS,
                "check" => CHECK,
                _ => {
                    return Err(format!(
                        "Unknown saved view '@{name}'; use @today, @week, @tasks or @check"
                    ));
                }
            };
            let mut expanded = view.trim().to_owned();
            for part in &parts[1..] {
                expanded.push_str(" | ");
                expanded.push_str(part);
            }
            return Self::parse(&expanded);
        }
        if parts.len() > 129 {
            return Err("Queries are limited to 128 stages".into());
        }
        let sources = if let Some(body) = parts[0]
            .strip_prefix("union(")
            .and_then(|s| s.strip_suffix(')'))
        {
            split(body, ',')?
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        } else {
            vec![parts[0].into()]
        };
        for name in &sources {
            if !catalog::COLLECTIONS.contains(&name.as_str()) {
                return Err(format!(
                    "Unknown collection '{name}'; expected {}",
                    catalog::COLLECTIONS.join(", ")
                ));
            }
        }
        let mut stages = Vec::new();
        for (i, part) in parts[1..].iter().enumerate() {
            let parsed=(||{
                let (op,argument)=part.split_once(char::is_whitespace).map(|(a,b)|(a,b.trim())).unwrap_or((part,""));
                Ok(match op {
                    "where"=>Stage::Where(expression(argument)?),
                    "select"=>Stage::Select(projection(argument)?),
                    "sort"=>Stage::Sort(split(argument,',')?.into_iter().map(|s|{
                        let (expr,desc)=if let Some(s)=s.strip_suffix(" desc") {(s.trim(),true)} else {(s.strip_suffix(" asc").unwrap_or(s).trim(),false)};
                        Ok((expression(expr)?,desc))
                    }).collect::<Result<_,String>>()?),
                    "limit"=>Stage::Limit(argument.parse().map_err(|_|"limit requires a nonnegative integer")?),
                    "count" if argument.is_empty()=>Stage::Count,
                    "sum"=>Stage::Sum(expression(argument)?),
                    "group"=>Stage::Group(expression(argument)?),
                    _=>return Err(format!("Unknown stage '{part}'; use where, select, sort, limit, count, sum or group")),
                })
            })().map_err(|e:String|format!("Stage {}: {e}",i+1))?;
            stages.push(parsed);
        }
        Ok(Self { sources, stages })
    }
}
fn boolean(v: Q) -> Result<bool, String> {
    if let Q::Scalar(Value::Bool(v)) = v {
        Ok(v)
    } else {
        Err("Expected a boolean predicate".into())
    }
}
fn equal(a: &Q, b: &Q) -> Result<bool, String> {
    match (a, b) {
        (Q::Scalar(a), Q::Scalar(b)) => {
            boolean(Q::Scalar(engine::binary("==", a.clone(), b.clone())?))
        }
        _ => Ok(a == b),
    }
}
fn compare(a: &Q, b: &Q) -> Result<Ordering, String> {
    match (a, b) {
        (Q::Null, Q::Null) => Ok(Ordering::Equal),
        (Q::Null, _) => Ok(Ordering::Greater),
        (_, Q::Null) => Ok(Ordering::Less),
        (Q::Scalar(Value::Text(a)), Q::Scalar(Value::Text(b))) => Ok(a.cmp(b)),
        (Q::Scalar(Value::Bool(a)), Q::Scalar(Value::Bool(b))) => Ok(a.cmp(b)),
        (Q::Scalar(a), Q::Scalar(b)) => {
            // Validate compatible types even when equality happens to be false.
            let less = boolean(Q::Scalar(engine::binary("<", a.clone(), b.clone())?))?;
            if less {
                Ok(Ordering::Less)
            } else if equal(&Q::Scalar(a.clone()), &Q::Scalar(b.clone()))? {
                Ok(Ordering::Equal)
            } else {
                Ok(Ordering::Greater)
            }
        }
        _ => Err("Sorting and ordering require compatible scalar values".into()),
    }
}
fn sum(values: impl IntoIterator<Item = Q>) -> Result<Q, String> {
    let mut total = None;
    for v in values {
        if v == Q::Null {
            continue;
        }
        let Q::Scalar(v) = v else {
            return Err("sum requires numbers, money or durations".into());
        };
        if !matches!(
            v,
            Value::Number(_)
                | Value::Count(_)
                | Value::Ratio(_)
                | Value::Money(..)
                | Value::Duration(_)
        ) {
            return Err("sum requires numbers, money or durations".into());
        }
        total = Some(match total {
            Some(a) => engine::binary("+", a, v)?,
            None => v,
        });
    }
    Ok(total.map(Q::Scalar).unwrap_or(Q::Null))
}
fn eval(
    expr: &Expr,
    item: &mut Item,
    engine: &mut Engine<'_>,
    ctx: QueryContext,
) -> Result<Q, String> {
    match expr {
        Expr::Spanned(_, _, e) => eval(e, item, engine, ctx),
        Expr::List(items) => items
            .iter()
            .map(|e| eval(e, item, engine, ctx))
            .collect::<Result<Vec<_>, _>>()
            .map(Q::Array),
        Expr::Record(fields) => fields
            .iter()
            .map(|(k, e)| Ok((k.clone(), eval(e, item, engine, ctx)?)))
            .collect::<Result<_, String>>()
            .map(Q::Object),
        Expr::Lambda(params, body) => {
            let captured = match item.clone().materialize(engine).value() {
                Value::Record(fields) => fields,
                _ => Default::default(),
            };
            Ok(Q::Scalar(Value::Function(std::sync::Arc::new(
                crate::evaluate::functional::Function {
                    params: params.clone(),
                    body: *body.clone(),
                    path: item.path().into(),
                    captured,
                },
            ))))
        }
        Expr::Apply(f, args) => {
            let function = eval(f, item, engine, ctx)?.value();
            let args = args
                .iter()
                .map(|e| eval(e, item, engine, ctx).map(|v| v.value()))
                .collect::<Result<Vec<_>, _>>()?;
            engine.call(function, args).map(Q::from_value)
        }
        Expr::Value(v) => Ok(Q::from_value(v.clone())),
        Expr::Name(n) => match n.as_str() {
            "null" => Ok(Q::Null),
            "true" => Ok(Q::boolean(true)),
            "false" => Ok(Q::boolean(false)),
            _ => item.field(n, engine),
        },
        Expr::Property(e, key) => eval(e, item, engine, ctx)?.property(key),
        Expr::Unary(op, e) => {
            let v = eval(e, item, engine, ctx)?;
            match (op.as_str(), v) {
                ("!", v) => Ok(Q::boolean(!boolean(v)?)),
                ("-", Q::Scalar(Value::Duration(n))) => n
                    .checked_neg()
                    .map(|n| Q::Scalar(Value::Duration(n)))
                    .ok_or("Duration overflow".into()),
                ("-", Q::Scalar(Value::Number(n))) => Ok(Q::Scalar(Value::Number(-n))),
                ("-", Q::Scalar(Value::Money(n, c))) => Ok(Q::Scalar(Value::Money(-n, c))),
                ("-", Q::Scalar(Value::Ratio(n))) => Ok(Q::Scalar(Value::Ratio(-n))),
                ("+", v @ Q::Scalar(Value::Number(_))) => Ok(v),
                _ => Err("Invalid unary operation".into()),
            }
        }
        Expr::Binary(op, a, b) => {
            let a = eval(a, item, engine, ctx)?;
            if op == "&&" && !boolean(a.clone())? {
                return Ok(Q::boolean(false));
            }
            if op == "||" && boolean(a.clone())? {
                return Ok(Q::boolean(true));
            }
            let b = eval(b, item, engine, ctx)?;
            match op.as_str() {
                "==" | "!=" => Ok(Q::boolean(equal(&a, &b)? == (op == "=="))),
                "<" | "<=" | ">" | ">=" => {
                    if a == Q::Null || b == Q::Null {
                        return Ok(Q::boolean(false));
                    }
                    let order = compare(&a, &b)?;
                    Ok(Q::boolean(match op.as_str() {
                        "<" => order.is_lt(),
                        "<=" => order.is_le(),
                        ">" => order.is_gt(),
                        _ => order.is_ge(),
                    }))
                }
                _ => {
                    if let (Q::Scalar(a), Q::Scalar(b)) = (a, b) {
                        engine::binary(op, a, b).map(Q::Scalar)
                    } else {
                        Err(format!("'{op}' requires scalar operands"))
                    }
                }
            }
        }
        Expr::Call(name, args) => {
            if name == "if" {
                if args.len() != 3 {
                    return Err("if expects a condition and two branches".into());
                }
                let condition = boolean(eval(&args[0], item, engine, ctx)?)?;
                return eval(&args[if condition { 1 } else { 2 }], item, engine, ctx);
            }
            if name == "coalesce" {
                for arg in args {
                    let v = eval(arg, item, engine, ctx)?;
                    if v != Q::Null {
                        return Ok(v);
                    }
                }
                return Ok(Q::Null);
            }
            let values = args
                .iter()
                .map(|e| eval(e, item, engine, ctx))
                .collect::<Result<Vec<_>, _>>()?;
            match (name.as_str(), values.as_slice()) {
                ("today", []) => Ok(Q::Scalar(Value::Date(ctx.today()))),
                ("now", []) => Ok(Q::Scalar(Value::DateTime(ctx.now))),
                ("date", [Q::Scalar(Value::DateTime(d))]) => Ok(Q::Scalar(Value::Date(
                    d.with_timezone(ctx.now.offset()).date_naive(),
                ))),
                ("date", [Q::Scalar(Value::Date(d))]) => Ok(Q::Scalar(Value::Date(*d))),
                ("date", [Q::Scalar(Value::Text(s))]) => engine::date_value(s)
                    .or_else(|| engine::relative_date(s, ctx.today()).map(Value::Date))
                    .map(Q::from_value)
                    .ok_or("Unrecognized date".into()),
                ("eval", [Q::Scalar(Value::Text(s))]) => {
                    engine.eval(item.path(), s).map(Q::from_value)
                }
                ("length", [Q::Array(a)]) => Ok(Q::count(a.len())),
                ("length", [Q::Object(a)]) => Ok(Q::count(a.len())),
                ("length", [Q::Scalar(Value::Text(s))]) => Ok(Q::count(s.chars().count())),
                ("text", [Q::Null]) => Ok(Q::Null),
                ("text", [value]) => Ok(Q::text(value.display())),
                ("contains", [Q::Array(a), b]) => {
                    Ok(Q::boolean(a.iter().any(|v| equal(v, b).unwrap_or(false))))
                }
                ("contains", [Q::Scalar(Value::Text(a)), Q::Scalar(Value::Text(b))]) => {
                    Ok(Q::boolean(a.contains(b)))
                }
                ("get", [Q::Object(a), Q::Scalar(Value::Text(key))]) => {
                    Ok(a.get(key).cloned().unwrap_or(Q::Null))
                }
                ("sum", [Q::Array(a)]) => sum(a.clone()),
                _ if crate::evaluate::functional::is_builtin(name) => engine
                    .functional(name, values.iter().map(Q::value).collect())
                    .map(Q::from_value),
                _ if engine.workspace.resolve(item.path(), name).is_ok() => {
                    let function = engine.named(item.path(), name)?;
                    engine
                        .call(function, values.iter().map(Q::value).collect())
                        .map(Q::from_value)
                }
                _ => Err(format!("Unknown function or invalid arguments: {name}")),
            }
        }
    }
}

pub fn execute(ws: &Workspace, query: &Query, ctx: &QueryContext) -> Result<QueryResult, String> {
    execute_in(&crate::RequestContext::new(ws, ctx.now), query)
}
pub fn execute_in(
    request: &crate::RequestContext<'_>,
    query: &Query,
) -> Result<QueryResult, String> {
    let ws = request.workspace();
    let ctx = &request.clock();
    let mut engine = request.engine();
    let mut items = Vec::new();
    for source in &query.sources {
        items.extend(
            catalog::collect(ws, source, *ctx, &mut engine)?
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
                        if boolean(eval(expr, &mut item, &mut engine, *ctx)?)? {
                            selected.push(item);
                        }
                    }
                    selected
                }
                Stage::Select(projection) => {
                    let mut selected = Vec::new();
                    for mut item in items {
                        let path = item.path().to_path_buf();
                        selected.push(match projection {
                            Projection::Object(fields) => {
                                let values = fields
                                    .iter()
                                    .map(|(k, e)| {
                                        Ok((k.clone(), eval(e, &mut item, &mut engine, *ctx)?))
                                    })
                                    .collect::<Result<BTreeMap<_, _>, String>>()?;
                                Item::Record(Record::projected(path, values))
                            }
                            Projection::Value(e) => {
                                Item::Value(eval(e, &mut item, &mut engine, *ctx)?, path)
                            }
                        });
                    }
                    selected
                }
                Stage::Limit(n) => items.into_iter().take(*n).collect(),
                Stage::Count => vec![Item::Value(Q::count(items.len()), ws.root().into())],
                Stage::Sum(expr) => {
                    let values = items
                        .iter_mut()
                        .map(|item| eval(expr, item, &mut engine, *ctx))
                        .collect::<Result<Vec<_>, _>>()?;
                    vec![Item::Value(sum(values)?, ws.root().into())]
                }
                Stage::Group(expr) => {
                    let mut groups: BTreeMap<String, (Q, Vec<Q>)> = BTreeMap::new();
                    for mut item in items {
                        let key = eval(expr, &mut item, &mut engine, *ctx)?;
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
                                ws.root().into(),
                            )
                        })
                        .collect()
                }
                Stage::Sort(keys) => {
                    let mut keyed = Vec::new();
                    for mut item in items {
                        let values = keys
                            .iter()
                            .map(|(e, _)| eval(e, &mut item, &mut engine, *ctx))
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
