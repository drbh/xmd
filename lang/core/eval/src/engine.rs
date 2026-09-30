//! The engine that turns a definition into a value: name resolution over the
//! environment frames, the request memo, and where a failure is placed. Calls
//! are in `calls`, the objects only the evaluator builds in `host`, and the
//! linear forms plans need in `linear`. The syntax it reads — the lexer, the
//! expression tree and the built-in vocabulary — lives in `syntax`, and the
//! value kinds and operators in `values`; both are re-exported here, so the
//! crate spells them `engine::lex`, `engine::Value` and so on.
pub(crate) use crate::linear::{Linear, RowVariable};
use crate::{
    plans::PlanValue,
    tables_impl::TableValue,
    timers::Timer,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset, NaiveDate};
pub(crate) use common::Currency;
use common::Span;
pub(crate) use common::ValueType;
use model::TaskState;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub(crate) use syntax::timer_arguments;
pub(crate) use syntax::{BinaryOp, Comparison, UnaryOp, relative_date};
pub(crate) use syntax::{Builtin, Expr, Lexeme, Parser, Tier, lex, sum_scope_at};
pub(crate) use values::binary;
use values::{Depth, EvalError, EvalResult, Overflow};
pub(crate) use values::{Function, Namespace, TaskKey, Unit, date_value};
pub(crate) use values::{Value, literal};
#[derive(Clone, Debug)]
pub struct EvalFailure {
    pub path: PathBuf,
    pub span: Span,
    pub message: EvalError,
    pub related: Vec<Symbol>,
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
    pub(crate) wanted: Vec<values::LookupKey>,
    pub(crate) time_dependent: bool,
}
/// Host-provided names are resolved lazily by the same evaluator as note functions.
/// Resolution runs without the caller's bindings, so definitions cannot capture them.
pub trait Bindings: Send + Sync {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<EvalResult<Value>>;
}

/// The expression nodes one evaluation may visit.
const STEP_LIMIT: usize = 200_000;

