//! Linear plans: `[name] := maximize(expr)` followed by a constraint table.
//! Names that resolve to note values are constants; the rest are decision
//! variables solved with a pure-Rust simplex, so plans re-solve as notes change.
use crate::{
    document::{Document, Named, Problem, Span, identifier},
    engine::{Engine, Linear, Value},
    tables::{Cell, Table},
    workspace::{Symbol, SymbolKind, Workspace},
};
use good_lp::{Expression, ProblemVariables, ResolutionError, Solution, SolverModel, variable};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Maximize,
    Minimize,
}
impl Goal {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Maximize => "maximize",
            Self::Minimize => "minimize",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Constraint {
    pub named: Named,
    pub source: String,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub definition: usize,
    pub goal: Goal,
    pub objective: String,
    pub objective_span: Span,
    pub header: usize,
    pub end_line: usize,
    pub columns: Vec<Named>,
    pub separators: Vec<String>,
    pub constraints: Vec<Constraint>,
    /// Every distinct name read by the objective or a constraint, first
    /// occurrence first. The workspace decides which are decision variables.
    pub names: Vec<Named>,
    pub problems: Vec<Problem>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintResult {
    pub name: String,
    pub op: String,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
}
/// (table, column index, (row index, chosen value) per row).
pub type ColumnChoices = (Symbol, usize, Vec<(usize, Value)>);
#[derive(Clone, Debug, PartialEq)]
pub struct PlanValue {
    pub origin: Symbol,
    pub goal: Goal,
    pub objective: Value,
    pub variables: Vec<(String, Value)>,
    pub constraints: Vec<ConstraintResult>,
    /// Decision-column cells the plan filled in.
    pub rows: Vec<(crate::engine::RowVariable, Value)>,
}
impl PlanValue {
    pub fn property(&self, name: &str) -> Result<Value, String> {
        if name == "objective" {
            return Ok(self.objective.clone());
        }
        if let Some((_, v)) = self.variables.iter().find(|(n, _)| n == name) {
            return Ok(v.clone());
        }
        if let Some(c) = self.constraints.iter().find(|c| c.name == name) {
            return Ok(c.slack.clone());
        }
        Err(format!("Unknown plan property '{name}'"))
    }
    /// Per decision column: (table, column, chosen values in row order).
    pub fn columns(&self) -> Vec<ColumnChoices> {
        let mut result: Vec<ColumnChoices> = Vec::new();
        for (row, value) in &self.rows {
            match result
                .iter_mut()
                .find(|(t, c, _)| *t == row.table && *c == row.column)
            {
                Some((_, _, cells)) => cells.push((row.row, value.clone())),
                None => result.push((
                    row.table.clone(),
                    row.column,
                    vec![(row.row, value.clone())],
                )),
            }
        }
        result
    }
    pub fn property_names(&self) -> Vec<String> {
        std::iter::once("objective".to_string())
            .chain(self.variables.iter().map(|(n, _)| n.clone()))
            .chain(self.constraints.iter().map(|c| c.name.clone()))
            .collect()
    }
}

/// `maximize(...)` or `minimize(...)` wrapping the whole source, with the byte
/// range of the objective inside the parentheses.
pub fn goal(source: &str) -> Option<(Goal, usize, usize)> {
    let source_end = source.trim_end().len();
    for (keyword, goal) in [("maximize", Goal::Maximize), ("minimize", Goal::Minimize)] {
        let Some(rest) = source.strip_prefix(keyword) else {
            continue;
        };
        let open = keyword.len() + (rest.len() - rest.trim_start().len());
        if source.as_bytes().get(open) != Some(&b'(') || !source[..source_end].ends_with(')') {
            return None;
        }
        // The closing paren must match the opening one, not an inner call.
        let mut depth = 0;
        for (i, c) in source[open..source_end].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && open + i + 1 != source_end {
                        return None;
                    }
                }
                _ => {}
            }
        }
        return Some((goal, open + 1, source_end - 1));
    }
    None
}

