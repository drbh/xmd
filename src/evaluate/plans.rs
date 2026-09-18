//! Linear plans: `[name] := maximize(expr)` followed by a constraint table.
//! Names that resolve to note values are constants; the rest are decision
//! variables solved with a pure-Rust simplex, so plans re-solve as notes change.
use crate::{
    document::{Document, Named, Problem, Span, identifier},
    engine::{Engine, Linear, Value},
    tables::{Cell, Table},
    workspace::{Symbol, SymbolKind, Workspace},
};

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
    /// Typed result plus source geometry; presentation policy lives in plan.wtf.
    pub fn record(&self, ws: &Workspace) -> Value {
        use crate::plugins::{from_json, record};
        let columns = self
            .columns()
            .into_iter()
            .filter_map(|(symbol, column, cells)| {
                let table = crate::tables::table(ws, &symbol)?;
                let doc = &ws.documents[&symbol.path];
                Some(record([
                    (
                        "name".into(),
                        Value::Text(table.columns[column].name.clone()),
                    ),
                    (
                        "cells".into(),
                        Value::List(
                            cells
                                .into_iter()
                                .filter_map(|(row, value)| {
                                    let cell = table.rows.get(row)?.get(column)?;
                                    let line = doc.line(cell.span.line).as_bytes();
                                    let (mut a, mut b) = (cell.span.start, cell.span.end);
                                    while a > 0 && line[a - 1] == b' ' {
                                        a -= 1;
                                    }
                                    while b < line.len() && line[b] == b' ' {
                                        b += 1;
                                    }
                                    Some(record([
                                        ("value".into(), value),
                                        (
                                            "label".into(),
                                            Value::Text(
                                                table.rows[row]
                                                    .first()
                                                    .map(|c| c.source.clone())
                                                    .unwrap_or_else(|| (row + 1).to_string()),
                                            ),
                                        ),
                                        (
                                            "document".into(),
                                            Value::Text(
                                                crate::paths::file_url(&symbol.path)
                                                    .ok()?
                                                    .to_string(),
                                            ),
                                        ),
                                        ("source".into(), Value::Text(cell.source.clone())),
                                        ("line".into(), Value::Count(cell.span.line)),
                                        (
                                            "anchor".into(),
                                            from_json(&serde_json::json!(
                                                cell.span.range(&doc.text).end
                                            )),
                                        ),
                                        (
                                            "range".into(),
                                            from_json(&serde_json::json!(
                                                Span::new(cell.span.line, a, b).range(&doc.text)
                                            )),
                                        ),
                                        ("width".into(), Value::Count((b - a).saturating_sub(2))),
                                    ]))
                                })
                                .collect(),
                        ),
                    ),
                ]))
            })
            .collect();
        let constraints = self
            .constraints
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut fields = std::collections::BTreeMap::from([
                    ("name".into(), Value::Text(c.name.clone())),
                    ("op".into(), Value::Text(c.op.clone())),
                    ("lhs".into(), c.lhs.clone()),
                    ("rhs".into(), c.rhs.clone()),
                    ("slack".into(), c.slack.clone()),
                    ("binding".into(), Value::Bool(c.binding)),
                ]);
                if let Some((_, plan)) = plan(ws, &self.origin)
                    && let Some(constraint) = plan.constraints.get(i)
                {
                    let doc = &ws.documents[&self.origin.path];
                    fields.insert(
                        "anchor".into(),
                        from_json(&serde_json::json!(doc.line_end(constraint.span.line))),
                    );
                    fields.insert(
                        "range".into(),
                        from_json(&serde_json::json!(constraint.span.range(&doc.text))),
                    );
                }
                Value::Record(fields)
            })
            .collect();
        record([
            ("goal".into(), Value::Text(self.goal.keyword().into())),
            ("objective".into(), self.objective.clone()),
            ("variables".into(), record(self.variables.iter().cloned())),
            (
                "variable_order".into(),
                Value::List(
                    self.variables
                        .iter()
                        .map(|(name, _)| Value::Text(name.clone()))
                        .collect(),
                ),
            ),
            ("constraints".into(), Value::List(constraints)),
            ("columns".into(), Value::List(columns)),
        ])
    }
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
    let raw = def.value_span.source(&doc.text);
    let offset = def.value_span.start + raw.len() - raw.trim_start().len();
    let header = def.end.line + 1;
    let objective = &def.source[inner_start..inner_end];
    let objective_start = inner_start + objective.len() - objective.trim_start().len();
    let objective = objective.trim();
    let mut plan = Plan {
        definition,
        goal,
        objective: objective.into(),
        objective_span: Span::new(def.value_span.line, offset, offset).relative(
            &doc.text,
            objective_start,
            objective_start + objective.len(),
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
pub fn contains(plan: &Plan, span: Span, text: &str) -> bool {
    regions(plan).any(|r| r.contains(text, span))
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
    let raw = def.value_span.source(&doc.text);
    let offset = def.value_span.start + raw.len() - raw.trim_start().len();
    let start = offset + def.source.find(body).unwrap_or(0);
    let span = Span::new(def.value_span.line, start, start + body.len()).relative(
        &doc.text,
        0,
        body.len(),
    );
    let vars = [name.clone()].into_iter().collect();
    let (lhs, op, rhs) = engine.constraint(&symbol.path, body, span, &vars)?;
    let difference = lhs.minus(&rhs)?;
    let fail = |engine: &mut Engine<'_>, message: String| {
        engine.failure.get_or_insert(crate::engine::EvalFailure {
            path: symbol.path.clone(),
            span,
            message: message.clone(),
            related: vec![],
        });
        message
    };
    let _ = op;
    let unit = difference
        .unknown_kind()
        .map(|k| typed_in(k, difference.currency, 1.0))
        .unwrap_or(Value::Null);
    crate::plugins::standard(
        "plan",
        "seek_boundary",
        vec![Value::Text(name), form_value(&difference), unit],
        engine.now,
    )
    .map_err(|message| fail(engine, message))
}
/// Human-readable direction for a goal seek.
pub fn seek_summary(op: &str, coefficient_positive: bool) -> String {
    crate::plugins::standard(
        "plan",
        "seek_summary",
        vec![Value::Text(op.into()), Value::Bool(coefficient_positive)],
        chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.fixed_offset(),
    )
    .expect("valid comparison")
    .display()
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

fn typed_in(kind: &str, currency: Option<crate::engine::Currency>, n: f64) -> Value {
    match kind {
        "Money" => Value::Money(n, currency.unwrap_or(crate::engine::Currency::USD)),
        "Duration" => Value::Duration(n.round() as i64),
        _ => Value::Number(n),
    }
}
fn form_value(form: &Linear) -> Value {
    crate::plugins::record([
        ("constant".into(), Value::Number(form.constant)),
        (
            "terms".into(),
            crate::plugins::record(
                form.terms
                    .iter()
                    .map(|(k, n)| (k.clone(), Value::Number(*n))),
            ),
        ),
        ("unit".into(), typed_in(form.kind, form.currency, 1.0)),
    ])
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
    use crate::plugins::{field, list, record};
    let input = record([
        ("goal".into(), Value::Text(plan.goal.keyword().into())),
        (
            "names".into(),
            Value::List(names.iter().cloned().map(Value::Text).collect()),
        ),
        (
            "decisions".into(),
            Value::List(
                rows.iter()
                    .map(|row| {
                        record([
                            ("name".into(), Value::Text(row.name.clone())),
                            (
                                "kind".into(),
                                Value::Text(
                                    match row.domain {
                                        crate::tables::Domain::Choice => "binary",
                                        crate::tables::Domain::Count => "integer",
                                    }
                                    .into(),
                                ),
                            ),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("objective".into(), form_value(&objective)),
        (
            "constraints".into(),
            Value::List(
                plan.constraints
                    .iter()
                    .zip(&constraints)
                    .map(|(constraint, (lhs, op, rhs))| {
                        record([
                            ("name".into(), Value::Text(constraint.named.name.clone())),
                            ("lhs".into(), form_value(lhs)),
                            ("rhs".into(), form_value(rhs)),
                            ("op".into(), Value::Text(op.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]);
    let result = crate::plugins::standard("plan", "solve_model", vec![input], engine.now)
        .inspect_err(|message| {
            engine.failure.get_or_insert(crate::engine::EvalFailure {
                path: path.clone(),
                span: plan.objective_span,
                message: message.clone(),
                related: vec![],
            });
        })?;
    let variables = list(field(&result, "variables")?)?
        .iter()
        .map(|v| Ok((field(v, "name")?.display(), field(v, "value")?.clone())))
        .collect::<Result<_, String>>()?;
    let results = list(field(&result, "constraints")?)?
        .iter()
        .map(|c| {
            Ok(ConstraintResult {
                name: field(c, "name")?.display(),
                op: field(c, "op")?.display(),
                lhs: field(c, "lhs")?.clone(),
                rhs: field(c, "rhs")?.clone(),
                slack: field(c, "slack")?.clone(),
                binding: matches!(field(c, "binding")?, Value::Bool(true)),
            })
        })
        .collect::<Result<_, String>>()?;
    let choices = field(&result, "rows")?;
    let rows = rows
        .into_iter()
        .map(|row| Ok((row.clone(), field(choices, &row.name)?.clone())))
        .collect::<Result<_, String>>()?;
    Ok(Value::Plan(std::sync::Arc::new(PlanValue {
        origin: symbol.clone(),
        goal: plan.goal,
        objective: field(&result, "objective")?.clone(),
        variables,
        constraints: results,
        rows,
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
/// WTF source for an alps problem file.
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