/// How much work one evaluation may still do. A module evaluates on an engine
/// of its own, which inherits the budget rather than being given a fresh one.
#[derive(Default)]
pub(crate) struct Budget {
    pub(crate) steps: usize,
    pub(crate) calls: usize,
}
/// What is being walked right now: enough to name a cycle, and to place a
/// failure in the note that asked for it.
#[derive(Default)]
pub(crate) struct Trace {
    /// Definitions on the value stack.
    symbols: Vec<Symbol>,
    /// Definitions being walked symbolically, separate from the value stack:
    /// a goal seek is legitimately on both at once.
    pub(crate) linear: Vec<Symbol>,
    /// The note and span each nested evaluation belongs to.
    pub(crate) contexts: Vec<(PathBuf, Span)>,
}
pub struct Engine<'a> {
    /// The workspace, clock snapshot, link registry and memo this evaluation
    /// shares with every other engine of the same request.
    pub(crate) request: crate::context::RequestContext<'a>,
    /// The dynamic environment a name is resolved against, innermost last.
    frames: Vec<Frame>,
    pub(crate) budget: Budget,
    pub(crate) trace: Trace,
    pub(crate) failure: Option<EvalFailure>,
    /// Lookup keys read during evaluation, hit or miss, for hovers and refresh.
    pub(crate) wanted: Vec<values::LookupKey>,
    pub(crate) time_dependent: bool,
    /// Decision-column variables met while linearizing a plan.
    pub(crate) row_variables: Vec<RowVariable>,
    /// A module's evaluation: immutable inputs only, its own workspace, and
    /// the expressions its note was parsed into. `module` is the plain answer
    /// to "am I running module code", which decides whether the module-tier
    /// built-ins are names at all.
    module: bool,
    environment: Option<std::sync::Arc<Workspace>>,
    pub(crate) expressions: Option<std::sync::Arc<BTreeMap<String, Expr>>>,
}
/// One level of the environment a name is resolved against. The stack is one
/// list rather than one per kind, but the order a name is searched in is not
/// the order frames were pushed: a call's locals answer first, then the
/// bindings a host supplied, then the row a `sum` is walking, then the
/// workspace.
pub(crate) enum Frame {
    /// A call in progress: its arguments by parameter index, and the function
    /// itself, which names those parameters and carries what it captured.
    Function {
        function: std::sync::Arc<Function>,
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
pub(crate) struct RowScope {
    pub(crate) table: String,
    pub(crate) values: BTreeMap<String, Value>,
    /// Decision columns, mapped to the per-row variable name a plan uses.
    pub(crate) decisions: BTreeMap<String, String>,
}
impl<'a> Engine<'a> {
    /// One clock snapshot per evaluation; injectable for deterministic tests.
    pub fn at(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        crate::context::RequestContext::new(workspace, now).engine()
    }
    pub(crate) fn in_request(request: &crate::context::RequestContext<'a>) -> Self {
        Self {
            request: request.clone(),
            frames: Vec::new(),
            budget: Budget::default(),
            trace: Trace::default(),
            failure: None,
            wanted: Vec::new(),
            time_dependent: false,
            row_variables: Vec::new(),
            module: false,
            environment: None,
            expressions: None,
        }
    }
    /// Share immutable inputs and memoized results with another feature session.
    pub fn request(&self) -> crate::context::RequestContext<'a> {
        self.request.clone()
    }
    pub fn workspace(&self) -> &'a Workspace {
        self.request.workspace
    }
    /// The one clock snapshot every date in this evaluation is read against.
    pub fn now(&self) -> DateTime<FixedOffset> {
        self.request.clock.now
    }
    pub fn today(&self) -> NaiveDate {
        self.request.clock.today()
    }
    /// Whether this evaluation has a clock to read: module code that native
    /// code calls at [`modules::no_clock`] has none.
    pub(crate) fn has_clock(&self) -> bool {
        !self.module || modules::has_clock(self.request.clock.now)
    }
    /// Where the last evaluation failed, when it did: the note and span to
    /// report it at, and the definitions it involved.
    pub fn failure(&self) -> Option<&EvalFailure> {
        self.failure.as_ref()
    }
    /// Forget a reported failure before evaluating the next, unrelated thing.
    pub fn clear_failure(&mut self) {
        self.failure = None;
    }
    /// The lookups evaluation read so far, cached or not.
    pub fn wanted(&self) -> &[values::LookupKey] {
        &self.wanted
    }
    /// Whether anything evaluated so far reads the clock.
    pub fn time_dependent(&self) -> bool {
        self.time_dependent
    }
    /// Record that a caller showed something that moves with the clock.
    pub fn mark_time_dependent(&mut self, dependent: bool) {
        self.time_dependent |= dependent;
    }
    pub fn date(&self, value: &Value) -> EvalResult<NaiveDate> {
        self.request
            .clock
            .date(value)
            .ok_or(EvalError::Expected("a date or appointment time"))
    }
    pub fn link_features(&self) -> modules::LinkFeatures<'a> {
        self.request.links
    }
    /// Override the registry for an embedded host or test before evaluation begins.
    pub fn with_link_features(mut self, features: modules::LinkFeatures<'a>) -> Self {
        self.request = self.request.with_link_features(features);
        self
    }
    pub fn bound_expr(
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
    pub(crate) fn local(&self, name: &str) -> Option<Value> {
        self.local_ref(name).cloned()
    }
    fn local_ref(&self, name: &str) -> Option<&Value> {
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
            .and_then(|i| arguments.get(i))
            .or_else(|| function.captured.get(name))
    }
    /// `a.b.c` where `a` is a local record: the value it ends at, read in
    /// place, with how many expression nodes evaluating it would take. Walking
    /// by reference clones only that value, where evaluating each step clones
    /// the whole record first — a hook's `ctx` holds the entire note. `None`
    /// for anything else, which [`Self::expr`] evaluates as usual.
    fn local_path(&self, expr: &Expr) -> Option<(&Value, usize)> {
        match expr {
            Expr::Spanned(_, _, inner) => self.local_path(inner).map(|(v, n)| (v, n + 1)),
            Expr::Param { index, .. } => match self.scope().function.map(|at| &self.frames[at]) {
                Some(Frame::Function { arguments, .. }) => Some((arguments.get(*index)?, 1)),
                _ => None,
            },
            Expr::Name(name) if keyword(name).is_none() => Some((self.local_ref(name)?, 1)),
            Expr::Property(record, key) => match self.local_path(record)? {
                (Value::Record(fields), n) => Some((fields.get(key)?, n + 1)),
                _ => None,
            },
            _ => None,
        }
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
    pub(crate) fn row(&self) -> Option<&RowScope> {
        match self.scope().row.map(|at| &self.frames[at]) {
            Some(Frame::Row(scope)) => Some(scope),
            _ => None,
        }
    }
    pub(crate) fn push_row(&mut self, scope: RowScope) {
        self.frames.push(Frame::Row(scope));
    }
    pub(crate) fn pop_row(&mut self) {
        self.frames.pop();
    }
    /// Hide the frames below, and answer with the height to restore.
    pub(crate) fn barrier(&mut self, rows: bool) -> usize {
        let height = self.frames.len();
        self.frames.push(Frame::Barrier { rows });
        height
    }
    pub(crate) fn unwind(&mut self, height: usize) {
        self.frames.truncate(height);
    }
    pub(crate) fn push_call(&mut self, function: std::sync::Arc<Function>, arguments: Vec<Value>) {
        self.frames.push(Frame::Function {
            function,
            arguments,
        });
    }
    pub(crate) fn binding(&mut self, name: &str) -> Option<EvalResult<Value>> {
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
        let parsed = self
            .expressions
            .as_ref()
            .and_then(|m| m.get(expression))
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| Parser::parse(expression));
        self.with_context(path, span, |engine| match parsed {
            // A code never outlives the expression it was written in.
            Ok(expr) => engine.expr(path, &expr).map(Value::plain),
            Err(message) => {
                let tokens = lex(expression).unwrap_or_default();
                let bounds = tokens
                    .last()
                    .map(|t| (t.start, t.end))
                    .unwrap_or((0, expression.len()));
                engine.refuse(bounds, EvalError::Message(message))
            }
        })
    }
    /// Run `f` as the evaluation of text at `span` in `path`, which is where
    /// its failures are placed.
    pub(crate) fn with_context<T>(
        &mut self,
        path: &Path,
        span: Span,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.trace.contexts.push((path.into(), span));
        let result = f(self);
        self.trace.contexts.pop();
        result
    }
    /// Record `message` at `bounds` and answer with it.
    pub(crate) fn refuse<T>(
        &mut self,
        bounds: (usize, usize),
        message: EvalError,
    ) -> EvalResult<T> {
        self.fail(bounds, &message);
        Err(message)
    }
    /// Place a failure at `bounds`, unless a narrower span already holds one.
    pub(crate) fn within<T>(
        &mut self,
        bounds: (usize, usize),
        result: EvalResult<T>,
    ) -> EvalResult<T> {
        if let Err(message) = &result {
            self.fail(bounds, message);
        }
        result
    }
    /// The memo every engine of this request shares.
    fn memo(&self) -> std::sync::MutexGuard<'_, BTreeMap<MemoKey, MemoEntry>> {
        self.request.memo.lock().expect("request cache poisoned")
    }
    /// Pin `error` to `span` unless an earlier failure already claimed the
    /// report, and hand it back for the caller to return.
    pub(crate) fn fail_at(&mut self, path: &Path, span: Span, error: EvalError) -> EvalError {
        self.failure.get_or_insert(EvalFailure {
            path: path.to_path_buf(),
            span,
            message: error.clone(),
            related: vec![],
        });
        error
    }
    pub(crate) fn fail(&mut self, bounds: (usize, usize), message: &EvalError) {
        if self.failure.is_none()
            && let Some((path, base)) = self.trace.contexts.last()
        {
            self.failure = Some(EvalFailure {
                path: path.clone(),
                span: self
                    .request
                    .workspace
                    .documents
                    .get(path)
                    .map(|doc| base.relative(doc, bounds.0, bounds.1))
                    .unwrap_or_else(|| {
                        Span::new(base.line, base.start + bounds.0, base.start + bounds.1)
                    }),
                message: message.clone(),
                related: vec![],
            });
        }
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
        let tokens = lex(source).map_err(EvalError::Message)?;
        let mut edits = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            if let Lexeme::Name(name) = &token.kind
                && !matches!(tokens.get(i + 1).map(|t| &t.kind), Some(Lexeme::Left))
                && (i == 0 || !matches!(tokens[i - 1].kind, Lexeme::Dot))
                && !matches!(keyword(name), Some(Value::Bool(_)))
                && sum_scope_at(source, token.start).is_none()
                && let Ok(value) = self.named(path, name)
            {
                if value.downcast::<TableValue>().is_some() {
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
        let symbol = self.request.workspace.resolve(path, name)?;
        self.symbol(&symbol)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> EvalResult<Value> {
        if self.idle() {
            self.budget.steps = 0;
        }
        let key = MemoKey::Symbol(symbol.clone());
        let cached = self.memo().get(&key).cloned();
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
                    .map(|s| self.request.workspace.named(s).name.clone())
                    .collect(),
            };
            self.failure = Some(EvalFailure {
                path: symbol.path.clone(),
                span: self.request.workspace.named(symbol).span,
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
        let doc = &self.request.workspace.documents[&symbol.path];
        let result = match symbol.kind {
            SymbolKind::Definition(i) => {
                let def = &doc.definitions[i];
                if let Some(plan) = doc.plan_of(i) {
                    crate::plans::solve(self, symbol, plan)
                } else if def.expression && model::plans::seek_body(&def.source).is_some() {
                    crate::plans::seek(self, symbol)
                } else if let Some(table) = doc.table_of(i) {
                    if let Some(problem) = table.problems.first() {
                        let message = EvalError::Message(problem.message.clone());
                        Err(self.fail_at(&symbol.path, problem.span, message))
                    } else {
                        crate::tables_impl::table_value(self, symbol, table)
                    }
                } else if def.expression {
                    self.eval_at(&symbol.path, &def.source, def.expression_span(&doc.text))
                        .map(|v| match v.downcast::<Timer>() {
                            Some(timer)
                                if timer.origin.is_none()
                                    && timer_arguments(&def.source).is_some() =>
                            {
                                let mut timer = timer.clone();
                                timer.origin = Some(symbol.clone());
                                Value::Host(std::sync::Arc::new(timer))
                            }
                            _ => v,
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
            SymbolKind::Section(i) => Ok(Value::Tasks(
                doc.section_tasks(i)
                    .map(|task| (symbol.path.clone(), task))
                    .collect(),
            )),
            SymbolKind::Column(_, _) => Err(EvalError::Message(
                "A column needs a row context, e.g. sum(table, column)".into(),
            )),
            SymbolKind::Variable(plan, name) => {
                let name = doc.plans[plan].names[name].name.clone();
                let definition = symbol.sibling(SymbolKind::Definition(doc.plans[plan].definition));
                self.symbol(&definition).and_then(|value| {
                    value
                        .downcast::<PlanValue>()
                        .ok_or(EvalError::Expected("a plan"))?
                        .property(&name)
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
        self.memo().insert(key, entry);
        self.failure = previous_failure.or(self.failure.take());
        self.time_dependent |= previous_time;
        result
    }
    /// One node, one dispatch: every kind of expression names the method that
    /// answers it.
    pub(crate) fn expr(&mut self, path: &Path, expr: &Expr) -> EvalResult<Value> {
        self.budget.steps += 1;
        if self.budget.steps > STEP_LIMIT {
            return self.refuse(expr.bounds(), EvalError::StepLimit);
        }
        match expr {
            Expr::Spanned(start, end, inner) => {
                let result = self.expr(path, inner);
                self.within((*start, *end), result)
            }
            Expr::Value(v) => Ok(Value::from(v.clone())),
            Expr::Code(c) => Ok(Value::Code(*c)),
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
                if let Some((value, nodes)) = self.local_path(expr)
                    && self.budget.steps + nodes - 1 <= STEP_LIMIT
                {
                    let value = value.clone();
                    self.budget.steps += nodes - 1;
                    return Ok(value);
                }
                let value = self.expr(path, v)?;
                let result = self.access(path, value, key);
                // `import("id").name` names a member the library keeps
                // private, or has no such member at all: either way, the
                // module is the thing to blame, not an anonymous record.
                match (result, v.bare()) {
                    (Err(EvalError::UnknownField { .. }), Expr::Builtin(Builtin::Import, args))
                        if let [arg] = args.as_slice()
                            && let Expr::Value(syntax::Literal::Text(id)) = arg.bare() =>
                    {
                        Err(EvalError::NotExported {
                            id: id.clone(),
                            name: key.clone(),
                        })
                    }
                    (result, _) => result,
                }
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
        Ok(Value::Function(std::sync::Arc::new(Function {
            environment: self
                .environment
                .clone()
                .map(|workspace| workspace as std::sync::Arc<dyn std::any::Any + Send + Sync>),
            expressions: self.expressions.clone(),
            params: params.to_vec(),
            body: body.clone(),
            path: path.into(),
            source: self.trace.contexts.last().cloned(),
            captured,
        })))
    }
    /// A bare name: the literals it may spell, then locals, bindings, the row
    /// scope, and finally the workspace.
    fn name(&mut self, path: &Path, n: &str) -> EvalResult<Value> {
        if let Some(value) = keyword(n) {
            return Ok(value);
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
            if self.module {
                return Err(EvalError::Message(
                    "Module evaluation cannot access the filesystem".into(),
                ));
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                return Err(EvalError::Message(
                    "Local file existence is unavailable in the browser".into(),
                ));
            }
            #[cfg(not(target_arch = "wasm32"))]
            return Ok(Value::Bool(
                resource
                    .url(path)?
                    .to_file_path()
                    .map(|p| p.exists())
                    .unwrap_or(false),
            ));
        }
        self.time_dependent |= self.request.links.time_dependent(
            &resource.target,
            &self.request.workspace.cache,
            self.request.clock.now.to_utc(),
        );
        let memo_key = MemoKey::ResourceProperty(resource.target.clone(), key.into());
        if let Some(entry) = self.memo().get(&memo_key).cloned() {
            return entry.value;
        }
        let value = self.request.links.property(
            &resource.target,
            &self.request.workspace.cache,
            self.request.clock.now.to_utc(),
            key,
        );
        self.memo().insert(
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
            Value::Namespace(path) => self.named(path.path(), key),
            Value::List(items) => items
                .iter()
                .map(|v| self.property(v, key))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List),
            _ => value.property(key),
        }
    }
    /// A module's own evaluation: immutable inputs only, including resource
    /// properties, no link features, and the module-tier built-ins answer.
    pub(crate) fn for_module(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        let mut engine =
            Self::at(workspace, now).with_link_features(modules::LinkFeatures::default());
        engine.module = true;
        engine
    }
    /// Whether module code is running: a module engine, or a buffer that lives
    /// where modules live, so an author still sees their own file evaluate.
    pub(crate) fn module_code(&self, path: &Path) -> bool {
        self.module || modules::is_module_path(path)
    }
    pub(crate) fn with_expressions(
        mut self,
        expressions: std::sync::Arc<BTreeMap<String, Expr>>,
    ) -> Self {
        self.expressions = Some(expressions);
        self
    }
    /// Share `memo` rather than the request's own.
    pub(crate) fn with_memo(mut self, memo: crate::context::Memo) -> Self {
        self.request.memo = memo;
        self
    }
    pub(crate) fn with_environment(mut self, environment: std::sync::Arc<Workspace>) -> Self {
        self.environment = Some(environment);
        self
    }
    pub fn task_done(&self, path: &Path, i: usize) -> bool {
        let doc = &self.request.workspace.documents[path];
        let children: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent == Some(i))
            .map(|(j, _)| j)
            .collect();
        if children.is_empty() {
            doc.tasks[i].state == TaskState::Done
        } else {
            children.into_iter().all(|j| self.task_done(path, j))
        }
    }
    /// Started but not finished: marked `[-]`, or a parent with some subtasks
    /// done or in progress and others still open.
    pub fn task_in_progress(&self, path: &Path, i: usize) -> bool {
        if self.task_done(path, i) {
            return false;
        }
        let doc = &self.request.workspace.documents[path];
        doc.tasks[i].state == TaskState::InProgress
            || doc.tasks.iter().enumerate().any(|(j, t)| {
                t.parent == Some(i) && (self.task_done(path, j) || self.task_in_progress(path, j))
            })
    }

    /// What each row adds to a `sum(table, row expression)`, or `None` when
    /// `source` is not one or does not evaluate.
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
                .filter(|(p, index)| {
                    self.request.workspace.documents[p].tasks[*index]
                        .named
                        .is_some()
                })
                .map(|(p, index)| Symbol::new(p.clone(), SymbolKind::Task(*index)))
                .collect();
            let message = EvalError::TaskCycle {
                names: related
                    .iter()
                    .map(|s| self.request.workspace.named(s).name.clone())
                    .collect(),
            };
            let task = &self.request.workspace.documents[path].tasks[i];
            self.failure = Some(EvalFailure {
                path: path.into(),
                span: task
                    .attributes
                    .get(syntax::AttributeKey::After.as_str())
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
        let task = &self.request.workspace.documents[path].tasks[i];
        let mut blocked = Vec::new();
        if let Some(attr) = task.attributes.get(syntax::AttributeKey::After.as_str()) {
            for name in attr.value.split(',').map(str::trim) {
                if let Ok(s) = self.request.workspace.resolve(path, name)
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
        if let Some(v) = relative_date(source, self.request.clock.today()) {
            return Ok(Value::Date(v));
        }
        let value = self.eval(path, source)?;
        self.date(&value)?;
        Ok(value)
    }
}

/// The value a bare name spells before any definition can answer for it.
pub(crate) fn keyword(name: &str) -> Option<Value> {
    match name {
        "null" => Some(Value::Null),
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        _ => None,
    }
}
/// Values are capped wherever one is built, not only where one is returned.
pub(crate) fn sized(value: Value) -> EvalResult<Value> {
    values::check_size(&value)?;
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