pub fn parse(doc: &Document, definition: usize, lines: &[&str]) -> Plan {
    let def = &doc.definitions[definition];
    let (goal, inner_start, inner_end) = goal(&def.source).unwrap();
    let raw = &lines[def.value_span.line][def.value_span.start..def.value_span.end];
    let offset = def.value_span.start + raw.len() - raw.trim_start().len();
    let header = def.named.span.line + 1;
    let mut plan = Plan {
        definition,
        goal,
        objective: def.source[inner_start..inner_end].trim().into(),
        objective_span: Span::new(
            def.value_span.line,
            offset + inner_start,
            offset + inner_end,
        ),
        header,
        end_line: header,
        columns: vec![],
        separators: vec![],
        constraints: vec![],
        names: vec![],
        problems: vec![],
    };
    let mut problem = |span, message: String| plan.problems.push(Problem { span, message });
    if plan.objective.is_empty() {
        problem(
            def.value_span,
            format!("{}() needs an objective expression", goal.keyword()),
        );
    }
    let Some(headers) = lines
        .get(header)
        .and_then(|l| crate::tables::cells(l, header))
    else {
        problem(
            def.value_span,
            "A plan needs a | constraint | expression | table on the next line".into(),
        );
        return plan;
    };
    if headers.len() != 2 || headers.iter().any(|(name, _)| !identifier(name)) {
        problem(
            Span::new(header, 0, lines[header].len()),
            "Plan tables have two columns: | constraint | expression |".into(),
        );
    }
    plan.columns = headers
        .into_iter()
        .map(|(name, span)| Named { name, span })
        .collect();
    plan.end_line = header + 1;
    if let Some(parts) = lines
        .get(header + 1)
        .and_then(|l| crate::tables::cells(l, header + 1))
    {
        plan.end_line = header + 2;
        plan.separators = parts.iter().map(|(s, _)| s.clone()).collect();
        if parts.len() != plan.columns.len()
            || parts.iter().any(|(s, _)| {
                let core = s.trim_matches(':');
                core.len() < 3 || !core.bytes().all(|c| c == b'-')
            })
        {
            problem(
                Span::new(header + 1, 0, lines[header + 1].len()),
                "Table separator must have one --- cell per column".into(),
            );
        }
    } else {
        problem(
            def.value_span,
            "A plan table needs a Markdown separator row after its header".into(),
        );
    }
    while let Some(line) = lines
        .get(plan.end_line)
        .filter(|l| l.trim_start().starts_with('|'))
    {
        let row = plan.end_line;
        plan.end_line += 1;
        let Some(parts) = crate::tables::cells(line, row) else {
            problem(
                Span::new(row, 0, line.len()),
                "Unclosed table row; use outer | delimiters".into(),
            );
            continue;
        };
        if parts.len() != 2 {
            problem(
                Span::new(row, 0, line.len()),
                format!(
                    "Expected a constraint name and an expression, found {} cells",
                    parts.len()
                ),
            );
            continue;
        }
        let (name, name_span) = &parts[0];
        let (source, span) = &parts[1];
        if !identifier(name) {
            problem(*name_span, "Constraint names must be identifiers".into());
            continue;
        }
        if plan.constraints.iter().any(|c| c.named.name == *name) {
            problem(*name_span, format!("Duplicate constraint '{name}'"));
        }
        if source.is_empty() {
            problem(
                *span,
                "Missing constraint expression, e.g. bagels >= 12".into(),
            );
        }
        plan.constraints.push(Constraint {
            named: Named {
                name: name.clone(),
                span: *name_span,
            },
            source: source.clone(),
            span: *span,
        });
    }
    plan
}
/// Byte regions whose references belong to the plan.
pub fn regions(plan: &Plan) -> impl Iterator<Item = Span> + '_ {
    std::iter::once(plan.objective_span).chain(plan.constraints.iter().map(|c| c.span))
}
pub fn contains(plan: &Plan, span: Span) -> bool {
    regions(plan).any(|r| r.line == span.line && span.start >= r.start && span.end <= r.end)
}
/// The plan's rows as a table, so formatting and format-on-type align them.
pub fn grid(plan: &Plan) -> Table {
    Table {
        definition: plan.definition,
        header: plan.header,
        end_line: plan.end_line,
        columns: plan.columns.clone(),
        separators: plan.separators.clone(),
        rows: plan
            .constraints
            .iter()
            .map(|c| {
                [(&c.named.name, c.named.span), (&c.source, c.span)]
                    .into_iter()
                    .map(|(source, span)| Cell {
                        source: source.clone(),
                        span,
                        value: Ok(Value::Text(source.clone())),
                        expression: None,
                    })
                    .collect()
            })
            .collect(),
        types: vec![Some("Text"); 2],
        problems: plan.problems.clone(),
        domains: vec![None; 2],
    }
}
/// `solve(constraint)`: the body of a goal-seek definition.
pub fn seek_body(source: &str) -> Option<&str> {
    let rest = source.strip_prefix("solve")?;
    let rest = rest.trim_start();
    let inner = rest.strip_prefix('(')?.trim_end().strip_suffix(')')?;
    let mut depth = 0;
    for c in inner.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(inner.trim())
}
/// Goal seek: the definition's own name is the unknown, and the answer is the
/// boundary value that makes the constraint hold. Linear equations have a
/// closed form, so no solver runs.
pub fn seek(engine: &mut Engine<'_>, symbol: &Symbol) -> Result<Value, String> {
    let doc = &engine.workspace.documents[&symbol.path];
    let SymbolKind::Definition(index) = symbol.kind else {
        return Err("Expected a definition".into());
    };
    let def = &doc.definitions[index];
    let name = def.named.name.clone();
    let body =
        seek_body(&def.source).ok_or("solve() needs a constraint, e.g. solve(total >= $500)")?;
    let raw = &doc.line(def.value_span.line)[def.value_span.start..def.value_span.end];
    let offset = def.value_span.start + raw.len() - raw.trim_start().len();
    let start = offset + def.source.find(body).unwrap_or(0);
    let span = Span::new(def.value_span.line, start, start + body.len());
    let vars = [name.clone()].into_iter().collect();
    let (lhs, op, rhs) = engine.constraint(&symbol.path, body, span, &vars)?;
    let difference = lhs.minus(&rhs)?;
    let coefficient = difference.terms.get(&name).copied().unwrap_or(0.0);
    let fail = |engine: &mut Engine<'_>, message: String| {
        engine.failure.get_or_insert(crate::engine::EvalFailure {
            path: symbol.path.clone(),
            span,
            message: message.clone(),
            related: vec![],
        });
        message
    };
    if coefficient == 0.0 {
        return Err(fail(
            engine,
            format!("The constraint does not depend on {name}"),
        ));
    }
    if difference.terms.len() > 1 {
        let others: Vec<_> = difference
            .terms
            .keys()
            .filter(|k| **k != name)
            .cloned()
            .collect();
        return Err(fail(
            engine,
            format!(
                "solve() finds one value; {} would need a plan",
                others.join(", ")
            ),
        ));
    }
    let value = -difference.constant / coefficient;
    let kind = difference.unknown_kind().ok_or_else(|| {
        fail(
            engine,
            "Cannot tell the unit of the answer; the unknown is scaled by two different units"
                .into(),
        )
    })?;
    let _ = op;
    Ok(typed(kind, value))
}
/// Human-readable direction for a goal seek: what the boundary value means.
pub fn seek_summary(op: &str, coefficient_positive: bool) -> &'static str {
    match (op, coefficient_positive) {
        ("==", _) => "the exact value that satisfies",
        (">=", true) | ("<=", false) => "the smallest value that satisfies",
        _ => "the largest value that satisfies",
    }
}
pub fn plan<'a>(ws: &'a Workspace, symbol: &Symbol) -> Option<(usize, &'a Plan)> {
    if let SymbolKind::Definition(index) = symbol.kind {
        ws.documents[&symbol.path]
            .plans
            .iter()
            .enumerate()
            .find(|(_, p)| p.definition == index)
    } else {
        None
    }
}

