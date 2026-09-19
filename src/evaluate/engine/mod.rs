//! Evaluation: the engine that turns a definition into a value, split into the
//! values it produces, the syntax it reads, and the linear forms plans need.
use crate::{
    document::Span,
    timers::Timer,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
mod arithmetic;
mod linear;
mod syntax;
mod value;
pub(crate) use arithmetic::binary;
pub use arithmetic::{BinaryOp, Comparison, Operator, UnaryOp};
pub use linear::{Linear, RowVariable, Unit};
pub(crate) use syntax::{Expr, Parser, expression_names, is_builtin_function, lex_with_comments};
pub use syntax::{Lexeme, Token, lex, simple_name, sum_scope_at, timer_arguments};
pub use value::{
    Currency, Forecast, TaskKey, Value, ValueType, date_value, decimal, duration, is_code,
    is_relative_date, literal, next_occurrence, relative_date,
};
#[derive(Clone, Debug)]
pub struct EvalFailure {
    pub path: PathBuf,
    pub span: Span,
    pub message: String,
    pub related: Vec<Symbol>,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MemoKey {
    Symbol(Symbol),
    ResourceProperty(String, String),
}
#[derive(Clone)]
pub(crate) struct MemoEntry {
    value: Result<Value, String>,
    failure: Option<EvalFailure>,
    wanted: Vec<crate::lookups::LookupKey>,
    time_dependent: bool,
}
/// Host-provided names are resolved lazily by the same evaluator as note functions.
/// Resolution runs without the caller's bindings, so definitions cannot capture them.
pub(crate) trait Bindings: Send + Sync {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<Result<Value, String>>;
}

pub struct Engine<'a> {
    pub workspace: &'a Workspace,
    pub today: NaiveDate,
    pub now: DateTime<FixedOffset>,
    pub time_dependent: bool,
    link_features: crate::link_features::LinkFeatures<'a>,
    pub failure: Option<EvalFailure>,
    contexts: Vec<(PathBuf, Span)>,
    memo: std::sync::Arc<std::sync::Mutex<BTreeMap<MemoKey, MemoEntry>>>,
    stack: Vec<Symbol>,
    row_values: Vec<RowScope>,
    /// Decision-column variables met while linearizing a plan.
    pub row_variables: Vec<RowVariable>,
    /// Lookup keys read during evaluation, hit or miss, for hovers and refresh.
    pub wanted: Vec<crate::lookups::LookupKey>,
    /// Definitions being walked symbolically, separate from the value stack:
    /// a goal seek is legitimately on both at once.
    linear_stack: Vec<Symbol>,
    steps: usize,
    locals: Vec<BTreeMap<String, Value>>,
    bindings: Option<std::sync::Arc<dyn Bindings>>,
    calls: usize,
    pure: bool,
    environment: Option<std::sync::Arc<Workspace>>,
    expressions: Option<std::sync::Arc<BTreeMap<String, Expr>>>,
}
/// Column values for one table row while a `sum` row expression runs.
pub(super) struct RowScope {
    pub(super) table: String,
    pub(super) values: BTreeMap<String, Value>,
    /// Decision columns, mapped to the per-row variable name a plan uses.
    pub(super) decisions: BTreeMap<String, String>,
}
impl<'a> Engine<'a> {
    /// One clock snapshot per evaluation; injectable for deterministic tests.
    pub fn at(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        crate::RequestContext::new(workspace, now).engine()
    }
    pub(crate) fn in_request(request: &crate::RequestContext<'a>) -> Self {
        Self {
            workspace: request.workspace(),
            today: request.today(),
            now: request.now(),
            time_dependent: false,
            link_features: request.link_features(),
            failure: None,
            contexts: Vec::new(),
            memo: request.memo.clone(),
            stack: Vec::new(),
            row_values: Vec::new(),
            row_variables: Vec::new(),
            wanted: Vec::new(),
            linear_stack: Vec::new(),
            steps: 0,
            locals: Vec::new(),
            bindings: None,
            calls: 0,
            pure: false,
            environment: None,
            expressions: None,
        }
    }
    /// Share immutable inputs and memoized results with another feature session.
    pub fn request(&self) -> crate::RequestContext<'a> {
        crate::RequestContext {
            workspace: self.workspace,
            clock: crate::context::Clock::new(self.now),
            today: self.today,
            links: self.link_features,
            memo: self.memo.clone(),
        }
    }
    pub fn date(&self, value: &Value) -> Result<NaiveDate, String> {
        self.request()
            .clock()
            .date(value)
            .ok_or_else(|| "Expected a date or appointment time".into())
    }
    pub fn link_features(&self) -> crate::link_features::LinkFeatures<'a> {
        self.link_features
    }
    /// Override the registry for an embedded host or test before evaluation begins.
    pub fn with_link_features(mut self, features: crate::link_features::LinkFeatures<'a>) -> Self {
        self.link_features = features;
        self.memo = Default::default();
        self
    }
    pub(crate) fn bound_expr(
        &mut self,
        path: &Path,
        expr: &Expr,
        bindings: std::sync::Arc<dyn Bindings>,
    ) -> Result<Value, String> {
        let previous = self.bindings.replace(bindings);
        let result = self.expr(path, expr);
        self.bindings = previous;
        result
    }
    fn binding(&mut self, name: &str) -> Option<Result<Value, String>> {
        let bindings = self.bindings.take()?;
        let result = bindings.get(name, self);
        self.bindings = Some(bindings);
        result
    }
    pub fn eval(&mut self, path: &Path, expression: &str) -> Result<Value, String> {
        self.eval_at(path, expression, Span::new(0, 0, expression.len()))
    }
    pub fn eval_at(&mut self, path: &Path, expression: &str, span: Span) -> Result<Value, String> {
        if self.contexts.is_empty()
            && self.stack.is_empty()
            && self.row_values.is_empty()
            && self.calls == 0
            && self.bindings.is_none()
        {
            self.steps = 0;
        }
        self.contexts.push((path.into(), span));
        let parsed = self
            .expressions
            .as_ref()
            .and_then(|m| m.get(expression))
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| Parser::parse(expression));
        let result = match parsed {
            Ok(expr) => self.expr(path, &expr),
            Err(message) => {
                let tokens = lex(expression).unwrap_or_default();
                let bounds = tokens
                    .last()
                    .map(|t| (t.start, t.end))
                    .unwrap_or((0, expression.len()));
                self.fail(bounds, &message);
                Err(message)
            }
        };
        self.contexts.pop();
        result
    }
    fn fail(&mut self, bounds: (usize, usize), message: &str) {
        if self.failure.is_none()
            && let Some((path, base)) = self.contexts.last()
        {
            self.failure = Some(EvalFailure {
                path: path.clone(),
                span: self
                    .workspace
                    .documents
                    .get(path)
                    .map(|doc| base.relative(&doc.text, bounds.0, bounds.1))
                    .unwrap_or_else(|| {
                        Span::new(base.line, base.start + bounds.0, base.start + bounds.1)
                    }),
                message: message.into(),
                related: vec![],
            });
        }
    }
    pub fn valid_expression(source: &str) -> bool {
        Parser::parse(source).is_ok()
    }
    pub fn is_subexpression(source: &str, start: usize, end: usize) -> bool {
        fn contains(expr: &Expr, start: usize, end: usize) -> bool {
            if expr.bounds() == (start, end) {
                return true;
            }
            match expr {
                Expr::Spanned(_, _, inner) => contains(inner, start, end),
                Expr::Call(_, args) | Expr::List(args) => {
                    args.iter().any(|e| contains(e, start, end))
                }
                Expr::Record(fields) => fields.iter().any(|(_, e)| contains(e, start, end)),
                Expr::Lambda(_, e) => contains(e, start, end),
                Expr::Apply(f, args) => {
                    contains(f, start, end) || args.iter().any(|e| contains(e, start, end))
                }
                Expr::Unary(_, e) | Expr::Property(e, _) => contains(e, start, end),
                Expr::Binary(_, a, b) => contains(a, start, end) || contains(b, start, end),
                _ => false,
            }
        }
        Parser::parse(source).is_ok_and(|e| contains(&e, start, end))
    }
    /// Return a substitution trace without re-evaluating side effects (evaluation is pure).
    pub fn substituted(&mut self, path: &Path, source: &str) -> Result<String, String> {
        let tokens = lex(source)?;
        let mut edits = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            if let Lexeme::Name(name) = &token.kind
                && !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left))
                && (i == 0 || !matches!(tokens[i - 1].kind, Lexeme::Dot))
                && !matches!(name.as_str(), "true" | "false")
                && sum_scope_at(source, token.start).is_none()
                && let Ok(value) = self.named(path, name)
            {
                if matches!(value, Value::Table(_)) {
                    continue;
                }
                // For properties substitute the complete access, not a timer's display text.
                let end = if matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Dot)) {
                    tokens.get(i + 2).map(|t| t.end).unwrap_or(token.end)
                } else {
                    token.end
                };
                let value = if end > token.end {
                    self.eval(path, &source[token.start..end])?
                } else {
                    value
                };
                edits.push((token.start, end, value.display()));
            }
        }
        let mut result = source.to_string();
        for (start, end, value) in edits.into_iter().rev() {
            result.replace_range(start..end, &value);
        }
        Ok(result)
    }
    pub fn named(&mut self, path: &Path, name: &str) -> Result<Value, String> {
        let symbol = self.workspace.resolve(path, name)?;
        self.symbol(&symbol)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> Result<Value, String> {
        if self.contexts.is_empty()
            && self.stack.is_empty()
            && self.row_values.is_empty()
            && self.calls == 0
            && self.bindings.is_none()
        {
            self.steps = 0;
        }
        let key = MemoKey::Symbol(symbol.clone());
        let cached = self
            .memo
            .lock()
            .expect("request cache poisoned")
            .get(&key)
            .cloned();
        if let Some(entry) = cached {
            if entry.value.is_err() && self.failure.is_none() {
                self.failure = entry.failure;
            }
            self.wanted.extend(entry.wanted);
            self.time_dependent |= entry.time_dependent;
            return entry.value;
        }
        if let Some(start) = self.stack.iter().position(|s| s == symbol) {
            let mut related = self.stack[start..].to_vec();
            related.push(symbol.clone());
            let message = format!(
                "Dependency cycle: {}",
                related
                    .iter()
                    .map(|s| self.workspace.named(s).name.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ")
            );
            self.failure = Some(EvalFailure {
                path: symbol.path.clone(),
                span: self.workspace.named(symbol).span,
                message: message.clone(),
                related,
            });
            return Err(message);
        }
        if self.stack.len() >= 64 {
            return Err("Dependency chain exceeds 64 levels".into());
        }
        let previous_failure = self.failure.take();
        let previous_time = std::mem::replace(&mut self.time_dependent, false);
        let wanted_start = self.wanted.len();
        self.stack.push(symbol.clone());
        // Named definitions never capture a caller's row locals.
        let caller_rows = std::mem::take(&mut self.row_values);
        let caller_locals = std::mem::take(&mut self.locals);
        let caller_bindings = self.bindings.take();
        let doc = &self.workspace.documents[&symbol.path];
        let result = match symbol.kind {
            SymbolKind::Definition(i) => {
                let def = &doc.definitions[i];
                if let Some(plan) = doc.plans.iter().find(|p| p.definition == i) {
                    crate::plans::solve(self, symbol, plan)
                } else if def.expression && crate::plans::seek_body(&def.source).is_some() {
                    crate::plans::seek(self, symbol)
                } else if let Some(table) = doc.tables.iter().find(|t| t.definition == i) {
                    if let Some(problem) = table.problems.first() {
                        self.failure = Some(EvalFailure {
                            path: symbol.path.clone(),
                            span: problem.span,
                            message: problem.message.clone(),
                            related: vec![],
                        });
                        Err(problem.message.clone())
                    } else {
                        self.table_value(symbol, table)
                    }
                } else if def.expression {
                    let raw = def.value_span.source(&doc.text);
                    let offset = raw.len() - raw.trim_start().len();
                    self.eval_at(
                        &symbol.path,
                        &def.source,
                        Span::new(
                            def.value_span.line,
                            def.value_span.start + offset,
                            def.value_span.end,
                        ),
                    )
                    .map(|v| match v {
                        Value::Timer(mut timer)
                            if timer.origin.is_none() && timer_arguments(&def.source).is_some() =>
                        {
                            std::sync::Arc::make_mut(&mut timer).origin = Some(symbol.clone());
                            Value::Timer(timer)
                        }
                        other => other,
                    })
                } else {
                    literal(&def.source).map(|v| match v {
                        Value::Resource(mut r) => {
                            r.origin = Some(symbol.path.clone());
                            Value::Resource(r)
                        }
                        other => other,
                    })
                }
            }
            SymbolKind::Task(i) => Ok(Value::Bool(self.task_done(&symbol.path, i))),
            SymbolKind::Section(i) => {
                let section = &doc.sections[i];
                Ok(Value::Tasks(
                    doc.tasks
                        .iter()
                        .enumerate()
                        .filter(|(i, t)| {
                            t.line > section.line
                                && t.line < section.end_line
                                && !doc.tasks.iter().any(|t| t.parent == Some(*i))
                        })
                        .map(|(i, _)| (symbol.path.clone(), i))
                        .collect(),
                ))
            }
            SymbolKind::Column(_, _) => {
                Err("A column needs a row context, e.g. sum(table, column)".into())
            }
            SymbolKind::Variable(plan, name) => {
                let name = doc.plans[plan].names[name].name.clone();
                let definition = Symbol {
                    path: symbol.path.clone(),
                    kind: SymbolKind::Definition(doc.plans[plan].definition),
                };
                self.symbol(&definition).and_then(|value| match value {
                    Value::Plan(p) => p.property(&name),
                    _ => Err("Expected a plan".into()),
                })
            }
        };
        self.row_values = caller_rows;
        self.locals = caller_locals;
        self.bindings = caller_bindings;
        self.stack.pop();
        let entry = MemoEntry {
            value: result.clone(),
            failure: if result.is_err() {
                self.failure.clone()
            } else {
                None
            },
            wanted: self.wanted[wanted_start..].to_vec(),
            time_dependent: self.time_dependent,
        };
        self.memo
            .lock()
            .expect("request cache poisoned")
            .insert(key, entry);
        self.failure = previous_failure.or(self.failure.take());
        self.time_dependent |= previous_time;
        result
    }
    pub(crate) fn expr(&mut self, path: &Path, expr: &Expr) -> Result<Value, String> {
        self.steps += 1;
        if self.steps > 200_000 {
            let message = "Evaluation exceeds 200,000 steps; simplify nested row calculations";
            self.fail(expr.bounds(), message);
            return Err(message.into());
        }
        match expr {
            Expr::Spanned(start, end, expr) => {
                let result = self.expr(path, expr);
                if let Err(message) = &result {
                    self.fail((*start, *end), message);
                }
                result
            }
            Expr::List(items) => {
                let value = Value::List(
                    items
                        .iter()
                        .map(|e| self.expr(path, e))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                crate::evaluate::functional::check_size(&value)?;
                Ok(value)
            }
            Expr::Record(fields) => {
                let value = Value::Record(
                    fields
                        .iter()
                        .map(|(k, e)| Ok((k.clone(), self.expr(path, e)?)))
                        .collect::<Result<BTreeMap<_, _>, String>>()?,
                );
                crate::evaluate::functional::check_size(&value)?;
                Ok(value)
            }
            Expr::Lambda(params, body) => {
                let mut captured = self
                    .row_values
                    .last()
                    .map(|s| s.values.clone())
                    .unwrap_or_default();
                if let Some(locals) = self.locals.last() {
                    captured.extend(locals.clone());
                }
                for (name, _) in expr.free_names() {
                    if !captured.contains_key(&name)
                        && let Some(value) = self.binding(&name)
                    {
                        captured.insert(name, value?);
                    }
                }
                Ok(Value::Function(std::sync::Arc::new(
                    crate::evaluate::functional::Function {
                        environment: self.environment.clone(),
                        expressions: self.expressions.clone(),
                        params: params.clone(),
                        body: *body.clone(),
                        path: path.into(),
                        source: self.contexts.last().cloned(),
                        captured,
                    },
                )))
            }
            Expr::Apply(function, args) => {
                let function = self.expr(path, function)?;
                let args = args
                    .iter()
                    .map(|e| self.expr(path, e))
                    .collect::<Result<Vec<_>, _>>()?;
                self.call(function, args)
            }
            Expr::Value(v) => Ok(v.clone()),
            Expr::Name(n) => match n.as_str() {
                "null" => Ok(Value::Null),
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                code if is_code(code) => Ok(Value::Text(code.to_string())),
                _ => {
                    if let Some(value) = self.locals.last().and_then(|s| s.get(n)) {
                        return Ok(value.clone());
                    }
                    if let Some(value) = self.binding(n) {
                        return value;
                    }
                    if let Some(scope) = self.row_values.last() {
                        if scope.decisions.contains_key(n) {
                            return Err(format!(
                                "'{n}' is a decision column; a plan chooses it, so sum over it inside maximize or minimize"
                            ));
                        }
                        scope.values.get(n).cloned().ok_or_else(|| {
                            format!("Unknown column '{n}' in table '{}'", scope.table)
                        })
                    } else {
                        self.named(path, n)
                    }
                }
            },
            Expr::Call(n, args) => {
                if n == "import" {
                    let [arg] = args.as_slice() else {
                        return Err("import expects a module ID or a literal note path".into());
                    };
                    let Value::Text(id) = self.expr(path, arg)? else {
                        return Err("import expects text".into());
                    };
                    if crate::model::imports::is_note_path(&id) {
                        if !matches!(arg.bare(), Expr::Value(Value::Text(_))) {
                            return Err("Note imports require a literal path, e.g. import(\"./values.wtf\")".into());
                        }
                        let target = crate::model::imports::note_path(path, &id)?;
                        if !self.workspace.documents.contains_key(&target) {
                            return Err(format!(
                                "Note import '{}' is not loaded (from {})",
                                target.display(),
                                path.display()
                            ));
                        }
                        return Ok(Value::Namespace(target));
                    }
                    return self.import(&id);
                }
                if n == "if" {
                    if args.len() != 3 {
                        return Err("if expects a condition and two branches".into());
                    }
                    let Value::Bool(condition) = self.expr(path, &args[0])? else {
                        return Err("if requires a Boolean condition".into());
                    };
                    return self.expr(path, &args[if condition { 1 } else { 2 }]);
                }
                if n == "coalesce" {
                    for arg in args {
                        let value = self.expr(path, arg)?;
                        if value != Value::Null {
                            return Ok(value);
                        }
                    }
                    return Ok(Value::Null);
                }
                if crate::evaluate::functional::is_builtin(n) {
                    let values = args
                        .iter()
                        .map(|e| self.expr(path, e))
                        .collect::<Result<Vec<_>, _>>()?;
                    return self.functional(n, values);
                }
                if !is_builtin_function(n) {
                    let function = self
                        .locals
                        .last()
                        .and_then(|s| s.get(n))
                        .cloned()
                        .map(Ok)
                        .or_else(|| self.binding(n))
                        .unwrap_or_else(|| self.named(path, n))?;
                    let values = args
                        .iter()
                        .map(|e| self.expr(path, e))
                        .collect::<Result<Vec<_>, _>>()?;
                    return self.call(function, values);
                }
                if n == "sum" && args.len() == 1 {
                    let Value::List(values) = self.expr(path, &args[0])? else {
                        return Err("sum expects a list, or a table and row expression".into());
                    };
                    return crate::evaluate::functional::sum(values);
                }
                if n == "eval" && args.len() == 1 {
                    if self.calls >= 32 {
                        return Err("Function call depth exceeds 32".into());
                    }
                    let Value::Text(source) = self.expr(path, &args[0])? else {
                        return Err("eval expects expression text".into());
                    };
                    // Dynamic expressions use the current document, not query row fields.
                    let bindings = self.bindings.take();
                    let locals = std::mem::take(&mut self.locals);
                    self.calls += 1;
                    let result = self.eval(path, &source);
                    self.calls -= 1;
                    self.locals = locals;
                    self.bindings = bindings;
                    return result;
                }
                if n == "sum" {
                    return self.sum(path, args).map(|(value, _)| value);
                }
                if n == "now" && args.is_empty() {
                    self.time_dependent = true;
                    return Ok(Value::DateTime(self.now));
                }
                if matches!(n.as_str(), "stopwatch" | "countdown") {
                    let values = args
                        .iter()
                        .map(|a| self.expr(path, a))
                        .collect::<Result<Vec<_>, _>>()?;
                    let time_dependent = self.time_dependent;
                    let timer = Timer::new(self, n, &values)?;
                    // The module declares whether this resolved state still needs a clock.
                    self.time_dependent = time_dependent || timer.time_dependent()?;
                    return Ok(Value::Timer(std::sync::Arc::new(timer)));
                }
                if n == "today" && args.is_empty() {
                    return Ok(Value::Date(self.today));
                }
                if matches!(n.as_str(), "rate" | "to" | "forecast" | "quote") {
                    return self.lookup(path, n, args);
                }
                if args.len() != 1 {
                    return Err(format!("{n} expects one argument"));
                }
                let value = self.expr(path, &args[0])?;
                if n == "date" {
                    return match value {
                        Value::Text(s) => date_value(&s)
                            .or_else(|| relative_date(&s, self.today).map(Value::Date))
                            .ok_or("Unrecognized date".into()),
                        other => self.date(&other).map(Value::Date),
                    };
                }
                let Value::Tasks(tasks) = value else {
                    return Err(format!("{n} expects a named checklist heading"));
                };
                let done = tasks.iter().filter(|(p, i)| self.task_done(p, *i)).count();
                match n.as_str() {
                    "total" => Ok(Value::Count(tasks.len())),
                    "completed" => Ok(Value::Count(done)),
                    "remaining" => Ok(Value::Count(tasks.len() - done)),
                    "effort" => {
                        let mut seconds = 0i64;
                        for (p, i) in tasks {
                            if !self.task_done(&p, i) {
                                let task = &self.workspace.documents[&p].tasks[i];
                                if let Some(attr) = task.attributes.get("estimate") {
                                    let Value::Duration(m) = self.eval(&p, &attr.value)? else {
                                        return Err("@estimate requires a duration".into());
                                    };
                                    if m < 0 {
                                        return Err("Estimate cannot be negative".into());
                                    }
                                    seconds = seconds.checked_add(m).ok_or("Duration overflow")?;
                                }
                            }
                        }
                        Ok(Value::Duration(seconds))
                    }
                    _ => Err(format!("Unknown function '{n}'")),
                }
            }
            Expr::Unary(op, v) => {
                let v = self.expr(path, v)?;
                match (op, v) {
                    (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (UnaryOp::Negate, Value::Number(n)) => Ok(Value::Number(-n)),
                    (UnaryOp::Negate, Value::Money(n, c)) => Ok(Value::Money(-n, c)),
                    (UnaryOp::Negate, Value::Ratio(n)) => Ok(Value::Ratio(-n)),
                    (UnaryOp::Negate, Value::Duration(n)) => n
                        .checked_neg()
                        .map(Value::Duration)
                        .ok_or("Duration overflow".into()),
                    (UnaryOp::Plus, v) if v.scalar().is_some() => Ok(v),
                    _ => Err("Invalid unary operation".into()),
                }
            }
            Expr::Binary(op, a, b) => {
                let a = self.expr(path, a)?;
                if *op == BinaryOp::And && a == Value::Bool(false) {
                    return Ok(a);
                }
                if *op == BinaryOp::Or && a == Value::Bool(true) {
                    return Ok(a);
                }
                let right = self.expr(path, b)?;
                let types = format!("{} {op} {}", a.type_name(), right.type_name());
                binary(*op, a, right).map_err(|message| {
                    let message = format!("{message} ({types})");
                    self.fail(b.bounds(), &message);
                    message
                })
            }
            Expr::Property(v, key) => {
                let v = self.expr(path, v)?;
                match v {
                    Value::Resource(resource) => {
                        if key == "url" {
                            return Ok(Value::Text(resource.target));
                        }
                        if key == "exists" {
                            if self.pure {
                                return Err("Module evaluation cannot access the filesystem".into());
                            }
                            #[cfg(target_arch = "wasm32")]
                            return Err("Local file existence is unavailable in the browser".into());
                            #[cfg(not(target_arch = "wasm32"))]
                            return Ok(Value::Bool(
                                resource
                                    .url(path)?
                                    .to_file_path()
                                    .map(|p| p.exists())
                                    .unwrap_or(false),
                            ));
                        }
                        self.time_dependent |= self.link_features.time_dependent(
                            &resource.target,
                            &self.workspace.cache,
                            self.now.to_utc(),
                        );
                        let memo_key =
                            MemoKey::ResourceProperty(resource.target.clone(), key.clone());
                        if let Some(entry) = self
                            .memo
                            .lock()
                            .expect("request cache poisoned")
                            .get(&memo_key)
                            .cloned()
                        {
                            return entry.value;
                        }
                        let value = self.link_features.property(
                            &resource.target,
                            &self.workspace.cache,
                            self.now.to_utc(),
                            key,
                        );
                        self.memo.lock().expect("request cache poisoned").insert(
                            memo_key,
                            MemoEntry {
                                value: value.clone(),
                                failure: None,
                                wanted: vec![],
                                time_dependent: false,
                            },
                        );
                        value
                    }
                    other => self.property(&other, key),
                }
            }
        }
    }
    fn property(&mut self, value: &Value, key: &str) -> Result<Value, String> {
        match value {
            Value::Namespace(path) => self.named(path, key),
            Value::List(items) => items
                .iter()
                .map(|v| self.property(v, key))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            _ => value.property(key),
        }
    }
    /// Restrict evaluation to immutable inputs, including resource properties.
    pub fn pure(mut self) -> Self {
        self.pure = true;
        self
    }
    pub(crate) fn with_expressions(
        mut self,
        expressions: std::sync::Arc<BTreeMap<String, Expr>>,
    ) -> Self {
        self.expressions = Some(expressions);
        self
    }
    pub(crate) fn with_environment(mut self, environment: std::sync::Arc<Workspace>) -> Self {
        self.environment = Some(environment);
        self
    }
    fn module_engine<'b>(&self, workspace: &'b std::sync::Arc<Workspace>) -> Engine<'b> {
        let mut engine = Engine::at(workspace, self.now)
            .pure()
            .with_link_features(crate::link_features::LinkFeatures::new(&[]))
            .with_environment(workspace.clone());
        engine.steps = self.steps;
        engine.calls = self.calls;
        engine.today = self.today;
        engine.memo = self.memo.clone();
        engine
    }
    fn absorb_module(&mut self, other: &Engine<'_>) {
        self.steps = other.steps;
        self.time_dependent |= other.time_dependent;
        if self.failure.is_none() {
            // A module has its own source workspace. Let the caller attach an
            // error there to its call site instead of carrying an unusable span.
            self.failure = other
                .failure
                .clone()
                .filter(|failure| self.workspace.documents.contains_key(&failure.path));
        }
    }
    /// Typed adapters use the same module snapshot, clock and execution budget as imports.
    pub(crate) fn call_module(
        &mut self,
        id: &str,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let module = self
            .workspace
            .modules
            .active()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("Module '{id}' is unavailable or disabled"))?
            .clone();
        let workspace = module.environment();
        let mut engine = self
            .module_engine(&workspace)
            .with_expressions(module.expressions.clone());
        let result = engine
            .named(&module.path, name)
            .and_then(|function| engine.call(function, args));
        self.absorb_module(&engine);
        result
    }
    fn import(&mut self, id: &str) -> Result<Value, String> {
        let module = self
            .workspace
            .modules
            .active()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("Unknown or undeclared import '{id}'"))?
            .clone();
        let workspace = module.environment();
        let mut engine = self
            .module_engine(&workspace)
            .with_expressions(module.expressions.clone());
        let result = workspace.documents[&module.path]
            .definitions
            .iter()
            .filter(|d| d.named.name != "module" && !d.named.name.starts_with('_'))
            .map(|d| {
                Ok((
                    d.named.name.clone(),
                    engine.named(&module.path, &d.named.name)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(Value::Record);
        self.absorb_module(&engine);
        result
    }
    pub fn call(&mut self, function: Value, args: Vec<Value>) -> Result<Value, String> {
        let Value::Function(function) = function else {
            return Err("Expected a function".into());
        };
        if let Some(workspace) = &function.environment
            && !std::ptr::eq(self.workspace, workspace.as_ref())
        {
            let mut engine = self.module_engine(workspace);
            engine.expressions = function.expressions.clone();
            let result = engine.call(Value::Function(function.clone()), args);
            self.absorb_module(&engine);
            return result;
        }
        if args.len() != function.params.len() {
            return Err(format!(
                "Function expects {} arguments, got {}",
                function.params.len(),
                args.len()
            ));
        }
        if self.calls >= 32 {
            return Err("Function call depth exceeds 32".into());
        }
        let mut locals = function.captured.clone();
        locals.extend(function.params.iter().cloned().zip(args));
        self.locals.push(locals);
        let rows = std::mem::take(&mut self.row_values);
        let bindings = self.bindings.take();
        self.calls += 1;
        if let Some(source) = &function.source {
            self.contexts.push(source.clone());
        }
        let result = self.expr(&function.path, &function.body);
        if function.source.is_some() {
            self.contexts.pop();
        }
        self.calls -= 1;
        self.row_values = rows;
        self.bindings = bindings;
        self.locals.pop();
        let value = result?;
        crate::evaluate::functional::check_size(&value)?;
        Ok(value)
    }
    pub(crate) fn functional(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        use Value::*;
        let value = match (name, args.as_slice()) {
            ("get", [Namespace(path), Text(key)]) => match self.workspace.resolve(path, key) {
                Ok(symbol) => self.symbol(&symbol)?,
                Err(_)
                    if !self
                        .workspace
                        .symbols()
                        .iter()
                        .any(|s| s.path == *path && self.workspace.named(s).name == *key) =>
                {
                    Null
                }
                Err(e) => return Err(e),
            },
            ("sort_by", [List(items), function @ Function(_)]) => {
                let mut keyed = Vec::new();
                let mut first = None;
                for item in items {
                    let key = self.call(function.clone(), vec![item.clone()])?;
                    crate::evaluate::functional::compare(&key, &key)?;
                    if key != Null {
                        if let Some(first) = &first {
                            crate::evaluate::functional::compare(first, &key)?;
                        } else {
                            first = Some(key.clone());
                        }
                    }
                    keyed.push((key, item.clone()));
                }
                // Every key has been checked against the common scalar type.
                keyed.sort_by(|(a, _), (b, _)| crate::evaluate::functional::compare(a, b).unwrap());
                List(keyed.into_iter().map(|(_, item)| item).collect())
            }
            ("group_by", [List(items), function @ Function(_)]) => {
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for item in items {
                    let key = self.call(function.clone(), vec![item.clone()])?;
                    crate::evaluate::functional::compare(&key, &key)?;
                    if let Some((_, rows)) = groups.iter_mut().find(|(k, _)| {
                        binary(BinaryOp::Equal, k.clone(), key.clone()) == Ok(Bool(true))
                    }) {
                        rows.push(item.clone());
                    } else {
                        groups.push((key, vec![item.clone()]));
                    }
                }
                List(
                    groups
                        .into_iter()
                        .map(|(key, rows)| {
                            Record([("key".into(), key), ("rows".into(), List(rows))].into())
                        })
                        .collect(),
                )
            }
            ("map" | "filter", [List(items), function @ Function(_)]) => {
                let mut output = Vec::new();
                for item in items {
                    let value = self.call(function.clone(), vec![item.clone()])?;
                    if name == "map" {
                        output.push(value);
                    } else {
                        match value {
                            Bool(true) => output.push(item.clone()),
                            Bool(false) => (),
                            _ => return Err("filter predicate must return a Boolean".into()),
                        }
                    }
                    if output.len() > 4096 {
                        return Err("List exceeds 4096 items".into());
                    }
                }
                List(output)
            }
            ("fold", [List(items), initial, function @ Function(_)]) => {
                let mut result = initial.clone();
                for item in items {
                    result = self.call(function.clone(), vec![result, item.clone()])?;
                }
                result
            }
            _ => crate::evaluate::functional::builtin(name, &args)?,
        };
        crate::evaluate::functional::check_size(&value)?;
        Ok(value)
    }
    pub fn task_done(&self, path: &Path, i: usize) -> bool {
        let doc = &self.workspace.documents[path];
        let children: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent == Some(i))
            .map(|(j, _)| j)
            .collect();
        if children.is_empty() {
            doc.tasks[i].checked
        } else {
            children.into_iter().all(|j| self.task_done(path, j))
        }
    }

    fn sum(&mut self, path: &Path, args: &[Expr]) -> Result<(Value, Vec<Value>), String> {
        if args.len() != 2 {
            return Err(
                "sum expects a table and a row expression: sum(groceries, quantity * price)".into(),
            );
        }
        let Expr::Name(name) = args[0].bare() else {
            return Err("The first argument to sum must be a table name".into());
        };
        let Value::Table(table) = self.named(path, name)? else {
            return Err(format!("'{name}' is not a table"));
        };
        let mut total = None;
        let mut contributions = Vec::new();
        let decisions = self.decision_columns(&table);
        for row in &table.rows {
            self.row_values.push(RowScope {
                table: name.clone(),
                values: table
                    .columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect(),
                decisions: decisions
                    .keys()
                    .map(|c| (c.clone(), String::new()))
                    .collect(),
            });
            let value = self.expr(path, &args[1]);
            self.row_values.pop();
            let value = value?;
            if !matches!(
                value,
                Value::Number(_) | Value::Money(..) | Value::Ratio(_) | Value::Duration(_)
            ) {
                return Err(format!(
                    "sum requires numeric, money, ratio, or duration results, found {}",
                    value.type_name()
                ));
            }
            total = Some(if let Some(previous) = total {
                let ratios =
                    matches!(previous, Value::Ratio(_)) && matches!(value, Value::Ratio(_));
                let added = binary(BinaryOp::Add, previous, value.clone())?;
                if ratios && let Value::Number(n) = added {
                    Value::Ratio(n)
                } else {
                    added
                }
            } else {
                value.clone()
            });
            contributions.push(value);
        }
        total.map(|v| (v, contributions)).ok_or_else(|| {
            "Cannot sum an empty table: add a row to establish its value type".into()
        })
    }

    /// Rows of a table, evaluating calculated cells and checking that each
    /// column keeps one type. Failures point at the offending cell.
    fn table_value(
        &mut self,
        symbol: &Symbol,
        table: &crate::tables::Table,
    ) -> Result<Value, String> {
        let mut types: Vec<Option<ValueType>> = table.types.clone();
        let mut rows = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            let mut values = Vec::with_capacity(row.len());
            for (column, cell) in row.iter().enumerate() {
                let value = match &cell.expression {
                    Some((inner, span)) => {
                        let value = self.eval_at(&symbol.path, inner, *span)?;
                        if matches!(
                            value,
                            Value::Table(_) | Value::Plan(_) | Value::Tasks(_) | Value::Timer(_)
                        ) {
                            let message = format!(
                                "A cell cannot hold a {}; use a scalar value",
                                value.type_name()
                            );
                            self.failure.get_or_insert(EvalFailure {
                                path: symbol.path.clone(),
                                span: *span,
                                message: message.clone(),
                                related: vec![],
                            });
                            return Err(message);
                        }
                        if let Some(expected) = types.get(column).copied().flatten() {
                            if expected != value.kind() {
                                let message = format!(
                                    "Column '{}' expects {expected}, found {}",
                                    table.columns[column].name,
                                    value.type_name()
                                );
                                self.failure.get_or_insert(EvalFailure {
                                    path: symbol.path.clone(),
                                    span: *span,
                                    message: message.clone(),
                                    related: vec![],
                                });
                                return Err(message);
                            }
                        } else if let Some(slot) = types.get_mut(column) {
                            *slot = Some(value.kind());
                        }
                        value
                    }
                    None => cell.value.clone().map_err(|e| e.to_string())?,
                };
                values.push(value);
            }
            rows.push(values);
        }
        Ok(Value::Table(std::sync::Arc::new(
            crate::tables::TableValue {
                origin: symbol.clone(),
                columns: table.columns.iter().map(|c| c.name.clone()).collect(),
                rows,
            },
        )))
    }
    /// `rate(EUR, USD)`, `to(money, USD)`, `forecast("Oaxaca", 2026-11-20[, F])`
    /// and `quote(NVDA)`: values from the lookup cache, never fetched here.
    fn lookup(&mut self, path: &Path, name: &str, args: &[Expr]) -> Result<Value, String> {
        let code = |value: Value, what: &str| match value {
            Value::Text(code) => Ok(code),
            other => Err(format!(
                "{what} must be a code such as USD, found {}",
                other.type_name()
            )),
        };
        let currency = |code: &str| {
            Currency::parse(code)
                .ok_or_else(|| format!("'{code}' is not a currency code such as USD"))
        };
        match name {
            "rate" => {
                if args.len() != 2 {
                    return Err("rate expects two currency codes: rate(EUR, USD)".into());
                }
                let from = currency(&code(self.expr(path, &args[0])?, "The first currency")?)?;
                let to = currency(&code(self.expr(path, &args[1])?, "The second currency")?)?;
                if from != to {
                    self.wanted.push(crate::lookups::LookupKey::rate(from, to));
                }
                crate::lookups::rate(&self.workspace.lookups, from, to).map(Value::Number)
            }
            "to" => {
                if args.len() != 2 {
                    return Err(
                        "to expects a money value and a currency code: to(hotel, USD)".into(),
                    );
                }
                let Value::Money(amount, from) = self.expr(path, &args[0])? else {
                    return Err("to converts money; the first argument is not money".into());
                };
                let to = currency(&code(self.expr(path, &args[1])?, "The currency")?)?;
                if from != to {
                    self.wanted.push(crate::lookups::LookupKey::rate(from, to));
                }
                let rate = crate::lookups::rate(&self.workspace.lookups, from, to)?;
                Ok(Value::Money(amount * rate, to))
            }
            "quote" => {
                if args.len() != 1 {
                    return Err("quote expects a ticker symbol: quote(NVDA)".into());
                }
                let symbol = code(self.expr(path, &args[0])?, "The ticker")?;
                self.wanted.push(crate::lookups::LookupKey::quote(&symbol));
                crate::lookups::quote(&self.workspace.lookups, &symbol)
            }
            _ => {
                if !(2..=3).contains(&args.len()) {
                    return Err(
                        "forecast expects a place and a date: forecast(\"Oaxaca\", 2026-11-20)"
                            .into(),
                    );
                }
                let Value::Text(place) = self.expr(path, &args[0])? else {
                    return Err(
                        "The place must be text, e.g. forecast(\"Oaxaca\", 2026-11-20)".into(),
                    );
                };
                let value = self.expr(path, &args[1])?;
                let date = self.date(&value)?;
                let fahrenheit = match args.get(2) {
                    Some(unit) => match code(self.expr(path, unit)?, "The unit")?.as_str() {
                        "F" | "FAHRENHEIT" => true,
                        "C" | "CELSIUS" => false,
                        other => {
                            return Err(format!("Unknown temperature unit '{other}'; use F or C"));
                        }
                    },
                    None => false,
                };
                self.wanted
                    .push(crate::lookups::LookupKey::forecast(&place, date));
                crate::lookups::forecast(&self.workspace.lookups, &place, date, fahrenheit)
                    .map(Value::Forecast)
            }
        }
    }
    pub fn sum_contributions(&mut self, path: &Path, source: &str) -> Option<Vec<Value>> {
        let parsed = Parser::parse(source).ok()?;
        let Expr::Call(name, args) = parsed.bare() else {
            return None;
        };
        (name == "sum")
            .then(|| self.sum(path, args).ok().map(|(_, rows)| rows))
            .flatten()
    }
    pub fn blocked(&mut self, path: &Path, i: usize) -> Result<Vec<String>, String> {
        self.blocked_inner(path, i, &mut Vec::new())
    }
    fn blocked_inner(
        &mut self,
        path: &Path,
        i: usize,
        stack: &mut Vec<TaskKey>,
    ) -> Result<Vec<String>, String> {
        let key = (path.to_path_buf(), i);
        if let Some(start) = stack.iter().position(|k| k == &key) {
            let related: Vec<_> = stack[start..]
                .iter()
                .chain(std::iter::once(&key))
                .filter(|(p, index)| self.workspace.documents[p].tasks[*index].named.is_some())
                .map(|(p, index)| Symbol {
                    path: p.clone(),
                    kind: SymbolKind::Task(*index),
                })
                .collect();
            let names = related
                .iter()
                .map(|s| self.workspace.named(s).name.as_str())
                .collect::<Vec<_>>();
            let message = format!("Task dependency cycle: {}", names.join(" → "));
            let task = &self.workspace.documents[path].tasks[i];
            self.failure = Some(EvalFailure {
                path: path.into(),
                span: task
                    .attributes
                    .get("after")
                    .map(|a| a.value_span)
                    .unwrap_or(task.checkbox),
                message: message.clone(),
                related,
            });
            return Err(message);
        }
        if stack.len() > 64 {
            return Err("Task dependency chain is too deep".into());
        }
        stack.push(key);
        let task = &self.workspace.documents[path].tasks[i];
        let mut blocked = Vec::new();
        if let Some(attr) = task.attributes.get("after") {
            for name in attr.value.split(',').map(str::trim) {
                if let Ok(s) = self.workspace.resolve(path, name)
                    && let SymbolKind::Task(j) = s.kind
                {
                    self.blocked_inner(&s.path, j, stack)?;
                }
                let ready = match self.eval(path, name)? {
                    Value::Bool(b) => b,
                    Value::Tasks(ts) => ts.iter().all(|(p, j)| self.task_done(p, *j)),
                    _ => {
                        return Err(
                            "@after requires task names, checklists, or boolean expressions".into(),
                        );
                    }
                };
                if !ready {
                    blocked.push(name.to_string());
                }
            }
        }
        stack.pop();
        Ok(blocked)
    }
    pub fn when(&mut self, path: &Path, source: &str) -> Result<Value, String> {
        if let Some(v) = date_value(source) {
            return Ok(v);
        }
        if let Some(v) = relative_date(source, self.today) {
            return Ok(Value::Date(v));
        }
        let value = self.eval(path, source)?;
        self.date(&value)?;
        Ok(value)
    }
}
