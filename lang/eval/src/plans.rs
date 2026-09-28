//! Solving a linear plan: names that resolve to note values are constants,
//! the rest are decision variables solved with a pure-Rust simplex, so plans
//! re-solve as notes change. The plan's shape — the goal, its constraint
//! table and the pure text scans over them — is parsed in `model::plans`.
use crate::error::{EvalError, EvalResult};
use crate::{
    engine_impl::{Comparison, Engine, Linear, Unit, Value},
    records::{
        CellRecord, ColumnRecord, ConstraintInput, ConstraintRecord, DecisionRecord, Fields,
        FormRecord, FromValue, PlanInput, PlanRecord, SolutionRecord, ToValue,
    },
    workspace::{Symbol, SymbolKind, Workspace},
};
use common::Span;

// A plan's shape is parsed in `model`; this module solves it, and both
// halves answer to `eval::plans`.
pub use model::plans::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintResult {
    pub name: String,
    pub op: Comparison,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
}
/// (table, column index, (row index, chosen value) per row).
pub(crate) type ColumnChoices = (Symbol, usize, Vec<(usize, Value)>);
#[derive(Clone, Debug, PartialEq)]
pub struct PlanValue {
    pub origin: Symbol,
    pub goal: Goal,
    pub objective: Value,
    pub variables: Vec<(String, Value)>,
    pub constraints: Vec<ConstraintResult>,
    /// Decision-column cells the plan filled in.
    pub rows: Vec<(crate::engine_impl::RowVariable, Value)>,
}
impl PlanValue {
    /// Typed result plus source geometry; presentation policy lives in plan.wtf.
    pub fn record(&self, ws: &Workspace) -> Value {
        use crate::modules_impl::from_json;
        let columns = self
            .columns()
            .into_iter()
            .filter_map(|(symbol, column, cells)| {
                let table = crate::tables::table(ws, &symbol)?;
                let doc = &ws.documents[&symbol.path];
                Some(ColumnRecord {
                    name: table.columns[column].name.clone(),
                    cells: cells
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
                            Some(CellRecord {
                                value,
                                label: table.rows[row]
                                    .first()
                                    .map(|c| c.source.clone())
                                    .unwrap_or_else(|| (row + 1).to_string()),
                                document: common::file_url(&symbol.path).ok()?.to_string(),
                                source: cell.source.clone(),
                                line: cell.span.line,
                                anchor: from_json(&serde_json::json!(
                                    cell.span.range(&doc.text).end
                                )),
                                range: from_json(&serde_json::json!(
                                    Span::new(cell.span.line, a, b).range(&doc.text)
                                )),
                                width: (b - a).saturating_sub(2),
                            })
                        })
                        .collect(),
                })
            })
            .collect();
        let constraints = self
            .constraints
            .iter()
            .enumerate()
            .map(|(i, c)| {
                // Only a plan still in the workspace can point at its own source.
                let source = plan(ws, &self.origin)
                    .and_then(|(_, plan)| plan.constraints.get(i))
                    .map(|constraint| {
                        let doc = &ws.documents[&self.origin.path];
                        (
                            from_json(&serde_json::json!(doc.line_end(constraint.span.line))),
                            from_json(&serde_json::json!(constraint.span.range(&doc.text))),
                        )
                    });
                ConstraintRecord {
                    name: c.name.clone(),
                    op: c.op.as_str().into(),
                    lhs: c.lhs.clone(),
                    rhs: c.rhs.clone(),
                    slack: c.slack.clone(),
                    binding: c.binding,
                    anchor: source.as_ref().map(|(anchor, _)| anchor.clone()),
                    range: source.map(|(_, range)| range),
                }
            })
            .collect();
        PlanRecord {
            goal: self.goal.keyword().into(),
            objective: self.objective.clone(),
            variables: self.variables.clone(),
            constraints,
            columns,
        }
        .to_value()
    }
    pub fn property(&self, name: &str) -> EvalResult<Value> {
        if name == "objective" {
            return Ok(self.objective.clone());
        }
        if let Some((_, v)) = self.variables.iter().find(|(n, _)| n == name) {
            return Ok(v.clone());
        }
        if let Some(c) = self.constraints.iter().find(|c| c.name == name) {
            return Ok(c.slack.clone());
        }
        Err(crate::error::EvalError::UnknownProperty {
            owner: crate::error::PropertyOwner::Plan,
            name: name.into(),
        })
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

/// Goal seek: the definition's own name is the unknown, and the answer is the
/// boundary value that makes the constraint hold. Linear equations have a
/// closed form, so no solver runs.
pub(crate) fn seek(engine: &mut Engine<'_>, symbol: &Symbol) -> EvalResult<Value> {
    let doc = &engine.workspace().documents[&symbol.path];
    let SymbolKind::Definition(index) = symbol.kind else {
        return Err(EvalError::Expected("a definition"));
    };
    let def = &doc.definitions[index];
    let name = def.named.name.clone();
    let body = crate::plans::seek_body(&def.source).ok_or(EvalError::Message(
        "solve() needs a constraint, e.g. solve(total >= $500)".into(),
    ))?;
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
    let fail = |engine: &mut Engine<'_>, message: EvalError| {
        engine
            .failure
            .get_or_insert(crate::engine_impl::EvalFailure {
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
    engine
        .call_module(
            "plan",
            "seek_boundary",
            vec![Value::Text(name), form_value(&difference), unit],
        )
        .map_err(|message| fail(engine, message))
}
pub(crate) fn plan<'a>(ws: &'a Workspace, symbol: &Symbol) -> Option<(usize, &'a Plan)> {
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

fn typed_in(kind: Unit, currency: Option<crate::engine_impl::Currency>, n: f64) -> Value {
    match kind {
        Unit::Money => Value::Money(n, currency.unwrap_or(crate::engine_impl::Currency::USD)),
        Unit::Duration => Value::Duration(n.round() as i64),
        _ => Value::Number(n),
    }
}
fn form_record(form: &Linear) -> FormRecord {
    FormRecord {
        constant: form.constant,
        terms: form.terms.iter().map(|(k, n)| (k.clone(), *n)).collect(),
        unit: typed_in(form.kind, form.currency, 1.0),
    }
}
fn form_value(form: &Linear) -> Value {
    form_record(form).to_value()
}
pub(crate) fn solve(engine: &mut Engine<'_>, symbol: &Symbol, plan: &Plan) -> EvalResult<Value> {
    let ws = engine.workspace();
    let path = symbol.path.clone();
    let names: Vec<String> = ws
        .plan_variables(&path, plan)
        .into_iter()
        .map(|(_, n)| n.name.clone())
        .collect();
    let vars: std::collections::BTreeSet<String> = names.iter().cloned().collect();
    if !plan.problems.is_empty() {
        let problem = &plan.problems[0];
        let message = EvalError::Message(problem.message.clone());
        engine
            .failure
            .get_or_insert(crate::engine_impl::EvalFailure {
                path: path.clone(),
                span: problem.span,
                message: message.clone(),
                related: vec![],
            });
        return Err(message);
    }
    engine.row_variables.clear();
    let objective = engine.linear(&path, &plan.objective, plan.objective_span, &vars)?;
    let mut constraints = Vec::new();
    for constraint in &plan.constraints {
        constraints.push(engine.constraint(&path, &constraint.source, constraint.span, &vars)?);
    }
    if names.is_empty() && engine.row_variables.is_empty() {
        let message =
            EvalError::Message("A plan needs at least one unknown name to solve for".into());
        engine
            .failure
            .get_or_insert(crate::engine_impl::EvalFailure {
                path: path.clone(),
                span: plan.objective_span,
                message: message.clone(),
                related: vec![],
            });
        return Err(message);
    }
    let rows = std::mem::take(&mut engine.row_variables);
    let input = PlanInput {
        goal: plan.goal.keyword().into(),
        names,
        decisions: rows
            .iter()
            .map(|row| DecisionRecord {
                name: row.name.clone(),
                kind: match row.domain {
                    crate::tables::Domain::Choice => "binary",
                    crate::tables::Domain::Count => "integer",
                }
                .into(),
            })
            .collect(),
        objective: form_record(&objective),
        constraints: plan
            .constraints
            .iter()
            .zip(&constraints)
            .map(|(constraint, (lhs, op, rhs))| ConstraintInput {
                name: constraint.named.name.clone(),
                lhs: form_record(lhs),
                rhs: form_record(rhs),
                op: op.as_str().into(),
            })
            .collect(),
    };
    let result = engine
        .call_module("plan", "solve_model", vec![input.to_value()])
        .inspect_err(|message| {
            engine
                .failure
                .get_or_insert(crate::engine_impl::EvalFailure {
                    path: path.clone(),
                    span: plan.objective_span,
                    message: message.clone(),
                    related: vec![],
                });
        })?;
    let solution = SolutionRecord::from_value(&result)?;
    let variables = solution
        .variables
        .into_iter()
        .map(|v| (v.name, v.value))
        .collect();
    let results = solution
        .constraints
        .into_iter()
        .map(|c| {
            Ok(ConstraintResult {
                name: c.name,
                op: c
                    .op
                    .parse()
                    .map_err(|_| "Linear comparison must be <=, >=, or ==")?,
                lhs: c.lhs,
                rhs: c.rhs,
                slack: c.slack,
                binding: c.binding,
            })
        })
        .collect::<EvalResult<_>>()?;
    let choices = Fields::new(&solution.rows)?;
    let rows = rows
        .into_iter()
        .map(|row| Ok((row.clone(), choices.required::<Value>(&row.name)?)))
        .collect::<EvalResult<_>>()?;
    Ok(Value::Plan(std::sync::Arc::new(PlanValue {
        origin: symbol.clone(),
        goal: plan.goal,
        objective: solution.objective,
        variables,
        constraints: results,
        rows,
    })))
}