fn typed(kind: &str, n: f64) -> Value {
    match kind {
        "Money" => Value::Money(n),
        "Duration" => Value::Duration(n.round() as i64),
        _ => Value::Number(n),
    }
}
fn evaluate(form: &Linear, values: &BTreeMap<String, f64>) -> f64 {
    form.constant
        + form
            .terms
            .iter()
            .map(|(name, coef)| coef * values.get(name).copied().unwrap_or(0.0))
            .sum::<f64>()
}
fn tidy(n: f64) -> f64 {
    // Simplex results carry ~1e-6 noise; four decimals is also the display precision.
    let rounded = (n * 1e4).round() / 1e4;
    if rounded == 0.0 { 0.0 } else { rounded }
}

pub fn solve(engine: &mut Engine<'_>, symbol: &Symbol, plan: &Plan) -> Result<Value, String> {
    let ws = engine.workspace;
    let path = symbol.path.clone();
    let names: Vec<String> = ws
        .plan_variables(&path, plan)
        .into_iter()
        .map(|(_, n)| n.name.clone())
        .collect();
    let vars: std::collections::BTreeSet<String> = names.iter().cloned().collect();
    if !plan.problems.is_empty() {
        let problem = &plan.problems[0];
        engine.failure.get_or_insert(crate::engine::EvalFailure {
            path: path.clone(),
            span: problem.span,
            message: problem.message.clone(),
            related: vec![],
        });
        return Err(problem.message.clone());
    }
    engine.row_variables.clear();
    let objective = engine.linear(&path, &plan.objective, plan.objective_span, &vars)?;
    let mut constraints = Vec::new();
    for constraint in &plan.constraints {
        constraints.push(engine.constraint(&path, &constraint.source, constraint.span, &vars)?);
    }
    if names.is_empty() && engine.row_variables.is_empty() {
        engine.failure.get_or_insert(crate::engine::EvalFailure {
            path: path.clone(),
            span: plan.objective_span,
            message: "A plan needs at least one unknown name to solve for".into(),
            related: vec![],
        });
        return Err("A plan needs at least one unknown name to solve for".into());
    }
    let rows = std::mem::take(&mut engine.row_variables);
    let mut problem = ProblemVariables::new();
    let mut variables: BTreeMap<String, good_lp::Variable> = names
        .iter()
        .map(|name| (name.clone(), problem.add(variable().min(0.0))))
        .collect();
    for row in &rows {
        let definition = match row.domain {
            crate::tables::Domain::Choice => variable().binary(),
            crate::tables::Domain::Count => variable().integer().min(0),
        };
        variables.insert(row.name.clone(), problem.add(definition));
    }
    let expression = |form: &Linear| {
        let mut e = Expression::from(form.constant);
        for (name, coef) in &form.terms {
            e += *coef * variables[name];
        }
        e
    };
    let unsolved = match plan.goal {
        Goal::Maximize => problem.maximise(expression(&objective)),
        Goal::Minimize => problem.minimise(expression(&objective)),
    };
    let mut model = unsolved.using(good_lp::microlp);
    for (lhs, op, rhs) in &constraints {
        let (l, r) = (expression(lhs), expression(rhs));
        model = model.with(match op.as_str() {
            "<=" => l.leq(r),
            ">=" => l.geq(r),
            _ => l.eq(r),
        });
    }
    let solution = model.solve().map_err(|e| {
        let message = match e {
            ResolutionError::Infeasible => {
                "No values satisfy every constraint; relax one of them".to_string()
            }
            ResolutionError::Unbounded => format!(
                "The objective can be {} without limit; add a constraint that bounds it",
                match plan.goal {
                    Goal::Maximize => "raised",
                    Goal::Minimize => "lowered",
                }
            ),
            other => format!("Solver error: {other}"),
        };
        engine.failure.get_or_insert(crate::engine::EvalFailure {
            path: path.clone(),
            span: plan.objective_span,
            message: message.clone(),
            related: vec![],
        });
        message
    })?;
    // Evaluate objective and constraints from the raw solution so rounding the
    // displayed variables never shifts the reported totals.
    let values: BTreeMap<String, f64> = variables
        .iter()
        .map(|(name, var)| (name.clone(), solution.value(*var)))
        .collect();
    let _ = &rows;
    let results = plan
        .constraints
        .iter()
        .zip(&constraints)
        .map(|(constraint, (lhs, op, rhs))| {
            let (l, r) = (tidy(evaluate(lhs, &values)), tidy(evaluate(rhs, &values)));
            let kind = if lhs.kind == "Number" {
                rhs.kind
            } else {
                lhs.kind
            };
            let slack = match op.as_str() {
                "<=" => r - l,
                ">=" => l - r,
                _ => 0.0,
            };
            ConstraintResult {
                name: constraint.named.name.clone(),
                op: op.clone(),
                lhs: typed(kind, l),
                rhs: typed(kind, r),
                slack: typed(kind, tidy(slack)),
                binding: tidy(slack) == 0.0,
            }
        })
        .collect();
    Ok(Value::Plan(std::sync::Arc::new(PlanValue {
        origin: symbol.clone(),
        goal: plan.goal,
        objective: typed(objective.kind, tidy(evaluate(&objective, &values))),
        variables: names
            .iter()
            .map(|n| (n.clone(), Value::Number(tidy(values[n]))))
            .collect(),
        constraints: results,
        rows: rows
            .into_iter()
            .map(|row| {
                let value = tidy(values[&row.name]);
                let value = match row.domain {
                    crate::tables::Domain::Choice => Value::Bool(value >= 0.5),
                    crate::tables::Domain::Count => Value::Number(value.round()),
                };
                (row, value)
            })
            .collect(),
    })))
}

