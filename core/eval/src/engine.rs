//! The engine that turns a definition into a value: name resolution over the
//! environment frames, the request memo, and where a failure is placed. Calls
//! are in `calls`, the objects only the evaluator builds in `host`, and the
//! linear forms a form's expressions are read as in `linear`; a checklist
//! item's state in `checklists`, an attribute's value in `attributes`, and the
//! worked step a hover shows in `substitution`. A definition that is a feature
//! (a form a module declares, a table) is evaluated by the feature `features`
//! registers for its kind, never named here. The syntax it reads — the lexer,
//! the expression tree and the built-in vocabulary — lives in `syntax`, and
//! the value kinds and operators in `values`; both are re-exported here, so
//! the crate spells them `engine::lex`, `engine::Value` and so on.
pub(crate) use crate::linear::{Linear, RowVariable};
use crate::memo::{CALL_LIMIT, DEPTH_LIMIT, Found, MemoEntry, MemoKey, STEP_LIMIT, Start, Walk};
use crate::workspace::{Symbol, SymbolKind, Workspace};
use crate::{features, lookups::LookupRead};
use chrono::{DateTime, FixedOffset, NaiveDate};
pub(crate) use common::Currency;
use common::Span;
pub(crate) use common::ValueType;
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};
pub(crate) use syntax::{BinaryOp, Comparison, UnaryOp, relative_date};
pub(crate) use syntax::{Builtin, Expr, Lexeme, Parser, Tier, lex, sum_scope_at};
pub(crate) use values::binary;
use values::{Depth, EvalError, EvalResult, Overflow};
pub(crate) use values::{Function, Namespace, Unit, date_value};
pub(crate) use values::{Value, literal};
#[derive(Clone, Debug)]
pub struct EvalFailure {
    pub path: PathBuf,
    pub span: Span,
    pub message: EvalError,
    pub related: Vec<Symbol>,
}
impl EvalFailure {
    fn new(path: &Path, span: Span, message: EvalError, related: Vec<Symbol>) -> Self {
        let path = path.into();
        Self {
            path,
            span,
            message,
            related,
        }
    }
}
/// Host-provided names are resolved lazily by the same evaluator as note functions.
/// Resolution runs without the caller's bindings, so definitions cannot capture them.
pub trait Bindings: Send + Sync {
    fn get(&self, name: &str, engine: &mut Engine<'_>) -> Option<EvalResult<Value>>;
}

/// How much work one evaluation may still do. A module evaluates on an engine
/// of its own, which inherits the budget rather than being given a fresh one.
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    pub(crate) steps: usize,
    pub(crate) calls: usize,
    /// The most steps it may take, and how big a value it may build.
    pub(crate) step_limit: usize,
    pub(crate) size_limit: values::Size,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            steps: 0,
            calls: 0,
            step_limit: STEP_LIMIT,
            size_limit: values::Size::LIMIT,
        }
    }
}
impl Budget {
    /// A hook's budget grows with what the host hands it: a few times as
    /// big a value as its input, and steps in proportion to it, so a module
    /// that builds a large note's records is not refused for the note's
    /// size. Never less than any evaluation's.
    pub(crate) fn scaled(input: values::Size) -> Self {
        let default = Self::default();
        Self {
            step_limit: default.step_limit.max(input.items.saturating_mul(64)),
            size_limit: values::Size {
                items: default.size_limit.items.max(input.items.saturating_mul(4)),
                bytes: default.size_limit.bytes.max(input.bytes.saturating_mul(4)),
            },
            ..default
        }
    }
}
/// What is being walked right now: enough to name a cycle, and to place a
/// failure in the note that asked for it.
#[derive(Default)]
pub(crate) struct Trace {
    /// Definitions on the value stack.
    symbols: Vec<Symbol>,
    /// Definitions being walked symbolically, separate from the value stack:
    /// a form solving for its own name is legitimately on both at once.
    pub(crate) linear: Vec<Symbol>,
    /// The note and span each nested evaluation belongs to.
    pub(crate) contexts: Vec<(Arc<Path>, Span)>,
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
    /// The lookup cache `cached` reads: the request's workspace's, which a
    /// module engine running on the request's behalf shares.
    pub(crate) lookups: Arc<values::Store>,
    /// Lookup keys read during evaluation, hit or miss, for hovers and
    /// refresh, each with the note text it was read for.
    pub(crate) wanted: Vec<LookupRead>,
    /// In a module engine, the note text whose evaluation called into the
    /// module: what a lookup the module reads is read for.
    pub(crate) reader: Option<(Arc<Path>, Span)>,
    pub(crate) time_dependent: bool,
    /// Decision-column variables met while reading a form's linear forms.
    pub(crate) row_variables: Vec<RowVariable>,
    /// What the module of the form definition just evaluated said about it
    /// besides its value, which the memo keeps with the value.
    pub(crate) about: Option<crate::forms::About>,
    /// The definition being evaluated, and what it has spent and read so far:
    /// recorded with its result, so a later request can charge itself for it.
    pub(crate) walk: Option<Walk>,
    /// A module's evaluation: immutable inputs only, its own workspace, and
    /// the expressions its note was parsed into. `module` is the plain answer
    /// to "am I running module code", which decides whether the module-tier
    /// built-ins are names at all.
    module: bool,
    environment: Option<Arc<Workspace>>,
    pub(crate) expressions: Option<Arc<BTreeMap<String, Expr>>>,
}
/// One level of the environment a name is resolved against. The stack is one
/// list rather than one per kind, but the order a name is searched in is not
/// the order frames were pushed: a call's locals answer first, then the
/// bindings a host supplied, then the row a `sum` is walking, then the
/// workspace.
pub(crate) enum Frame {
    /// A call in progress: the function, which names its parameters and
    /// carries what it captured, and its arguments by parameter index.
    Function(Arc<Function>, Vec<Value>),
    /// One table row, while a `sum` row expression runs.
    Row(RowScope),
    /// Names a host resolves lazily: a query's row fields, a module's context.
    /// Taken out of the frame while it answers, so resolving a binding cannot
    /// see itself.
    Bindings(Option<Arc<dyn Bindings>>),
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
    /// Decision columns, mapped to the per-row unknown a linear reading
    /// solves for.
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
            lookups: request.workspace.lookups.clone(),
            wanted: Vec::new(),
            reader: None,
            time_dependent: false,
            row_variables: Vec::new(),
            about: None,
            walk: None,
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
    pub fn wanted(&self) -> impl Iterator<Item = &values::LookupKey> {
        self.wanted.iter().map(|read| &read.key)
    }
    /// The same, each with the note text it was read for.
    pub fn reads(&self) -> &[LookupRead] {
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
    pub(crate) fn date(&self, value: &Value) -> EvalResult<NaiveDate> {
        self.request
            .clock
            .date(value)
            .ok_or(EvalError::Expected("a date or appointment time"))
    }
    pub fn link_features(&self) -> modules::LinkFeatures<'a> {
        self.request.links
    }
    pub fn bound_expr(
        &mut self,
        path: &Path,
        expr: &Expr,
        bindings: Arc<dyn Bindings>,
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
                Frame::Function(..) if locals => scope.function = scope.function.or(Some(at)),
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
    /// The call in scope, if any: the function and its arguments by
    /// parameter index.
    fn call_frame(&self) -> Option<(&Arc<Function>, &[Value])> {
        match &self.frames[self.scope().function?] {
            Frame::Function(function, arguments) => Some((function, arguments)),
            _ => None,
        }
    }
    /// A call's locals: its parameters first, then what it captured.
    pub(crate) fn local(&self, name: &str) -> Option<Value> {
        self.local_ref(name).cloned()
    }
    fn local_ref(&self, name: &str) -> Option<&Value> {
        let (function, arguments) = self.call_frame()?;
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
            Expr::Param { index, .. } => Some((self.call_frame()?.1.get(*index)?, 1)),
            Expr::Name(name) if keyword(name).is_none() => Some((self.local_ref(name)?, 1)),
            Expr::Property(record, key) => match self.local_path(record)? {
                (Value::Record(fields), n) => Some((fields.get(key)?, n + 1)),
                _ => None,
            },
            _ => None,
        }
    }
    /// Those locals as a closure captures them: the call's parameters in a
    /// scope inside what the function itself captured.
    fn captured(&self) -> Option<values::Captured> {
        let (function, arguments) = self.call_frame()?;
        let params = function.params.iter().cloned();
        let names = params.zip(arguments.iter().cloned()).collect();
        Some(function.captured.with(names))
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
    /// Bind the next parameter of the call in progress.
    pub(crate) fn push_argument(&mut self, value: Value) {
        if let Some(Frame::Function(_, arguments)) = self.frames.last_mut() {
            arguments.push(value);
        }
    }
    pub(crate) fn push_call(&mut self, function: Arc<Function>, arguments: Vec<Value>) {
        self.frames.push(Frame::Function(function, arguments));
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
        self.start();
        let parsed = match self.expressions.as_ref().and_then(|m| m.get(expression)) {
            Some(expr) => Ok(expr.clone()),
            None => Parser::parse(expression),
        };
        self.with_context(path, span, |engine| match parsed {
            // A code never outlives the expression it was written in.
            Ok(expr) => engine.expr(path, &expr).map(Value::plain),
            Err(message) => {
                let tokens = lex(expression).unwrap_or_default();
                let bounds = tokens
                    .last()
                    .map_or((0, expression.len()), |t| (t.start, t.end));
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
        self.trace.contexts.push((Arc::from(path), span));
        let result = f(self);
        self.trace.contexts.pop();
        result
    }
    /// Record `error` at `bounds` and answer with it.
    pub(crate) fn refuse<T>(&mut self, bounds: (usize, usize), error: EvalError) -> EvalResult<T> {
        self.fail(bounds, &error);
        Err(error)
    }
    /// Place a failure at `at`, unless a narrower span already holds one.
    pub(crate) fn within<T>(&mut self, at: (usize, usize), result: EvalResult<T>) -> EvalResult<T> {
        if let Err(message) = &result {
            self.fail(at, message);
        }
        result
    }
    /// The result being made depends on where it is evaluated from: see
    /// [`crate::memo`].
    pub(crate) fn contextual(&mut self) {
        if let Some(walk) = &mut self.walk {
            walk.contextual = true;
        }
    }
    /// A call is about to check how many calls are in progress.
    pub(crate) fn call_check(&mut self) -> EvalResult<()> {
        if let Some(walk) = &mut self.walk {
            walk.call_check(self.budget.calls);
        }
        if self.budget.calls >= CALL_LIMIT {
            self.contextual();
            return Err(EvalError::DepthExceeded(Depth::Call));
        }
        Ok(())
    }
    /// Pin `error` to `span` unless an earlier failure already claimed the
    /// report, and hand it back for the caller to return.
    pub(crate) fn fail_at(&mut self, path: &Path, span: Span, error: EvalError) -> EvalError {
        let failure = EvalFailure::new(path, span, error.clone(), vec![]);
        self.failure.get_or_insert(failure);
        error
    }
    pub(crate) fn fail(&mut self, bounds: (usize, usize), message: &EvalError) {
        if self.failure.is_some() {
            return;
        }
        // Placed at text the definition being evaluated did not push: its
        // caller's, which another caller would not share.
        let contexts = self.trace.contexts.len();
        if let Some(walk) = &mut self.walk
            && walk.contexts_base.is_some_and(|base| contexts <= base)
        {
            walk.contextual = true;
        }
        if let Some((path, base)) = self.trace.contexts.last() {
            let span = match self.request.workspace.documents.get(&**path) {
                Some(doc) => base.relative(doc, bounds.0, bounds.1),
                None => Span::new(base.line, base.start + bounds.0, base.start + bounds.1),
            };
            self.failure = Some(EvalFailure::new(path, span, message.clone(), vec![]));
        }
    }
    /// Nothing is part-way through: the step budget starts again. A module
    /// evaluates on its own engine, which inherits the budget, so there only
    /// when no call is in progress either. Where the definition being
    /// evaluated reaches such a point is part of what it costs.
    fn start(&mut self) {
        if !(self.trace.contexts.is_empty()
            && self.trace.symbols.is_empty()
            && self.frames.is_empty())
        {
            return;
        }
        if let Some(walk) = &mut self.walk {
            walk.quiet(self.budget.steps, self.budget.calls);
        }
        if self.budget.calls == 0 {
            self.budget.steps = 0;
        }
        if let Some(walk) = &mut self.walk {
            walk.resume(self.budget.steps);
        }
    }
    pub fn named(&mut self, path: &Path, name: &str) -> EvalResult<Value> {
        let symbol = self.request.workspace.resolve_shared(path, name)?;
        self.shared_symbol(symbol)
    }
    /// A name as code reads it: what the note or module at `path` defines,
    /// else what the prelude exports under that name.
    pub(crate) fn resolved(&mut self, path: &Path, name: &str) -> EvalResult<Value> {
        if self.request.workspace.candidates(path, name).is_empty()
            && let Some(value) = self.prelude(path, name)
        {
            return value;
        }
        self.named(path, name)
    }
    pub fn symbol(&mut self, symbol: &Symbol) -> EvalResult<Value> {
        self.shared_symbol(Arc::new(symbol.clone()))
    }
    fn shared_symbol(&mut self, symbol: Arc<Symbol>) -> EvalResult<Value> {
        self.start();
        let key = MemoKey::Symbol(symbol.clone());
        if let Some(walk) = &mut self.walk {
            walk.reach(key.clone(), self.budget.steps, self.budget.calls);
        }
        let result = self.reached(&symbol, key);
        if let Some(walk) = &mut self.walk {
            walk.resume(self.budget.steps);
        }
        result
    }
    /// A definition's value: from the memo when this request may read it
    /// there, evaluated otherwise.
    fn reached(&mut self, symbol: &Symbol, key: MemoKey) -> EvalResult<Value> {
        match self.request.memo.find(&key) {
            Some(Found::Reached(entry)) => return self.reuse(&entry),
            // Symbolic walks see the definitions they are inside, which an
            // earlier request's result never saw.
            Some(Found::Earlier(entry)) if self.trace.linear.is_empty() => {
                let start = Start {
                    steps: self.budget.steps,
                    step_limit: self.budget.step_limit,
                    calls: self.budget.calls,
                    stack: &self.trace.symbols,
                };
                if let Some(steps) = self.request.memo.replay(&key, start) {
                    self.budget.steps = steps;
                    return self.reuse(&entry);
                }
            }
            _ => {}
        }
        if let Some(start) = self.trace.symbols.iter().position(|s| s == symbol) {
            let mut related = self.trace.symbols[start..].to_vec();
            related.push(symbol.clone());
            let span = self.request.workspace.named(symbol).span;
            return self.cycle(&symbol.path, span, related, |names| EvalError::Cycle {
                names,
            });
        }
        if self.trace.symbols.len() >= DEPTH_LIMIT {
            self.contextual();
            return Err(EvalError::DepthExceeded(Depth::Dependency));
        }
        let previous_failure = self.failure.take();
        let previous_time = std::mem::replace(&mut self.time_dependent, false);
        let wanted_start = self.wanted.len();
        let (steps, calls) = (self.budget.steps, self.budget.calls);
        let caller = self
            .walk
            .replace(Walk::new(steps, calls, self.trace.contexts.len()));
        self.trace.symbols.push(symbol.clone());
        // Named definitions never capture a caller's frames.
        let height = self.barrier(false);
        self.about = None;
        let result = self.evaluate(symbol);
        let about = self.about.take().filter(|_| result.is_ok());
        self.unwind(height);
        self.trace.symbols.pop();
        let walk = std::mem::replace(&mut self.walk, caller).expect("pushed above");
        let (cost, contextual) = walk.finish(self.budget.steps);
        if contextual {
            self.contextual();
        }
        let entry = MemoEntry {
            value: result.clone(),
            failure: self.failure.clone().filter(|_| result.is_err()),
            wanted: self.wanted[wanted_start..].to_vec(),
            time_dependent: self.time_dependent,
            contextual,
            cost,
            about,
        };
        self.request.memo.keep(key, entry);
        self.failure = previous_failure.or(self.failure.take());
        self.time_dependent |= previous_time;
        result
    }
    /// Fail with the cycle through `related`, reported at `span` in `path`
    /// and naming each definition it passes.
    pub(crate) fn cycle<T>(
        &mut self,
        path: &Path,
        span: Span,
        related: Vec<Symbol>,
        cycle: fn(Vec<String>) -> EvalError,
    ) -> EvalResult<T> {
        let ws = self.request.workspace;
        let message = cycle(related.iter().map(|s| ws.named(s).name.clone()).collect());
        self.failure = Some(EvalFailure::new(path, span, message.clone(), related));
        self.contextual();
        Err(message)
    }
    /// Read a result the memo holds as evaluating it would have: its failure,
    /// lookups, clock reading and context.
    fn reuse(&mut self, entry: &MemoEntry) -> EvalResult<Value> {
        if entry.value.is_err() && self.failure.is_none() {
            self.failure = entry.failure.clone();
        }
        self.wanted.extend(entry.wanted.iter().cloned());
        self.time_dependent |= entry.time_dependent;
        if entry.contextual {
            self.contextual();
        }
        entry.value.clone()
    }
    /// Evaluate a definition afresh.
    fn evaluate(&mut self, symbol: &Symbol) -> EvalResult<Value> {
        let doc = &self.request.workspace.documents[&symbol.path];
        match symbol.kind {
            SymbolKind::Definition(i) => {
                let def = &doc.definitions()[i];
                let kind = doc.definition_kind(i);
                if let Some(evaluate) = features::evaluator(kind) {
                    evaluate(self, symbol, i)
                } else if kind == document::DefinitionKind::Expression {
                    self.eval_at(&symbol.path, &def.source, def.expression_span(doc))
                        .map(|v| features::adopt(v, symbol, doc))
                } else {
                    literal(&def.source).map(|mut v| {
                        if let Value::Resource(r) = &mut v {
                            r.origin = Some(symbol.path.clone());
                        }
                        v
                    })
                }
            }
            SymbolKind::Task(i) => Ok(Value::Bool(self.task_done(&symbol.path, i))),
            SymbolKind::Section(i) => Ok(Value::Tasks(
                doc.section_tasks(i)
                    .map(|task| (symbol.path.clone(), task))
                    .collect(),
            )),
            SymbolKind::Column(_, _) => {
                Err("A column needs a row context, e.g. sum(table, column)".into())
            }
            // A name a form solves for: that field of the value its
            // definition evaluates to.
            SymbolKind::Variable(form, name) => {
                let formed = &doc.forms()[form];
                let name = formed.names[name].name.clone();
                let definition = symbol.sibling(SymbolKind::Definition(formed.definition));
                self.symbol(&definition)
                    .and_then(|value| self.property(&value, &name))
            }
        }
    }
    /// One node, one dispatch: every kind of expression names the method that
    /// answers it.
    pub(crate) fn expr(&mut self, path: &Path, expr: &Expr) -> EvalResult<Value> {
        self.step(expr)?;
        match expr {
            // A span is a node of its own, counted and checked as one, but
            // stepped through here rather than by a call of its own: most
            // nodes have one.
            Expr::Spanned(start, end, inner) => {
                let result = match &**inner {
                    Expr::Spanned(..) => self.expr(path, inner),
                    node => self.step(node).and_then(|()| self.node(path, node)),
                };
                self.within((*start, *end), result)
            }
            node => self.node(path, node),
        }
    }
    /// Count one node against the step budget.
    fn step(&mut self, node: &Expr) -> EvalResult<()> {
        self.budget.steps += 1;
        if self.budget.steps > self.budget.step_limit {
            self.contextual();
            return self.refuse(node.bounds(), EvalError::StepLimit);
        }
        Ok(())
    }
    /// One node that is not a span, its step already counted.
    fn node(&mut self, path: &Path, expr: &Expr) -> EvalResult<Value> {
        match expr {
            Expr::Spanned(..) => unreachable!("expr steps through spans"),
            Expr::Value(v) => Ok(Value::from(v.clone())),
            Expr::Code(c) => Ok(Value::Code(*c)),
            Expr::Name(n) => self.name(path, n),
            Expr::Param { name, index } => self.param(path, name, *index),
            Expr::Builtin(builtin, args) => self.builtin(path, *builtin, args),
            Expr::Call(n, args) => self.call_named(path, n, args),
            Expr::Lambda(params, defaults, body) => self.lambda(path, expr, params, defaults, body),
            Expr::Apply(function, args) => {
                let function = self.expr(path, function)?;
                let args = self.values(path, args)?;
                self.call(function, args)
            }
            Expr::List(items) => {
                let value = Value::list(self.values(path, items)?);
                self.sized(value)
            }
            Expr::Record(fields) => self.record(path, fields),
            Expr::Unary(op, v) => {
                let v = self.expr(path, v)?;
                unary(*op, v)
            }
            Expr::Binary(op, a, b) => self.binary_expr(path, *op, a, b),
            Expr::Property(v, key) => {
                if let Some((value, nodes)) = self.local_path(expr)
                    && self.budget.steps + nodes - 1 <= self.budget.step_limit
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
        let value = Value::record(
            fields
                .iter()
                .map(|(k, e)| Ok((k.clone(), self.expr(path, e)?.plain())))
                .collect::<EvalResult<BTreeMap<_, _>>>()?,
        );
        self.sized(value)
    }
    /// Values are capped wherever one is built, not only where one is
    /// returned.
    pub(crate) fn sized(&mut self, value: Value) -> EvalResult<Value> {
        self.budget.size_limit.check(&value).map(|()| value)
    }
    /// A lambda captures the names it reads: the enclosing row scope and
    /// locals, then whatever bindings answer for its remaining free names.
    fn lambda(
        &mut self,
        path: &Path,
        expr: &Expr,
        params: &[String],
        defaults: &[Expr],
        body: &Arc<Expr>,
    ) -> EvalResult<Value> {
        let locals = self.captured().unwrap_or_default();
        // A row's columns sit below the locals, which win over them.
        let mut captured = match self.row() {
            Some(scope) => {
                let mut names = scope.values.clone();
                names.extend(locals.names());
                values::Captured::of(names)
            }
            None => locals,
        };
        // Walking the body for its free names is only worth it when some
        // bindings are in scope to answer them.
        if self.scope().bindings.is_some() {
            let mut bound = Vec::new();
            for (name, _) in expr.free_names() {
                if captured.get(&name).is_none()
                    && !bound.iter().any(|(n, _)| *n == name)
                    && let Some(value) = self.binding(&name)
                {
                    bound.push((name, value?));
                }
            }
            captured = captured.with(bound);
        }
        Ok(Value::Function(Arc::new(Function {
            environment: self.environment.clone().map(|workspace| workspace as _),
            expressions: self.expressions.clone(),
            params: params.to_vec(),
            defaults: defaults.to_vec(),
            body: body.clone(),
            // The note the enclosing call runs in, shared, when it is this one.
            path: match self.call_frame() {
                Some((function, _)) if *function.path == *path => function.path.clone(),
                _ => Arc::from(path),
            },
            source: self.trace.contexts.last().cloned(),
            captured,
        })))
    }
    /// A bare name: the literals it may spell, then locals, bindings, the row
    /// scope, and finally the workspace.
    fn name(&mut self, path: &Path, n: &str) -> EvalResult<Value> {
        if let Some(value) = keyword(n).or_else(|| self.local(n)) {
            return Ok(value);
        }
        if let Some(value) = self.binding(n) {
            return value;
        }
        let Some(scope) = self.row() else {
            return self.resolved(path, n);
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
    /// its call — a body walked symbolically by a linear reading — the name
    /// still decides.
    fn param(&mut self, path: &Path, name: &str, index: usize) -> EvalResult<Value> {
        if let Some(value) = self.call_frame().and_then(|(_, args)| args.get(index)) {
            return Ok(value.clone());
        }
        self.name(path, name)
    }
    fn binary_expr(&mut self, path: &Path, op: BinaryOp, a: &Expr, b: &Expr) -> EvalResult<Value> {
        let a = self.expr(path, a)?;
        // A decided `and` or `or` never evaluates its right-hand side.
        if (op == BinaryOp::And && a == Value::Bool(false))
            || (op == BinaryOp::Or && a == Value::Bool(true))
        {
            return Ok(a);
        }
        let right = self.expr(path, b)?;
        // The operands' kind names, kept for a failure: a tagged record's is
        // its own text, every other kind's a static name, so nothing is
        // copied for the common case.
        let name = |value: &Value| match value.kind() {
            ValueType::Tagged => std::borrow::Cow::Owned(value.type_name().to_owned()),
            kind => std::borrow::Cow::Borrowed(kind.as_str()),
        };
        let (left, right_kind) = (name(&a), name(&right));
        binary(op, a, right).map_err(|source| {
            let message = source.in_binary(op, &left, &right_kind);
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
                return Err("Module evaluation cannot access the filesystem".into());
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                return Err("Local file existence is unavailable in the browser".into());
            }
            #[cfg(not(target_arch = "wasm32"))]
            return Ok(Value::Bool(
                resource
                    .url(path, self.request.workspace.home())?
                    .to_file_path()
                    .is_ok_and(|p| p.exists()),
            ));
        }
        let (links, ws, now) = (
            self.request.links,
            self.request.workspace,
            self.request.clock.now,
        );
        let (target, now) = (&resource.target, now.to_utc());
        let time_dependent = links.time_dependent(target, &ws.cache, now);
        self.time_dependent |= time_dependent;
        let memo_key = MemoKey::ResourceProperty(resource.target.clone(), key.into());
        if let Some(Found::Reached(entry) | Found::Earlier(entry)) =
            self.request.memo.find(&memo_key)
        {
            return entry.value.clone();
        }
        let value = links.property(target, &ws.cache, now, key);
        self.request.memo.keep(
            memo_key,
            MemoEntry {
                value: value.clone(),
                failure: None,
                wanted: vec![],
                time_dependent,
                contextual: false,
                cost: Default::default(),
                about: None,
            },
        );
        value
    }
    fn property(&mut self, value: &Value, key: &str) -> EvalResult<Value> {
        match value {
            Value::Namespace(path) => self.named(path.path(), key),
            Value::Tasks(keys) if key == values::CHECKLIST_TASKS => Ok(self.checklist_tasks(keys)),
            Value::List(items) => items
                .iter()
                .map(|v| self.property(v, key))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::list),
            _ => value.property(key),
        }
    }
    /// A module's own evaluation: immutable inputs only, including resource
    /// properties, no link features, and the module-tier built-ins answer.
    /// It shares `memo`.
    pub(crate) fn for_module(
        workspace: &'a Workspace,
        now: DateTime<FixedOffset>,
        memo: crate::memo::Memo,
    ) -> Self {
        let request = crate::context::RequestContext {
            workspace,
            clock: crate::context::Clock::new(now),
            links: modules::LinkFeatures::default(),
            memo,
        };
        let mut engine = Self::in_request(&request);
        engine.module = true;
        engine
    }
    /// The note text being evaluated: a module engine's is its caller's,
    /// since its own text is the module's.
    pub(crate) fn reading_for(&self) -> Option<(Arc<Path>, Span)> {
        if self.module {
            self.reader.clone()
        } else {
            self.trace.contexts.last().cloned()
        }
    }
    /// Whether module code is running: a module engine, or a buffer that lives
    /// where modules live, so an author still sees their own file evaluate.
    pub(crate) fn module_code(&self, path: &Path) -> bool {
        self.module || modules::is_module_path(path)
    }
    pub(crate) fn with_expressions(mut self, expressions: Arc<BTreeMap<String, Expr>>) -> Self {
        self.expressions = Some(expressions);
        self
    }
    pub(crate) fn with_environment(mut self, environment: Arc<Workspace>) -> Self {
        self.environment = Some(environment);
        self
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
        _ => Err("Invalid unary operation".into()),
    }
}
