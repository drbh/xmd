//! Calling: the built-in dispatch table, note functions, modules, and the
//! lookups and row sums that decide for themselves what to evaluate.
use super::{
    BinaryOp, Builtin, Currency, Engine, Expr, RowScope, Tier, Value, binary, date_value,
    relative_date,
};
use crate::{
    error::{Depth, EvalError, EvalResult, Limit, Overflow},
    timers_impl::Timer,
    workspace::Workspace,
};
use std::{collections::BTreeMap, path::Path};
use syntax::Literal;

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
            Builtin::Coalesce => self.call_coalesce(path, args),
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
                crate::functional_impl::sum(values)
            }
            Builtin::Eval if args.len() == 1 => self.call_eval(path, &args[0]),
            Builtin::Sum => self.sum(path, args).map(|(value, _)| value),
            Builtin::Now if args.is_empty() => {
                self.time_dependent = true;
                Ok(Value::DateTime(self.request.clock.now))
            }
            Builtin::Stopwatch | Builtin::Countdown => self.call_timer(path, builtin, args),
            Builtin::Today if args.is_empty() => Ok(Value::Date(self.request.today)),
            Builtin::Rate
            | Builtin::To
            | Builtin::Forecast
            | Builtin::ForecastRange
            | Builtin::Quote => self.lookup(path, builtin, args),
            // The one-argument tail: a date, a checklist question, and the
            // names that only a plan or a goal seek answers.
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
        let mut engine = Engine::at(workspace, self.request.clock.now)
            .pure()
            .module()
            .with_link_features(crate::link_features_impl::LinkFeatures::new(&[]))
            .with_environment(workspace.clone());
        engine.budget.steps = self.budget.steps;
        engine.budget.calls = self.budget.calls;
        engine.request.today = self.request.today;
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
    /// Typed adapters use the same module snapshot, clock and execution budget as imports.
    pub fn call_module(&mut self, id: &str, name: &str, args: Vec<Value>) -> EvalResult<Value> {
        let module = self
            .request
            .workspace
            .modules
            .active()
            .find(|m| m.id == id)
            .ok_or_else(|| EvalError::ModuleUnavailable(id.into()))?
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
    /// `import(id)` reaches libraries and nothing else: a link or feature
    /// module is the host's to call, so naming one from a note is an error
    /// rather than a record of hooks. A note sees the library's declared
    /// `exports`; module code, which the engine trusts the way it trusts its
    /// own adapters, sees every non-`_` member of a library it imports.
    fn import(&mut self, path: &Path, id: &str) -> EvalResult<Value> {
        let module = self
            .request
            .workspace
            .modules
            .active()
            .find(|m| m.id == id)
            .ok_or_else(|| EvalError::UnknownImport(id.into()))?
            .clone();
        if module.kind != crate::modules_impl::ModuleKind::Library {
            return Err(EvalError::NotALibrary {
                id: id.into(),
                kind: module.kind,
            });
        }
        let workspace = module.environment();
        let mut engine = self
            .module_engine(&workspace)
            .with_expressions(module.expressions.clone());
        let names = if self.module_code(path) {
            module.member_names()
        } else {
            let names = module.public_names();
            if names.is_empty() {
                // The engine's own libraries: called by name, never imported.
                return Err(EvalError::NotALibrary {
                    id: id.into(),
                    kind: module.kind,
                });
            }
            names
        };
        let result = names
            .into_iter()
            .map(|name| {
                let value = engine.named(&module.path, &name)?;
                Ok((name, value))
            })
            .collect::<EvalResult<BTreeMap<_, _>>>()
            .map(Value::Record);
        self.absorb_module(&engine);
        result
    }
    pub fn call(&mut self, function: Value, args: Vec<Value>) -> EvalResult<Value> {
        let Value::Function(function) = function else {
            return Err(EvalError::Expected("a function"));
        };
        if let Some(workspace) = &function.environment
            && !std::ptr::eq(self.request.workspace, workspace.as_ref())
        {
            let mut engine = self.module_engine(workspace);
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
        let value = result?;
        crate::functional_impl::check_size(&value)?;
        Ok(value)
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
                return Err(EvalError::Message(
                    "Note imports require a literal path, e.g. import(\"./values.wtf\")".into(),
                ));
            }
            let target = model::note_path(path, &id)?;
            if !self.request.workspace.documents.contains_key(&target) {
                return Err(EvalError::Message(format!(
                    "Note import '{}' is not loaded (from {})",
                    target.display(),
                    path.display()
                )));
            }
            return Ok(Value::Namespace(super::Namespace(target)));
        }
        self.import(path, &id)
    }
    /// `if(condition, then, else)`: only the chosen branch is evaluated.
    fn call_if(&mut self, path: &Path, args: &[Expr]) -> EvalResult<Value> {
        if args.len() != 3 {
            return Err(EvalError::Message(
                "if expects a condition and two branches".into(),
            ));
        }
        let Value::Bool(condition) = self.expr(path, &args[0])? else {
            return Err(EvalError::Message("if requires a Boolean condition".into()));
        };
        self.expr(path, &args[if condition { 1 } else { 2 }])
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
        Ok(Value::Timer(std::sync::Arc::new(timer)))
    }
    /// The one-argument built-ins: `date` over text or a timestamp, and the
    /// counts a named checklist heading answers.
    fn call_checklist(
        &mut self,
        path: &Path,
        builtin: Builtin,
        args: &[Expr],
    ) -> EvalResult<Value> {
        if args.len() != 1 {
            return Err(EvalError::Arity(builtin));
        }
        let value = self.expr(path, &args[0])?.plain();
        if builtin == Builtin::Date {
            return match value {
                Value::Text(s) => date_value(&s)
                    .or_else(|| relative_date(&s, self.request.today).map(Value::Date))
                    .ok_or(EvalError::Message("Unrecognized date".into())),
                other => self.date(&other).map(Value::Date),
            };
        }
        let Value::Tasks(tasks) = value else {
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
            (Builtin::SortBy, [List(items), function @ Function(_)]) => {
                let mut keyed = Vec::new();
                let mut first = None;
                for item in items {
                    let key = self.call(function.clone(), vec![item.clone()])?;
                    crate::functional_impl::compare(&key, &key)?;
                    if key != Null {
                        if let Some(first) = &first {
                            crate::functional_impl::compare(first, &key)?;
                        } else {
                            first = Some(key.clone());
                        }
                    }
                    keyed.push((key, item.clone()));
                }
                // Every key has been checked against the common scalar type.
                keyed.sort_by(|(a, _), (b, _)| crate::functional_impl::compare(a, b).unwrap());
                List(keyed.into_iter().map(|(_, item)| item).collect())
            }
            (Builtin::GroupBy, [List(items), function @ Function(_)]) => {
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for item in items {
                    let key = self.call(function.clone(), vec![item.clone()])?;
                    crate::functional_impl::compare(&key, &key)?;
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
            _ => crate::functional_impl::builtin(name, &args)?,
        };
        crate::functional_impl::check_size(&value)?;
        Ok(value)
    }
    pub(super) fn sum(&mut self, path: &Path, args: &[Expr]) -> EvalResult<(Value, Vec<Value>)> {
        if args.len() != 2 {
            return Err(EvalError::Message(
                "sum expects a table and a row expression: sum(groceries, quantity * price)".into(),
            ));
        }
        let Some(name) = args[0].as_name() else {
            return Err(EvalError::Message(
                "The first argument to sum must be a table name".into(),
            ));
        };
        let Value::Table(table) = self.named(path, name)? else {
            return Err(EvalError::NotATable(name.into()));
        };
        let mut total = None;
        let mut contributions = Vec::new();
        let decisions = self.decision_columns(&table);
        for row in &table.rows {
            self.push_row(RowScope {
                table: name.into(),
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

    /// Rows of a table, evaluating calculated cells and checking that each
    /// column keeps one type. Failures point at the offending cell.
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
                if from != to {
                    self.wanted
                        .push(crate::lookups_impl::LookupKey::rate(from, to));
                }
                crate::lookups_impl::rate(&self.request.workspace.lookups, from, to)
                    .map(Value::Number)
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
                if from != to {
                    self.wanted
                        .push(crate::lookups_impl::LookupKey::rate(from, to));
                }
                let rate = crate::lookups_impl::rate(&self.request.workspace.lookups, from, to)?;
                Ok(Value::Money(amount * rate, to))
            }
            Builtin::Quote => {
                if args.len() != 1 {
                    return Err(EvalError::Message(
                        "quote expects a ticker symbol: quote(NVDA)".into(),
                    ));
                }
                let symbol = code(self.expr(path, &args[0])?, "The ticker")?;
                self.wanted
                    .push(crate::lookups_impl::LookupKey::quote(&symbol));
                crate::lookups_impl::quote(&self.request.workspace.lookups, &symbol)
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
                    return Err(EvalError::LimitExceeded(crate::error::Limit::ListItems));
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
                        .map(|date| crate::lookups_impl::LookupKey::forecast(&place, *date)),
                );
                if range {
                    dates
                        .into_iter()
                        .map(|date| {
                            crate::lookups_impl::forecast(
                                &self.request.workspace.lookups,
                                &place,
                                date,
                                fahrenheit,
                            )
                            .map(Value::Forecast)
                        })
                        .collect::<EvalResult<Vec<_>>>()
                        .map(Value::List)
                } else {
                    crate::lookups_impl::forecast(
                        &self.request.workspace.lookups,
                        &place,
                        date,
                        fahrenheit,
                    )
                    .map(Value::Forecast)
                }
            }
        }
    }
}