/// Canonical text for a linear form: `3 * bagels + 1.25 * doughnuts + 4`.
pub fn render(form: &Linear) -> String {
    let mut parts: Vec<String> = form
        .terms
        .iter()
        .filter(|(_, c)| **c != 0.0)
        .map(|(name, coef)| {
            if *coef == 1.0 {
                name.clone()
            } else {
                format!("{} * {name}", crate::engine::decimal(*coef))
            }
        })
        .collect();
    if form.constant != 0.0 || parts.is_empty() {
        parts.push(crate::engine::decimal(form.constant));
    }
    parts.join(" + ").replace("+ -", "- ")
}

/// The alps interchange format: variables, one objective, named constraints,
/// with every note value substituted so the file stands alone.
pub fn export(
    engine: &mut Engine<'_>,
    symbol: &Symbol,
    plan: &Plan,
) -> Result<serde_json::Value, String> {
    let ws = engine.workspace;
    let names: Vec<String> = ws
        .plan_variables(&symbol.path, plan)
        .into_iter()
        .map(|(_, n)| n.name.clone())
        .collect();
    let vars = names.iter().cloned().collect();
    let objective = engine.linear(&symbol.path, &plan.objective, plan.objective_span, &vars)?;
    let mut constraints = Vec::new();
    for c in &plan.constraints {
        let (lhs, op, rhs) = engine.constraint(&symbol.path, &c.source, c.span, &vars)?;
        constraints.push(serde_json::json!({
            "name": c.named.name,
            "expression": format!("{} {op} {}", render(&lhs), render(&rhs)),
        }));
    }
    Ok(serde_json::json!({
        "variables": names.iter().map(|n| (n.clone(), serde_json::json!({}))).collect::<serde_json::Map<_, _>>(),
        "objective": {
            "goal": match plan.goal { Goal::Maximize => "max", Goal::Minimize => "min" },
            "expression": render(&objective),
        },
        "constraints": constraints,
    }))
}
/// Jot source for an alps problem file.
pub fn import(name: &str, problem: &serde_json::Value) -> Result<String, String> {
    if !identifier(name) {
        return Err("Plan names use letters, digits, and underscores".into());
    }
    let objective = &problem["objective"];
    let goal = match objective["goal"].as_str() {
        Some("max" | "maximize" | "maximise") => Goal::Maximize,
        Some("min" | "minimize" | "minimise") => Goal::Minimize,
        _ => return Err("objective.goal must be max or min".into()),
    };
    let expression = objective["expression"]
        .as_str()
        .ok_or("objective.expression must be a string")?;
    let mut rows: Vec<(String, String)> = Vec::new();
    for c in problem["constraints"]
        .as_array()
        .ok_or("constraints must be a list")?
    {
        let name = c["name"].as_str().ok_or("Each constraint needs a name")?;
        let expr = c["expression"]
            .as_str()
            .ok_or("Each constraint needs an expression")?;
        rows.push((name.into(), expr.into()));
    }
    if let Some(vars) = problem["variables"].as_object() {
        for (var, bounds) in vars {
            if let Some(min) = bounds["min"].as_f64() {
                rows.push((
                    format!("{var}_min"),
                    format!("{var} >= {}", crate::engine::decimal(min)),
                ));
            }
            if let Some(max) = bounds["max"].as_f64() {
                rows.push((
                    format!("{var}_max"),
                    format!("{var} <= {}", crate::engine::decimal(max)),
                ));
            }
        }
    }
    let width = rows
        .iter()
        .map(|(n, _)| n.len())
        .max()
        .unwrap_or(10)
        .max(10);
    let wide = rows
        .iter()
        .map(|(_, e)| e.len())
        .max()
        .unwrap_or(10)
        .max(10);
    let mut out = format!("[{name}] := {}({})\n", goal.keyword(), expression.trim());
    out.push_str(&format!(
        "| {:<width$} | {:<wide$} |\n",
        "constraint", "expression"
    ));
    out.push_str(&format!(
        "| {} | {} |\n",
        "-".repeat(width),
        "-".repeat(wide)
    ));
    for (n, e) in rows {
        out.push_str(&format!("| {n:<width$} | {e:<wide$} |\n"));
    }
    Ok(out)
}
