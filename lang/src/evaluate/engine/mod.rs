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
    Code, Currency, TaskKey, Value, ValueType, date_value, decimal, duration, is_code,
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

/// How much work one evaluation may still do. A module evaluates on an engine
/// of its own, which inherits the budget rather than being given a fresh one.
#[derive(Default)]
struct Budget {
    steps: usize,
    calls: usize,
}
/// What is being walked right now: enough to name a cycle, and to place a
/// failure in the note that asked for it.
#[derive(Default)]
struct Trace {
    /// Definitions on the value stack.
    symbols: Vec<Symbol>,
    /// Definitions being walked symbolically, separate from the value stack:
    /// a goal seek is legitimately on both at once.
    linear: Vec<Symbol>,
    /// The note and span each nested evaluation belongs to.
    contexts: Vec<(PathBuf, Span)>,
}
pub struct Engine<'a> {
    pub workspace: &'a Workspace,
    /// The one clock snapshot every date in this evaluation is read against.
    pub today: NaiveDate,
    pub now: DateTime<FixedOffset>,
    features: crate::link_features::LinkFeatures<'a>,
    memo: std::sync::Arc<std::sync::Mutex<BTreeMap<MemoKey, MemoEntry>>>,
    /// The dynamic environment a name is resolved against, innermost last.
    frames: Vec<Frame>,
    budget: Budget,
    trace: Trace,
    pub failure: Option<EvalFailure>,
    /// Lookup keys read during evaluation, hit or miss, for hovers and refresh.
    pub wanted: Vec<crate::lookups::LookupKey>,
    pub time_dependent: bool,
    /// Decision-column variables met while linearizing a plan.
    pub row_variables: Vec<RowVariable>,
    /// A module's evaluation: immutable inputs only, its own workspace, and
    /// the expressions its note was parsed into.
    pure: bool,
    environment: Option<std::sync::Arc<Workspace>>,
    expressions: Option<std::sync::Arc<BTreeMap<String, Expr>>>,
}
/// One level of the environment a name is resolved against. The stack is one
/// list rather than one per kind, but the order a name is searched in is not
/// the order frames were pushed: a call's locals answer first, then the
/// bindings a host supplied, then the row a `sum` is walking, then the
/// workspace.
pub(super) enum Frame {
    /// A call in progress: its arguments by parameter index, and the function
    /// itself, which names those parameters and carries what it captured.
    Function {
        function: std::sync::Arc<crate::evaluate::functional::Function>,
        arguments: Vec<Value>,
    },
    /// One table row, while a `sum` row expression runs.
    Row(RowScope),
    /// Names a host resolves lazily: a query's row fields, a module's context.
    /// Taken out of the frame while it answers, so resolving a binding cannot
    /// see itself.
    Bindings(Option<std::sync::Arc<dyn Bindings>>),
    /// A named definition, a call, or a dynamic `eval`: nothing below is in
    /// scope. A row survives an `eval`, which still runs inside its row.
    Barrier { rows: bool },
}
/// Where each kind of frame is found, or `None` when a barrier hides it.
#[derive(Default)]
struct Scope {
    function: Option<usize>,
    bindings: Option<usize>,
    row: Option<usize>,
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
            features: request.link_features(),
            memo: request.memo.clone(),
            frames: Vec::new(),
            budget: Budget::default(),
            trace: Trace::default(),
            failure: None,
            wanted: Vec::new(),
            time_dependent: false,
            row_variables: Vec::new(),
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
            links: self.features,
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
        self.features
    }
    /// Override the registry for an embedded host or test before evaluation begins.
    pub fn with_link_features(mut self, features: crate::link_features::LinkFeatures<'a>) -> Self {
        self.features = features;
        self.memo = Default::default();
        self
    }
    pub(crate) fn bound_expr(
        &mut self,
        path: &Path,
        expr: &Expr,
        bindings: std::sync::Arc<dyn Bindings>,
    ) -> EvalResult<Value> {
        let height = self.frames.len();
        self.frames.push(Frame::Bindings(Some(bindings)));
        let result = self.expr(path, expr).map(Value::plain);
        self.frames.truncate(height);
        result
    }
    /// The innermost frame of each kind that is still in scope.
    fn scope(&self) -> Scope {
        let mut scope = Scope::default();
        let (mut locals, mut bindings, mut rows) = (true, true, true);
        for (at, frame) in self.frames.iter().enumerate().rev() {
            match frame {
                Frame::Function { .. } if locals => scope.function = scope.function.or(Some(at)),
                Frame::Bindings(_) if bindings => scope.bindings = scope.bindings.or(Some(at)),
                Frame::Row(_) if rows => scope.row = scope.row.or(Some(at)),
                Frame::Barrier { rows: keep } => {
                    (locals, bindings) = (false, false);
                    rows &= *keep;
                    if !rows {
                        break;
                    }
                }
                _ => (),
            }
        }
        scope
    }
    /// A call's locals: its parameters first, then what it captured.
    pub(super) fn local(&self, name: &str) -> Option<Value> {
        let Some(Frame::Function {
            function,
            arguments,
        }) = self.scope().function.map(|at| &self.frames[at])
        else {
            return None;
        };
        function
            .params
            .iter()
            .position(|p| p == name)
            .and_then(|i| arguments.get(i).cloned())
            .or_else(|| function.captured.get(name).cloned())
    }
    /// Those locals as the name-to-value map a closure captures.
    fn captured(&self) -> Option<BTreeMap<String, Value>> {
        let Some(Frame::Function {
            function,
            arguments,
        }) = self.scope().function.map(|at| &self.frames[at])
        else {
            return None;
        };
        let mut locals = function.captured.clone();
        locals.extend(
            function
                .params
                .iter()
                .cloned()
                .zip(arguments.iter().cloned()),
        );
        Some(locals)
    }
    pub(super) fn row(&self) -> Option<&RowScope> {
        match self.scope().row.map(|at| &self.frames[at]) {
            Some(Frame::Row(scope)) => Some(scope),
            _ => None,
        }
    }
    pub(super) fn push_row(&mut self, scope: RowScope) {
        self.frames.push(Frame::Row(scope));
    }
    pub(super) fn pop_row(&mut self) {
        self.frames.pop();
    }
    /// Hide the frames below, and answer with the height to restore.
    pub(super) fn barrier(&mut self, rows: bool) -> usize {
        let height = self.frames.len();
        self.frames.push(Frame::Barrier { rows });
        height
    }
    pub(super) fn unwind(&mut self, height: usize) {
        self.frames.truncate(height);
    }
    pub(super) fn push_call(
        &mut self,
        function: std::sync::Arc<crate::evaluate::functional::Function>,
        arguments: Vec<Value>,
    ) {
        self.frames.push(Frame::Function {
            function,
            arguments,
        });
    }
    pub(super) fn binding(&mut self, name: &str) -> Option<EvalResult<Value>> {
        let at = self.scope().bindings?;
        let Frame::Bindings(slot) = &mut self.frames[at] else {
            return None;
        };
        let bindings = slot.take()?;
        let result = bindings.get(name, self);
        self.frames[at] = Frame::Bindings(Some(bindings));
        result
    }
    pub fn eval(&mut self, path: &Path, expression: &str) -> EvalResult<Value> {
        self.eval_at(path, expression, Span::new(0, 0, expression.len()))
    }
    pub fn eval_at(&mut self, path: &Path, expression: &str, span: Span) -> EvalResult<Value> {
        if self.idle() {
            self.budget.steps = 0;
        }
        self.trace.contexts.push((path.into(), span));
        let parsed = self
            .expressions
            .as_ref()
            .and_then(|m| m.get(expression))
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| Parser::parse(expression));
        let result = match parsed {
            // A code never outlives the expression it was written in.
            Ok(expr) => self.expr(path, &expr).map(Value::plain),
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
        self.trace.contexts.pop();
        result
    }
    fn fail(&mut self, bounds: (usize, usize), message: &EvalError) {
        if self.failure.is_none()
            && let Some((path, base)) = self.trace.contexts.last()
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
    /// Nothing is part-way through: the step budget starts again.
    fn idle(&self) -> bool {
        self.trace.contexts.is_empty() && self.trace.symbols.is_empty() && self.frames.is_empty()
            // A module evaluates on its own engine, which inherits the budget.
            && self.budget.calls == 0
    }
    pub fn named(&mut self, path: &Path, name: &str) -> EvalResult<Value> {
        let symbol = self.workspace.resolve(path, name)?;
        self.symbol(&symbol)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> EvalResult<Value> {
        if self.idle() {
            self.budget.steps = 0;
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
        if let Some(start) = self.trace.symbols.iter().position(|s| s == symbol) {
            let mut related = self.trace.symbols[start..].to_vec();
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
        if self.trace.symbols.len() >= 64 {
            return Err(EvalError::DepthExceeded(Depth::Dependency));
        }
        let previous_failure = self.failure.take();
        let previous_time = std::mem::replace(&mut self.time_dependent, false);
        let wanted_start = self.wanted.len();
        self.trace.symbols.push(symbol.clone());
        // Named definitions never capture a caller's frames.
        let height = self.barrier(false);
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
        self.unwind(height);
        self.trace.symbols.pop();
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
        self.budget.steps += 1;
        if self.budget.steps > 200_000 {
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
            Expr::Param { name, index } => self.param(path, name, *index),
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
    /// Every argument of a call or list, left to right. A code spells itself
    /// out here: only a lookup, which reads its arguments itself, wants one.
    pub(crate) fn values(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Vec<Value>> {
        args.iter()
            .map(|e| self.expr(path, e).map(Value::plain))
            .collect()
    }
    fn record(&mut self, path: &Path, fields: &[(String, Expr)]) -> EvalResult<Value> {
        let value = Value::Record(
            fields
                .iter()
                .map(|(k, e)| Ok((k.clone(), self.expr(path, e)?.plain())))
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
        let mut captured = self.row().map(|s| s.values.clone()).unwrap_or_default();
        if let Some(locals) = self.captured() {
            captured.extend(locals);
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
                source: self.trace.contexts.last().cloned(),
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
            _ => (),
        }
        if let Some(value) = self.local(n) {
            return Ok(value);
        }
        if let Some(value) = self.binding(n) {
            return value;
        }
        let Some(scope) = self.row() else {
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
    /// A lambda parameter, read straight out of the call's arguments. Outside
    /// its call — a body walked symbolically by a plan — the name still
    /// decides.
    fn param(&mut self, path: &Path, name: &str, index: usize) -> EvalResult<Value> {
        if let Some(Frame::Function { arguments, .. }) =
            self.scope().function.map(|at| &self.frames[at])
            && let Some(value) = arguments.get(index)
        {
            return Ok(value.clone());
        }
        self.name(path, name)
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
        self.time_dependent |= self.features.time_dependent(
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
        let value = self.features.property(
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
