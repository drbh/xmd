//! Evaluation: the engine that turns a definition into a value, split into the
//! values it produces, the syntax it reads, and the linear forms plans need.
use crate::{
    document::Span,
    error::{Depth, EvalError, EvalResult, Overflow},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset, NaiveDate};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
mod arithmetic;
mod builtins;
mod calls;
mod linear;
mod syntax;
mod value;
pub(crate) use arithmetic::binary;
pub use arithmetic::{BinaryOp, Comparison, Operator, UnaryOp};
pub use builtins::Builtin;
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
    pub message: EvalError,
    pub related: Vec<Symbol>,
}
impl EvalFailure {
    /// The rendered sentence, for hosts that only display it.
    pub fn message(&self) -> String {
        self.message.to_string()
    }
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MemoKey {
    Symbol(Symbol),
    ResourceProperty(String, String),
}
#[derive(Clone)]
pub(crate) struct MemoEntry {
    value: EvalResult<Value>,
    failure: Option<EvalFailure>,
    wanted: Vec<crate::lookups::LookupKey>,
    time_dependent: bool,
}
/// Host-provided names are resolved lazily by the same evaluator as note functions.
/// Resolution runs without the caller's bindings, so definitions cannot capture them.
pub(crate) trait Bindings: Send + Sync {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<EvalResult<Value>>;
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
    pub fn date(&self, value: &Value) -> EvalResult<NaiveDate> {
        self.request()
            .clock()
            .date(value)
            .ok_or(EvalError::Expected("a date or appointment time"))
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
    ) -> EvalResult<Value> {
        let previous = self.bindings.replace(bindings);
        let result = self.expr(path, expr);
        self.bindings = previous;
        result
    }
    fn binding(&mut self, name: &str) -> Option<EvalResult<Value>> {
        let bindings = self.bindings.take()?;
        let result = bindings.get(name, self);
        self.bindings = Some(bindings);
        result
    }
    pub fn eval(&mut self, path: &Path, expression: &str) -> EvalResult<Value> {
        self.eval_at(path, expression, Span::new(0, 0, expression.len()))
    }
    pub fn eval_at(&mut self, path: &Path, expression: &str, span: Span) -> EvalResult<Value> {
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
                let error = EvalError::Parse(message);
                self.fail(bounds, &error);
                Err(error)
            }
        };
        self.contexts.pop();
        result
    }
    fn fail(&mut self, bounds: (usize, usize), message: &EvalError) {
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
                message: message.clone(),
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
                Expr::Call(_, args) | Expr::Builtin(_, args) | Expr::List(args) => {
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
    pub fn substituted(&mut self, path: &Path, source: &str) -> EvalResult<String> {
        let tokens = lex(source).map_err(EvalError::Parse)?;
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
    pub fn named(&mut self, path: &Path, name: &str) -> EvalResult<Value> {
        let symbol = self.workspace.resolve(path, name)?;
        self.symbol(&symbol)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> EvalResult<Value> {
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
            let message = EvalError::Cycle {
                names: related
                    .iter()
                    .map(|s| self.workspace.named(s).name.clone())
                    .collect(),
            };
            self.failure = Some(EvalFailure {
                path: symbol.path.clone(),
                span: self.workspace.named(symbol).span,
                message: message.clone(),
                related,
            });
            return Err(message);
        }
        if self.stack.len() >= 64 {
            return Err(EvalError::DepthExceeded(Depth::Dependency));
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
                        let message = EvalError::Message(problem.message.clone());
                        self.failure = Some(EvalFailure {
                            path: symbol.path.clone(),
                            span: problem.span,
                            message: message.clone(),
                            related: vec![],
                        });
                        Err(message)
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
            SymbolKind::Column(_, _) => Err(EvalError::Message(
                "A column needs a row context, e.g. sum(table, column)".into(),
            )),
            SymbolKind::Variable(plan, name) => {
                let name = doc.plans[plan].names[name].name.clone();
                let definition = Symbol {
                    path: symbol.path.clone(),
                    kind: SymbolKind::Definition(doc.plans[plan].definition),
                };
                self.symbol(&definition).and_then(|value| match value {
                    Value::Plan(p) => p.property(&name),
                    _ => Err(EvalError::Expected("a plan")),
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
    /// One node, one dispatch: every kind of expression names the method that
    /// answers it.
    pub(crate) fn expr(&mut self, path: &Path, expr: &Expr) -> EvalResult<Value> {
        self.steps += 1;
        if self.steps > 200_000 {
            let message = EvalError::StepLimit;
            self.fail(expr.bounds(), &message);
            return Err(message);
        }
        match expr {
            Expr::Spanned(start, end, inner) => {
                let result = self.expr(path, inner);
                if let Err(message) = &result {
                    self.fail((*start, *end), message);
                }
                result
            }
            Expr::Value(v) => Ok(v.clone()),
            Expr::Name(n) => self.name(path, n),
            Expr::Builtin(builtin, args) => self.builtin(path, *builtin, args),
            Expr::Call(n, args) => self.call_named(path, n, args),
            Expr::Lambda(params, body) => self.lambda(path, expr, params, body),
            Expr::Apply(function, args) => {
                let function = self.expr(path, function)?;
                let args = self.values(path, args)?;
                self.call(function, args)
            }
            Expr::List(items) => {
                let value = Value::List(self.values(path, items)?);
                sized(value)
            }
            Expr::Record(fields) => self.record(path, fields),
            Expr::Unary(op, v) => {
                let v = self.expr(path, v)?;
                unary(*op, v)
            }
            Expr::Binary(op, a, b) => self.binary_expr(path, *op, a, b),
            Expr::Property(v, key) => {
                let value = self.expr(path, v)?;
                self.access(path, value, key)
            }
        }
    }
    /// Every argument of a call or list, left to right.
    pub(crate) fn values(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Vec<Value>> {
        args.iter().map(|e| self.expr(path, e)).collect()
    }
    fn record(&mut self, path: &Path, fields: &[(String, Expr)]) -> EvalResult<Value> {
        let value = Value::Record(
            fields
                .iter()
                .map(|(k, e)| Ok((k.clone(), self.expr(path, e)?)))
                .collect::<EvalResult<BTreeMap<_, _>>>()?,
        );
        sized(value)
    }
    /// A lambda captures the names it reads: the enclosing row scope and
    /// locals, then whatever bindings answer for its remaining free names.
    fn lambda(
        &mut self,
        path: &Path,
        expr: &Expr,
        params: &[String],
        body: &Expr,
    ) -> EvalResult<Value> {
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
                params: params.to_vec(),
                body: body.clone(),
                path: path.into(),
                source: self.contexts.last().cloned(),
                captured,
            },
        )))
    }
    /// A bare name: the literals it may spell, then locals, bindings, the row
    /// scope, and finally the workspace.
    fn name(&mut self, path: &Path, n: &str) -> EvalResult<Value> {
        match n {
            "null" => return Ok(Value::Null),
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            code if is_code(code) => return Ok(Value::Text(code.to_string())),
            _ => (),
        }
        if let Some(value) = self.locals.last().and_then(|s| s.get(n)) {
            return Ok(value.clone());
        }
        if let Some(value) = self.binding(n) {
            return value;
        }
        let Some(scope) = self.row_values.last() else {
            return self.named(path, n);
        };
        if scope.decisions.contains_key(n) {
            return Err(EvalError::DecisionColumnOutsidePlan(n.into()));
        }
        scope
            .values
            .get(n)
            .cloned()
            .ok_or_else(|| EvalError::UnknownColumn {
                name: n.into(),
                table: scope.table.clone(),
            })
    }
    fn binary_expr(&mut self, path: &Path, op: BinaryOp, a: &Expr, b: &Expr) -> EvalResult<Value> {
        let a = self.expr(path, a)?;
        // A decided `and` or `or` never evaluates its right-hand side.
        if op == BinaryOp::And && a == Value::Bool(false) {
            return Ok(a);
        }
        if op == BinaryOp::Or && a == Value::Bool(true) {
            return Ok(a);
        }
        let right = self.expr(path, b)?;
        let (left, right_kind) = (a.kind(), right.kind());
        binary(op, a, right).map_err(|source| {
            let message = source.in_binary(op, left, right_kind);
            self.fail(b.bounds(), &message);
            message
        })
    }
    /// `value.key`: a resource answers from the link registry and its cache,
    /// everything else from its own structure.
    fn access(&mut self, path: &Path, value: Value, key: &str) -> EvalResult<Value> {
        let Value::Resource(resource) = value else {
            return self.property(&value, key);
        };
        if key == "url" {
            return Ok(Value::Text(resource.target));
        }
        if key == "exists" {
            if self.pure {
                return Err(EvalError::Message(
                    "Module evaluation cannot access the filesystem".into(),
                ));
            }
            #[cfg(target_arch = "wasm32")]
            return Err(EvalError::Message(
                "Local file existence is unavailable in the browser".into(),
            ));
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
        let memo_key = MemoKey::ResourceProperty(resource.target.clone(), key.into());
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
    fn property(&mut self, value: &Value, key: &str) -> EvalResult<Value> {
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

    fn table_value(&mut self, symbol: &Symbol, table: &crate::tables::Table) -> EvalResult<Value> {
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
                            let message = EvalError::Message(format!(
                                "A cell cannot hold a {}; use a scalar value",
                                value.type_name()
                            ));
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
                                let message = EvalError::Message(format!(
                                    "Column '{}' expects {expected}, found {}",
                                    table.columns[column].name,
                                    value.type_name()
                                ));
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
                    None => cell.value.clone()?,
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
    pub fn sum_contributions(&mut self, path: &Path, source: &str) -> Option<Vec<Value>> {
        let parsed = Parser::parse(source).ok()?;
        let Expr::Builtin(Builtin::Sum, args) = parsed.bare() else {
            return None;
        };
        self.sum(path, args).ok().map(|(_, rows)| rows)
    }
    pub fn blocked(&mut self, path: &Path, i: usize) -> EvalResult<Vec<String>> {
        self.blocked_inner(path, i, &mut Vec::new())
    }
    fn blocked_inner(
        &mut self,
        path: &Path,
        i: usize,
        stack: &mut Vec<TaskKey>,
    ) -> EvalResult<Vec<String>> {
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
            let message = EvalError::TaskCycle {
                names: related
                    .iter()
                    .map(|s| self.workspace.named(s).name.clone())
                    .collect(),
            };
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
            return Err(EvalError::DepthExceeded(Depth::Task));
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
                        return Err(EvalError::Message(
                            "@after requires task names, checklists, or boolean expressions".into(),
                        ));
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
    pub fn when(&mut self, path: &Path, source: &str) -> EvalResult<Value> {
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

/// Values are capped wherever one is built, not only where one is returned.
fn sized(value: Value) -> EvalResult<Value> {
    crate::evaluate::functional::check_size(&value)?;
    Ok(value)
}
fn unary(op: UnaryOp, v: Value) -> EvalResult<Value> {
    match (op, v) {
        (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnaryOp::Negate, Value::Number(n)) => Ok(Value::Number(-n)),
        (UnaryOp::Negate, Value::Money(n, c)) => Ok(Value::Money(-n, c)),
        (UnaryOp::Negate, Value::Ratio(n)) => Ok(Value::Ratio(-n)),
        (UnaryOp::Negate, Value::Duration(n)) => n
            .checked_neg()
            .map(Value::Duration)
            .ok_or(EvalError::Overflowed(Overflow::Duration)),
        (UnaryOp::Plus, v) if v.scalar().is_some() => Ok(v),
        _ => Err(EvalError::Message("Invalid unary operation".into())),
    }
}
