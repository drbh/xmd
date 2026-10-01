//! Calling: the built-in dispatch, the core special forms (`import`, `if`,
//! `match`, `coalesce`, `eval`, `date`, the clock and `clocked`), the lookup
//! cache (`cached`), note functions and modules, the prelude's among them. A
//! special built-in a feature owns, such as a row `sum`, is answered by
//! what `features` registers for it.
use crate::engine::{
    BinaryOp, Builtin, Engine, Expr, Function, Tier, Value, binary, date_value, relative_date,
};
use crate::{features, memo::Walk, workspace::Workspace};
use modules::Module;
use std::{collections::BTreeMap, path::Path};
use syntax::Literal;
use values::{EvalError, EvalResult, Limit};

impl Engine<'_> {
    /// Dispatch a built-in by variant. A special form decides for itself
    /// whether and how to evaluate its arguments; everything else is answered
    /// by the functional table over already-evaluated values, and a special
    /// form the engine does not answer by the feature registered for it.
    pub(crate) fn builtin(
        &mut self,
        path: &Path,
        builtin: Builtin,
        args: &[Expr],
    ) -> EvalResult<Value> {
        // A module-tier built-in is not a name a note has: outside module code
        // it fails exactly as a misspelling would.
        if builtin.tier() == Tier::Module && !self.module_code(path) {
            return Err(EvalError::UnknownFunction(builtin.as_str().into()));
        }
        match builtin {
            Builtin::Import => self.call_import(path, args),
            Builtin::If => self.call_if(path, args),
            Builtin::Match => self.call_match(path, args),
            Builtin::Coalesce => self.call_coalesce(path, args),
            // The parser turns every `let` into lambda calls.
            Builtin::Let => Err(EvalError::Message(
                "let expects {name: value, …} and a body".into(),
            )),
            // Everything else with eagerly evaluated arguments.
            builtin if !builtin.is_special_form() => {
                let values = self.values(path, args)?;
                self.functional(builtin, values)
            }
            Builtin::Sum if args.len() == 1 => {
                let Value::List(values) = self.expr(path, &args[0])? else {
                    return Err(EvalError::Message(
                        "sum expects a list, or a table and row expression".into(),
                    ));
                };
                values::sum(values.iter().cloned())
            }
            Builtin::Eval if args.len() == 1 => self.call_eval(path, &args[0]),
            Builtin::Now | Builtin::Today if args.is_empty() && !self.has_clock() => {
                Err(EvalError::Message(format!(
                    "{builtin}() is unavailable here: native code calls this without a clock, \
                     so the dates it needs are its arguments"
                )))
            }
            Builtin::Now if args.is_empty() => {
                self.time_dependent = true;
                Ok(Value::DateTime(self.request.clock.now))
            }
            Builtin::Today if args.is_empty() => Ok(Value::Date(self.request.clock.today())),
            Builtin::Date => self.call_date(path, args),
            Builtin::Clocked => self.call_clocked(path, args),
            // Without arguments the engine answers these itself.
            Builtin::Today | Builtin::Now => {
                Err(EvalError::Message(format!("{builtin} takes no arguments")))
            }
            // A row `sum` is the feature's that registers it; the rest take
            // one argument and are here with any other count.
            builtin => match features::call(builtin) {
                Some(call) => call(self, path, args),
                None => Err(EvalError::Arity(builtin)),
            },
        }
    }
    /// `clocked(value, ticking)`: `value`, which keeps depending on the clock
    /// it read only while `ticking(value)` is true. A value that counts from
    /// a moment reads the clock and moves with it; once it stops it reads the
    /// clock too, but stays where it stopped, so nothing need refresh it.
    fn call_clocked(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        let builtin = Builtin::Clocked;
        let [value, ticking] = args else {
            return Err(EvalError::Message(format!(
                "{builtin} expects a value and a function that says whether it still ticks"
            )));
        };
        let before = std::mem::replace(&mut self.time_dependent, false);
        let result = self.expr(path, value).and_then(|value| {
            let read = std::mem::replace(&mut self.time_dependent, false);
            let ticking = self.expr(path, ticking)?;
            match self.call(ticking, vec![value.clone()])? {
                Value::Bool(ticks) => Ok((value, read && ticks)),
                _ => Err(EvalError::Message(format!(
                    "{builtin}'s function must return true or false"
                ))),
            }
        });
        match result {
            Ok((value, ticks)) => {
                self.time_dependent = before || ticks;
                Ok(value)
            }
            // A failure may have read the clock: it holds only for that instant.
            Err(error) => {
                self.time_dependent = true;
                Err(error)
            }
        }
    }
    /// A call to a note function: the callee is resolved like any other name,
    /// locals first, before its arguments are evaluated.
    pub(crate) fn call_named(
        &mut self,
        path: &Path,
        name: &str,
        args: &[Expr],
    ) -> EvalResult<Value> {
        let function = self
            .local(name)
            .map(Ok)
            .or_else(|| self.binding(name))
            .unwrap_or_else(|| self.resolved(path, name))
            .map_err(|error| match error {
                // A form is a definition, not a value inside one.
                EvalError::UnknownName { .. } if self.workspace().form(name).is_some() => {
                    EvalError::Message(format!(
                        "{name}(...) is only valid as a definition's whole expression"
                    ))
                }
                other => other,
            })?;
        // Called by name, a function is named when the call does not fit it,
        // before any argument is evaluated.
        if let Value::Function(f) = &function
            && !(f.required()..=f.params.len()).contains(&args.len())
        {
            return Err(EvalError::CallArity {
                name: name.into(),
                required: f.required(),
                params: f.params.len(),
                found: args.len(),
            });
        }
        let values = self.values(path, args)?;
        self.call(function, values)
    }
    /// An engine over a module's own workspace, carrying this one's budget,
    /// memo and the definition being evaluated, all of which
    /// [`Self::absorb_module`] takes back.
    fn module_engine<'b>(&mut self, workspace: &'b std::sync::Arc<Workspace>) -> Engine<'b> {
        let mut engine =
            Engine::for_module(workspace, self.request.clock.now, self.request.memo.clone())
                .with_environment(workspace.clone());
        engine.budget = self.budget;
        engine.walk = self.walk.take().map(Walk::lend);
        // The prelude's lookups read the note's cache, not the module's.
        engine.lookups = self.lookups.clone();
        // What the module reads, it reads for the note text calling it.
        engine.reader = self.reading_for();
        engine
    }
    fn absorb_module(&mut self, other: &mut Engine<'_>) {
        self.budget.steps = other.budget.steps;
        self.time_dependent |= other.time_dependent;
        self.wanted.append(&mut other.wanted);
        self.walk = other.walk.take().map(Walk::take_back);
        if self.failure.is_none() {
            // A module has its own source workspace. Let the caller attach an
            // error there to its call site instead of carrying an unusable span.
            self.failure = other
                .failure
                .clone()
                .filter(|failure| self.request.workspace.documents.contains_key(&failure.path));
        }
    }
    /// Run `f` on an engine over `module`'s own workspace and parsed
    /// expressions, then take back the budget and failure it leaves.
    pub(crate) fn in_module<T>(
        &mut self,
        module: &Module,
        f: impl FnOnce(&mut Engine<'_>) -> EvalResult<T>,
    ) -> EvalResult<T> {
        let workspace = crate::module_runtime::workspace(module);
        let mut engine = self
            .module_engine(&workspace)
            .with_expressions(module.expressions().clone());
        let result = f(&mut engine);
        self.absorb_module(&mut engine);
        result
    }
    /// Typed adapters use the same module snapshot, clock and execution budget as imports.
    /// Only the stdlib contract (`contract.rs`) calls a module function by name.
    pub(crate) fn call_module(
        &mut self,
        id: &str,
        name: &str,
        args: Vec<Value>,
    ) -> EvalResult<Value> {
        let module = self
            .workspace()
            .modules
            .get(id)
            .ok_or_else(|| EvalError::ModuleUnavailable(id.into()))?;
        self.in_module(module, |engine| {
            let function = engine.named(&module.path, name)?;
            engine.call(function, args)
        })
    }
    /// Call the hook a feature module defines for what the host evaluates
    /// on its behalf (`define`, what a definition of one of its forms is
    /// worth), inside this evaluation: on its memo, clock and budget.
    pub(crate) fn call_hook(
        &mut self,
        id: &str,
        hook: modules::Hook,
        args: Vec<Value>,
    ) -> EvalResult<Value> {
        let module = self
            .workspace()
            .modules
            .get(id)
            .filter(|module| module.kind == modules::ModuleKind::Feature)
            .ok_or_else(|| EvalError::ModuleUnavailable(id.into()))?;
        self.in_module(module, |engine| {
            let function = engine.named(&module.path, hook.as_ref())?;
            engine.call(function, args)
        })
    }
    /// `import(id)` reaches libraries and nothing else: a link or feature
    /// module is the host's to call, so naming one from a note is an error
    /// rather than a record of hooks. A note sees the library's declared
    /// `exports`; module code, which the engine trusts the way it trusts its
    /// own adapters, sees every non-`_` member of a library it imports.
    fn import(&mut self, path: &Path, id: &str) -> EvalResult<Value> {
        let module = self
            .workspace()
            .modules
            .get(id)
            .ok_or_else(|| EvalError::UnknownImport(id.into()))?;
        let module_code = self.module_code(path);
        let names = if module_code {
            module.member_names()
        } else {
            module.public_names()
        };
        // A library that exports nothing is one of the engine's own: called
        // by name, never imported.
        if module.kind != modules::ModuleKind::Library || (!module_code && names.is_empty()) {
            return Err(EvalError::NotALibrary {
                id: id.into(),
                kind: module.kind.into(),
            });
        }
        self.in_module(module, |engine| {
            names
                .into_iter()
                .map(|name| {
                    let value = engine.named(&module.path, &name)?;
                    Ok((name, value))
                })
                .collect::<EvalResult<BTreeMap<_, _>>>()
                .map(Value::record)
        })
    }
    pub(crate) fn call(&mut self, function: Value, args: Vec<Value>) -> EvalResult<Value> {
        let Value::Function(function) = function else {
            return Err(EvalError::Expected("a function"));
        };
        if let Some(workspace) = function
            .environment
            .clone()
            .and_then(|e| e.downcast::<Workspace>().ok())
            && !std::ptr::eq(self.request.workspace, workspace.as_ref())
        {
            let args = args.into_iter().map(|arg| self.export(arg)).collect();
            let mut engine = self.module_engine(&workspace);
            engine.expressions = function.expressions.clone();
            let result = engine.call(Value::Function(function.clone()), args);
            self.absorb_module(&mut engine);
            return result;
        }
        if args.len() < function.required() || args.len() > function.params.len() {
            return Err(EvalError::FunctionArity {
                expected: function.params.len(),
                found: args.len(),
            });
        }
        self.call_check()?;
        let height = self.barrier(false);
        let passed = args.len();
        self.push_call(function.clone(), args);
        self.budget.calls += 1;
        if let Some(source) = &function.source {
            self.trace.contexts.push(source.clone());
        }
        let result = self
            .fill_defaults(&function, passed)
            .and_then(|()| self.expr(&function.path, &function.body));
        if function.source.is_some() {
            self.trace.contexts.pop();
        }
        self.budget.calls -= 1;
        self.unwind(height);
        self.sized(result?)
    }
    /// Bind each parameter a call left out to its default, in order, so a
    /// default sees the parameters before it.
    fn fill_defaults(&mut self, function: &Function, passed: usize) -> EvalResult<()> {
        let first = function.required();
        for default in &function.defaults[passed.max(first) - first..] {
            let value = self.expr(&function.path, default)?.plain();
            self.push_argument(value);
        }
        Ok(())
    }
    /// `import(id)`: a module ID, or a literal path to another note.
    fn call_import(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        let [arg] = args else {
            return Err(EvalError::Message(
                "import expects a module ID or a literal note path".into(),
            ));
        };
        let Value::Text(id) = self.expr(path, arg)?.plain() else {
            return Err(EvalError::Message("import expects text".into()));
        };
        if model::is_note_path(&id) {
            if !matches!(arg.bare(), Expr::Value(Literal::Text(_))) {
                return Err(EvalError::Message(format!(
                    "Note imports require a literal path, e.g. import(\"./{}\")",
                    common::note_file("values")
                )));
            }
            let target = model::note_path(path, &id)?;
            if !self.request.workspace.documents.contains_key(&target) {
                return Err(EvalError::Message(format!(
                    "Note import '{}' is not loaded (from {})",
                    target.display(),
                    path.display()
                )));
            }
            return Ok(Value::Namespace(crate::engine::Namespace(target)));
        }
        self.import(path, &id)
    }
    /// `if(condition, then, …, else)`: conditions in order until one holds,
    /// and only the chosen result is evaluated.
    fn call_if(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        if args.len() < 3 || args.len().is_multiple_of(2) {
            return Err(EvalError::Message(
                "if expects conditions and results in pairs, then an else".into(),
            ));
        }
        let (otherwise, pairs) = args.split_last().expect("at least three arguments");
        for pair in pairs.chunks(2) {
            let Value::Bool(condition) = self.expr(path, &pair[0])? else {
                return Err(EvalError::Message("if requires a Boolean condition".into()));
            };
            if condition {
                return self.expr(path, &pair[1]);
            }
        }
        self.expr(path, otherwise)
    }
    /// `match(value, case, result, …, otherwise)`: the first case equal to
    /// the value picks its result, compared as `==` does; only the cases up
    /// to it and the chosen result are evaluated.
    fn call_match(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        if args.len() < 4 || !args.len().is_multiple_of(2) {
            return Err(EvalError::Message(
                "match expects a value, cases and results in pairs, then an otherwise".into(),
            ));
        }
        let (otherwise, rest) = args.split_last().expect("at least four arguments");
        let value = self.expr(path, &rest[0])?;
        for pair in rest[1..].chunks(2) {
            let case = self.expr(path, &pair[0])?;
            if binary(BinaryOp::Equal, value.clone(), case)? == Value::Bool(true) {
                return self.expr(path, &pair[1]);
            }
        }
        self.expr(path, otherwise)
    }
    /// `coalesce(a, b, …)`: stop at the first non-null argument.
    fn call_coalesce(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        for arg in args {
            let value = self.expr(path, arg)?;
            if value != Value::Null {
                return Ok(value);
            }
        }
        Ok(Value::Null)
    }
    /// `eval(text)`: parse and run expression text in the current document.
    fn call_eval(&mut self, path: &Path, arg: &Expr) -> EvalResult<Value> {
        self.call_check()?;
        let Value::Text(source) = self.expr(path, arg)? else {
            return Err(EvalError::Message("eval expects expression text".into()));
        };
        // Dynamic expressions use the current document, not query row fields,
        // but a row expression's `eval` still runs inside its row.
        let height = self.barrier(true);
        self.budget.calls += 1;
        let result = self.eval(path, &source);
        self.budget.calls -= 1;
        self.unwind(height);
        result
    }
    /// `date` over text or a timestamp.
    fn call_date(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        let [arg] = args else {
            return Err(EvalError::Arity(Builtin::Date));
        };
        match self.expr(path, arg)?.plain() {
            Value::Text(s) => date_value(&s)
                .or_else(|| relative_date(&s, self.request.clock.today()).map(Value::Date))
                .ok_or(EvalError::Message("Unrecognized date".into())),
            other => self.date(&other).map(Value::Date),
        }
    }
    pub(crate) fn functional(&mut self, name: Builtin, args: Vec<Value>) -> EvalResult<Value> {
        use Value::*;
        let value = match (name, args.as_slice()) {
            (Builtin::Get, [Namespace(path), Text(key)]) => {
                match self.request.workspace.resolve(path.path(), key) {
                    Ok(symbol) => self.symbol(&symbol)?,
                    Err(_)
                        if !self.request.workspace.symbols().iter().any(|s| {
                            s.path == path.path() && self.request.workspace.named(s).name == *key
                        }) =>
                    {
                        Null
                    }
                    Err(e) => return Err(e),
                }
            }
            (Builtin::Cached, [Text(kind), key]) => self.cached(kind, key, None)?,
            (Builtin::Cached, [Text(kind), key, Text(label)]) => {
                self.cached(kind, key, Some(label))?
            }
            (Builtin::Desc, [function @ Function(_)]) => {
                Value::record([(DESCENDING.into(), function.clone())].into())
            }
            (Builtin::SortBy, [List(items), keys]) => {
                // One key, desc(key), or a list of them, compared in order.
                let keys = match keys {
                    List(keys) => keys.iter().map(sort_key).collect::<Option<Vec<_>>>(),
                    key => sort_key(key).map(|key| vec![key]),
                }
                .filter(|keys| !keys.is_empty())
                .ok_or(EvalError::Message(
                    "sort_by expects a key function, desc(key), or a list of them".into(),
                ))?;
                let mut keyed = Vec::new();
                let mut first = vec![None; keys.len()];
                for item in items.iter() {
                    let mut values = Vec::new();
                    for ((function, _), first) in keys.iter().zip(&mut first) {
                        let key = self.call(function.clone(), vec![item.clone()])?;
                        values::compare(&key, &key)?;
                        if key != Null {
                            if let Some(first) = first {
                                values::compare(first, &key)?;
                            } else {
                                *first = Some(key.clone());
                            }
                        }
                        values.push(key);
                    }
                    keyed.push((values, item.clone()));
                }
                // Every key has been checked against its column's scalar type.
                // A descending key reverses values, but nulls still come last.
                keyed.sort_by(|(a, _), (b, _)| {
                    a.iter()
                        .zip(b)
                        .zip(&keys)
                        .map(|((a, b), (_, descending))| {
                            let order = values::compare(a, b).unwrap();
                            if *descending && *a != Null && *b != Null {
                                order.reverse()
                            } else {
                                order
                            }
                        })
                        .find(|order| order.is_ne())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                Value::list(keyed.into_iter().map(|(_, item)| item).collect())
            }
            (Builtin::GroupBy, [List(items), function @ Function(_)]) => {
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for item in items.iter() {
                    let key = self.call(function.clone(), vec![item.clone()])?;
                    values::compare(&key, &key)?;
                    if let Some((_, rows)) = groups.iter_mut().find(|(k, _)| {
                        binary(BinaryOp::Equal, k.clone(), key.clone()) == Ok(Bool(true))
                    }) {
                        rows.push(item.clone());
                    } else {
                        groups.push((key, vec![item.clone()]));
                    }
                }
                Value::list(
                    groups
                        .into_iter()
                        .map(|(key, rows)| {
                            Value::record(
                                [("key".into(), key), ("rows".into(), Value::list(rows))].into(),
                            )
                        })
                        .collect(),
                )
            }
            (Builtin::Map | Builtin::Filter, [List(items), function @ Function(_)]) => {
                let mut output = Vec::new();
                for item in items.iter() {
                    let value = self.call(function.clone(), vec![item.clone()])?;
                    if name == Builtin::Map {
                        output.push(value);
                    } else {
                        match value {
                            Bool(true) => output.push(item.clone()),
                            Bool(false) => (),
                            _ => {
                                return Err(EvalError::Message(
                                    "filter predicate must return a Boolean".into(),
                                ));
                            }
                        }
                    }
                    if output.len() > 4096 {
                        return Err(EvalError::LimitExceeded(Limit::ListItems));
                    }
                }
                Value::list(output)
            }
            (Builtin::Fold, [List(items), initial, function @ Function(_)]) => {
                let mut result = initial.clone();
                for item in items.iter() {
                    result = self.call(function.clone(), vec![result, item.clone()])?;
                }
                result
            }
            _ => values::builtin(name, &args)?,
        };
        self.sized(value)
    }
}

/// The field `desc(key)` wraps its key in, which `sort_by` reads back.
const DESCENDING: &str = "desc";

/// A `sort_by` key: a function, ascending, or `desc(key)`.
fn sort_key(key: &Value) -> Option<(Value, bool)> {
    match key {
        Value::Function(_) => Some((key.clone(), false)),
        Value::Record(fields) if fields.len() == 1 => match fields.get(DESCENDING)? {
            function @ Value::Function(_) => Some((function.clone(), true)),
            _ => None,
        },
        _ => None,
    }
}
