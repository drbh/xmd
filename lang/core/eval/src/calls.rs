//! Calling: the built-in dispatch table, note functions, modules, and the
//! lookups and row sums that decide for themselves what to evaluate.
use crate::engine::{
    BinaryOp, Builtin, Currency, Engine, Expr, RowScope, Tier, Value, binary, date_value,
    relative_date, sized,
};
use crate::{timers::Timer, workspace::Workspace};
use modules::Module;
use std::{collections::BTreeMap, path::Path};
use syntax::Literal;
use values::{Depth, EvalError, EvalResult, Limit, Overflow};

impl Engine<'_> {
    /// Dispatch a built-in by variant. A special form decides for itself
    /// whether and how to evaluate its arguments; everything else is answered
    /// by the functional table over already-evaluated values.
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
                values::sum(values)
            }
            Builtin::Eval if args.len() == 1 => self.call_eval(path, &args[0]),
            Builtin::Sum => self.sum(path, args).map(|(value, _)| value),
            Builtin::Now if args.is_empty() => {
                self.time_dependent = true;
                Ok(Value::DateTime(self.request.clock.now))
            }
            Builtin::Stopwatch | Builtin::Countdown => self.call_timer(path, builtin, args),
            Builtin::Today if args.is_empty() => Ok(Value::Date(self.request.clock.today())),
            Builtin::Rate
            | Builtin::To
            | Builtin::Forecast
            | Builtin::ForecastRange
            | Builtin::Quote => self.lookup(path, builtin, args),
            Builtin::Date => self.call_date(path, args),
            // The checklist counts. `today(x)`, `now(x)`, a wrong-arity
            // `eval` and the names only a plan or goal seek answers end here
            // too, and fail the way a checklist question would.
            builtin => self.call_checklist(path, builtin, args),
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
            .unwrap_or_else(|| self.named(path, name))?;
        let values = self.values(path, args)?;
        self.call(function, values)
    }
    fn module_engine<'b>(&self, workspace: &'b std::sync::Arc<Workspace>) -> Engine<'b> {
        let mut engine = Engine::for_module(workspace, self.request.clock.now)
            .with_environment(workspace.clone());
        engine.budget.steps = self.budget.steps;
        engine.budget.calls = self.budget.calls;
        engine.request.memo = self.request.memo.clone();
        engine
    }
    fn absorb_module(&mut self, other: &Engine<'_>) {
        self.budget.steps = other.budget.steps;
        self.time_dependent |= other.time_dependent;
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
    fn in_module<T>(
        &mut self,
        module: &Module,
        f: impl FnOnce(&mut Engine<'_>) -> EvalResult<T>,
    ) -> EvalResult<T> {
        let workspace = crate::module_runtime::workspace(module);
        let mut engine = self
            .module_engine(&workspace)
            .with_expressions(module.expressions().clone());
        let result = f(&mut engine);
        self.absorb_module(&engine);
        result
    }
    /// Typed adapters use the same module snapshot, clock and execution budget as imports.
    pub fn call_module(&mut self, id: &str, name: &str, args: Vec<Value>) -> EvalResult<Value> {
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
    /// A library's words for something, or why it has none: what a hover or
    /// label shows in place of text the module could not produce.
    pub fn present(&mut self, id: &str, name: &str, args: Vec<Value>) -> String {
        values::words(self.call_module(id, name, args))
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
        if module.kind != modules::ModuleKind::Library {
            return Err(EvalError::NotALibrary {
                id: id.into(),
                kind: module.kind.into(),
            });
        }
        let names = if self.module_code(path) {
            module.member_names()
        } else {
            let names = module.public_names();
            if names.is_empty() {
                // The engine's own libraries: called by name, never imported.
                return Err(EvalError::NotALibrary {
                    id: id.into(),
                    kind: module.kind.into(),
                });
            }
            names
        };
        self.in_module(module, |engine| {
            names
                .into_iter()
                .map(|name| {
                    let value = engine.named(&module.path, &name)?;
                    Ok((name, value))
                })
                .collect::<EvalResult<BTreeMap<_, _>>>()
                .map(Value::Record)
        })
    }
    pub fn call(&mut self, function: Value, args: Vec<Value>) -> EvalResult<Value> {
        let Value::Function(function) = function else {
            return Err(EvalError::Expected("a function"));
        };
        if let Some(workspace) = function
            .environment
            .clone()
            .and_then(|e| e.downcast::<Workspace>().ok())
            && !std::ptr::eq(self.request.workspace, workspace.as_ref())
        {
            let mut engine = self.module_engine(&workspace);
            engine.expressions = function.expressions.clone();
            let result = engine.call(Value::Function(function.clone()), args);
            self.absorb_module(&engine);
            return result;
        }
        if args.len() != function.params.len() {
            return Err(EvalError::FunctionArity {
                expected: function.params.len(),
                found: args.len(),
            });
        }
        if self.budget.calls >= 32 {
            return Err(EvalError::DepthExceeded(Depth::Call));
        }
        let height = self.barrier(false);
        self.push_call(function.clone(), args);
        self.budget.calls += 1;
        if let Some(source) = &function.source {
            self.trace.contexts.push(source.clone());
        }
        let result = self.expr(&function.path, &function.body);
        if function.source.is_some() {
            self.trace.contexts.pop();
        }
        self.budget.calls -= 1;
        self.unwind(height);
        sized(result?)
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
        if self.budget.calls >= 32 {
            return Err(EvalError::DepthExceeded(Depth::Call));
        }
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
    /// `stopwatch(…)` and `countdown(…)`: the timer module resolves the state.
    fn call_timer(&mut self, path: &Path, builtin: Builtin, args: &[Expr]) -> EvalResult<Value> {
        let values = self.values(path, args)?;
        let time_dependent = self.time_dependent;
        let timer = Timer::new(self, builtin.as_str(), &values)?;
        // The module declares whether this resolved state still needs a clock.
        self.time_dependent = time_dependent || timer.time_dependent()?;
        Ok(Value::Host(std::sync::Arc::new(timer)))
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
    /// The counts a named checklist heading answers.
    fn call_checklist(
        &mut self,
        path: &Path,
        builtin: Builtin,
        args: &[Expr],
    ) -> EvalResult<Value> {
        let [arg] = args else {
            return Err(EvalError::Arity(builtin));
        };
        let Value::Tasks(tasks) = self.expr(path, arg)?.plain() else {
            return Err(EvalError::Message(format!(
                "{builtin} expects a named checklist heading"
            )));
        };
        let done = tasks.iter().filter(|(p, i)| self.task_done(p, *i)).count();
        match builtin {
            Builtin::Total => Ok(Value::Count(tasks.len())),
            Builtin::Completed => Ok(Value::Count(done)),
            Builtin::Remaining => Ok(Value::Count(tasks.len() - done)),
            Builtin::Effort => {
                let mut seconds = 0i64;
                for (p, i) in tasks {
                    if !self.task_done(&p, i) {
                        let task = &self.request.workspace.documents[&p].tasks[i];
                        if let Some(attr) = task.attributes.get("estimate") {
                            let Value::Duration(m) = self.eval(&p, &attr.value)? else {
                                return Err(EvalError::Message(
                                    "@estimate requires a duration".into(),
                                ));
                            };
                            if m < 0 {
                                return Err(EvalError::Message(
                                    "Estimate cannot be negative".into(),
                                ));
                            }
                            seconds = seconds
                                .checked_add(m)
                                .ok_or(EvalError::Overflowed(Overflow::Duration))?;
                        }
                    }
                }
                Ok(Value::Duration(seconds))
            }
            _ => Err(EvalError::UnknownFunction(builtin.to_string())),
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
            (Builtin::Desc, [function @ Function(_)]) => {
                Record([(DESCENDING.into(), function.clone())].into())
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
                for item in items {
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
                List(keyed.into_iter().map(|(_, item)| item).collect())
            }
            (Builtin::GroupBy, [List(items), function @ Function(_)]) => {
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for item in items {
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
                List(
                    groups
                        .into_iter()
                        .map(|(key, rows)| {
                            Record([("key".into(), key), ("rows".into(), List(rows))].into())
                        })
                        .collect(),
                )
            }
            (Builtin::Map | Builtin::Filter, [List(items), function @ Function(_)]) => {
                let mut output = Vec::new();
                for item in items {
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
                List(output)
            }
            (Builtin::Fold, [List(items), initial, function @ Function(_)]) => {
                let mut result = initial.clone();
                for item in items {
                    result = self.call(function.clone(), vec![result, item.clone()])?;
                }
                result
            }
            _ => values::builtin(name, &args)?,
        };
        sized(value)
    }
    /// The table a row `sum` walks, named by its first argument.
    pub(crate) fn summed_table<'e>(
        &mut self,
        path: &Path,
        first: &'e Expr,
    ) -> EvalResult<(&'e str, std::sync::Arc<crate::tables_impl::TableValue>)> {
        let Some(name) = first.as_name() else {
            return Err(EvalError::Message(
                "The first argument to sum must be a table name".into(),
            ));
        };
        let Some(table) = self
            .named(path, name)?
            .downcast_arc::<crate::tables_impl::TableValue>()
        else {
            return Err(EvalError::NotATable(name.into()));
        };
        Ok((name, table))
    }
    pub(crate) fn sum(&mut self, path: &Path, args: &[Expr]) -> EvalResult<(Value, Vec<Value>)> {
        if args.len() != 2 {
            return Err(EvalError::Message(
                "sum expects a table and a row expression: sum(groceries, quantity * price)".into(),
            ));
        }
        let (name, table) = self.summed_table(path, &args[0])?;
        let mut total = None;
        let mut contributions = Vec::new();
        let decisions = self.decision_columns(&table);
        for values in table.named_rows() {
            self.push_row(RowScope {
                table: name.into(),
                values,
                decisions: decisions
                    .keys()
                    .map(|c| (c.clone(), String::new()))
                    .collect(),
            });
            let value = self.expr(path, &args[1]);
            self.pop_row();
            let value = value?;
            if !matches!(
                value,
                Value::Number(_) | Value::Money(..) | Value::Ratio(_) | Value::Duration(_)
            ) {
                return Err(EvalError::Message(format!(
                    "sum requires numeric, money, ratio, or duration results, found {}",
                    value.type_name()
                )));
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
            EvalError::Message(
                "Cannot sum an empty table: add a row to establish its value type".into(),
            )
        })
    }

    /// `rate(EUR, USD)`, `to(money, USD)`, `forecast("Oaxaca", 2026-11-20[, F])`,
    /// `forecast_range` and `quote(NVDA)`: values from the lookup cache, never
    /// fetched here.
    fn lookup(&mut self, path: &Path, name: Builtin, args: &[Expr]) -> EvalResult<Value> {
        // A code literal is what these calls are written with; text is still
        // accepted, so a computed name works too.
        let code = |value: Value, what: &str| match value {
            Value::Code(code) => Ok(code.to_string()),
            Value::Text(code) => Ok(code),
            other => Err(EvalError::Message(format!(
                "{what} must be a code such as USD, found {}",
                other.type_name()
            ))),
        };
        let currency = |code: &str| {
            Currency::parse(code).ok_or_else(|| {
                EvalError::Message(format!("'{code}' is not a currency code such as USD"))
            })
        };
        match name {
            Builtin::Rate => {
                if args.len() != 2 {
                    return Err(EvalError::Message(
                        "rate expects two currency codes: rate(EUR, USD)".into(),
                    ));
                }
                let from = currency(&code(self.expr(path, &args[0])?, "The first currency")?)?;
                let to = currency(&code(self.expr(path, &args[1])?, "The second currency")?)?;
                self.rate(from, to).map(Value::Number)
            }
            Builtin::To => {
                if args.len() != 2 {
                    return Err(EvalError::Message(
                        "to expects a money value and a currency code: to(hotel, USD)".into(),
                    ));
                }
                let Value::Money(amount, from) = self.expr(path, &args[0])? else {
                    return Err(EvalError::Message(
                        "to converts money; the first argument is not money".into(),
                    ));
                };
                let to = currency(&code(self.expr(path, &args[1])?, "The currency")?)?;
                Ok(Value::Money(amount * self.rate(from, to)?, to))
            }
            Builtin::Quote => {
                if args.len() != 1 {
                    return Err(EvalError::Message(
                        "quote expects a ticker symbol: quote(NVDA)".into(),
                    ));
                }
                let symbol = code(self.expr(path, &args[0])?, "The ticker")?;
                self.wanted.push(values::LookupKey::quote(&symbol));
                values::quote(&self.request.workspace.lookups, &symbol)
            }
            _ => {
                let range = name == Builtin::ForecastRange;
                if range && !(3..=4).contains(&args.len()) {
                    return Err(EvalError::Message(
                        "forecast_range expects a place, start date, end date, and optional unit: forecast_range(\"Oaxaca\", 2026-11-20, 2026-11-26, F)".into(),
                    ));
                }
                if !range && !(2..=3).contains(&args.len()) {
                    return Err(EvalError::Message(
                        "forecast expects a place and a date: forecast(\"Oaxaca\", 2026-11-20)"
                            .into(),
                    ));
                }
                let Value::Text(place) = self.expr(path, &args[0])? else {
                    return Err(EvalError::Message(
                        "The place must be text, e.g. forecast(\"Oaxaca\", 2026-11-20)".into(),
                    ));
                };
                let value = self.expr(path, &args[1])?;
                let date = self.date(&value)?;
                let end = if range {
                    let value = self.expr(path, &args[2])?;
                    self.date(&value)?
                } else {
                    date
                };
                let days = (end - date).num_days();
                if days < 0 {
                    return Err(EvalError::Message(
                        "forecast_range end date must be on or after the start date".into(),
                    ));
                }
                if days >= 4096 {
                    return Err(EvalError::LimitExceeded(Limit::ListItems));
                }
                let fahrenheit = match args.get(if range { 3 } else { 2 }) {
                    Some(unit) => match code(self.expr(path, unit)?, "The unit")?.as_str() {
                        "F" | "FAHRENHEIT" => true,
                        "C" | "CELSIUS" => false,
                        other => {
                            return Err(EvalError::Message(format!(
                                "Unknown temperature unit '{other}'; use F or C"
                            )));
                        }
                    },
                    None => false,
                };
                let dates: Vec<_> = (0..=days)
                    .map(|offset| date + chrono::Duration::days(offset))
                    .collect();
                // Discover the entire interval before a missing cache entry
                // can fail evaluation, so one refresh fetches every day.
                self.wanted.extend(
                    dates
                        .iter()
                        .map(|date| values::LookupKey::forecast(&place, *date)),
                );
                let mut forecasts = dates
                    .into_iter()
                    .map(|date| {
                        let lookups = &self.request.workspace.lookups;
                        values::forecast(lookups, &place, date, fahrenheit).map(Value::Forecast)
                    })
                    .collect::<EvalResult<Vec<_>>>()?;
                Ok(if range {
                    Value::List(forecasts)
                } else {
                    forecasts.remove(0)
                })
            }
        }
    }
    /// The cached rate from one currency to another, noting that it was read.
    fn rate(&mut self, from: Currency, to: Currency) -> EvalResult<f64> {
        if from != to {
            self.wanted.push(values::LookupKey::rate(from, to));
        }
        values::rate(&self.request.workspace.lookups, from, to)
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
