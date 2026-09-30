//! Solving a linear plan: names that resolve to note values are constants,
//! the rest are decision variables solved with a pure-Rust simplex, so plans
//! re-solve as notes change. The plan's shape — the goal, its constraint
//! table and the pure text scans over them — is parsed in `model::plans`.
use crate::{
    engine::{Engine, Linear, Unit, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use common::Span;
use model::plans::{Goal, Plan};
use std::collections::BTreeMap;
use syntax::Comparison;
use values::{EvalError, EvalResult};
use values::{Fields, FromValue, LINEAR_COMPARISON, RecordFields, ToValue, geometry, record};

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
    pub rows: Vec<(crate::engine::RowVariable, Value)>,
}
impl PlanValue {
    /// Typed result plus source geometry; presentation policy lives in plan.xmd.
    pub fn record(&self, ws: &Workspace) -> Value {
        let columns = self
            .columns()
            .into_iter()
            .filter_map(|(symbol, column, cells)| {
                let table = crate::tables_impl::table(ws, &symbol)?;
                let doc = &ws.documents[&symbol.path];
                Some(ColumnRecord {
                    name: table.columns[column].name.clone(),
                    cells: cells
                        .into_iter()
                        .filter_map(|(row, value)| {
                            let cell = table.rows.get(row)?.get(column)?;
                            let line = doc.line(cell.span.line);
                            // The cell with its padding, out to the pipes.
                            let head = line.get(..cell.span.start).unwrap_or(line);
                            let tail = line.get(cell.span.end..).unwrap_or("");
                            let a = head.trim_end_matches(' ').len();
                            let b = line.len() - tail.trim_start_matches(' ').len();
                            Some(CellRecord {
                                value,
                                label: table.rows[row]
                                    .first()
                                    .map(|c| c.source.clone())
                                    .unwrap_or_else(|| (row + 1).to_string()),
                                document: common::file_url(&symbol.path).ok()?.to_string(),
                                source: cell.source.clone(),
                                line: cell.span.line,
                                anchor: geometry(cell.span.range(doc).end),
                                range: geometry(Span::new(cell.span.line, a, b).range(doc)),
                                width: (b - a).saturating_sub(2),
                            })
                        })
                        .collect(),
                })
            })
            .collect();
        // Only a plan still in the workspace can point at its own source.
        let plan = match self.origin.kind {
            SymbolKind::Definition(index) => ws
                .documents
                .get(&self.origin.path)
                .and_then(|doc| doc.plan_of(index)),
            _ => None,
        };
        let constraints = self
            .constraints
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let source = plan
                    .and_then(|plan| plan.constraints.get(i))
                    .map(|constraint| {
                        let doc = &ws.documents[&self.origin.path];
                        (
                            geometry(doc.line_end(constraint.span.line)),
                            geometry(constraint.span.range(doc)),
                        )
                    });
                ConstraintRecord {
                    result: c,
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
        Err(values::EvalError::UnknownProperty {
            owner: values::PropertyOwner::Plan,
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
    let body = model::plans::seek_body(&def.source).ok_or(EvalError::Message(
        "solve() needs a constraint, e.g. solve(total >= $500)".into(),
    ))?;
    let raw = def.value_span.source(doc);
    let offset = def.value_span.start + raw.len() - raw.trim_start().len();
    let start = offset + def.source.find(body).unwrap_or(0);
    let span = Span::new(def.value_span.line, start, start + body.len()).relative(
        &doc.text,
        0,
        body.len(),
    );
    let vars = [name.clone()].into_iter().collect();
    let (lhs, _, rhs) = engine.constraint(&symbol.path, body, span, &vars)?;
    let difference = lhs.minus(&rhs)?;
    let unit = difference
        .unknown_kind()
        .map(|k| unit_value(k, difference.currency))
        .unwrap_or(Value::Null);
    engine
        .call_module(
            "plan",
            "seek_boundary",
            vec![Value::Text(name), form_record(&difference).to_value(), unit],
        )
        .map_err(|message| engine.fail_at(&symbol.path, span, message))
}

/// One of a form's unit, the value that tells a module what the form counts.
fn unit_value(kind: Unit, currency: Option<crate::engine::Currency>) -> Value {
    match kind {
        Unit::Money => Value::Money(1.0, currency.unwrap_or(crate::engine::Currency::USD)),
        Unit::Duration => Value::Duration(1),
        _ => Value::Number(1.0),
    }
}
fn form_record(form: &Linear) -> FormRecord {
    FormRecord {
        constant: form.constant,
        terms: form.terms.iter().map(|(k, n)| (k.clone(), *n)).collect(),
        unit: unit_value(form.kind, form.currency),
    }
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
        return Err(engine.fail_at(&path, problem.span, message));
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
        return Err(engine.fail_at(&path, plan.objective_span, message));
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
                    model::tables::Domain::Choice => "binary",
                    model::tables::Domain::Count => "integer",
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
        .map_err(|message| engine.fail_at(&path, plan.objective_span, message))?;
    let solution = SolutionRecord::from_value(&result)?;
    let constraints = solution
        .constraints
        .into_iter()
        .map(ConstraintResult::parse_op)
        .collect::<EvalResult<_>>()?;
    let choices = Fields::new(&solution.rows)?;
    let rows = rows
        .into_iter()
        .map(|row| Ok((row.clone(), choices.required::<Value>(&row.name)?)))
        .collect::<EvalResult<_>>()?;
    Ok(Value::Host(std::sync::Arc::new(PlanValue {
        origin: symbol.clone(),
        goal: plan.goal,
        objective: solution.objective,
        variables: solution.variables,
        constraints,
        rows,
    })))
}
record! {
    /// One side of a linear constraint, as `plan.xmd` receives it: a constant,
    /// the coefficients by variable name, and one value carrying the form's unit.
    pub(crate) struct FormRecord {
        pub constant: f64,
        pub terms: BTreeMap<String, f64>,
        pub unit: Value,
    }
}

record! {
    /// One decision column the plan solves for.
    pub(crate) struct DecisionRecord {
        pub name: String,
        pub kind: String,
    }
}

record! {
    /// One named constraint on the way into `plan.solve_model`.
    pub(crate) struct ConstraintInput {
        pub name: String,
        pub lhs: FormRecord,
        pub rhs: FormRecord,
        pub op: String,
    }
}

record! {
    /// The whole model `plan.solve_model` is handed.
    pub(crate) struct PlanInput {
        pub goal: String,
        pub names: Vec<String>,
        pub decisions: Vec<DecisionRecord>,
        pub objective: FormRecord,
        pub constraints: Vec<ConstraintInput>,
    }
}

/// How one constraint fared. `plan.solve_model` reports the comparison in its
/// own words, a `ConstraintResult<String>`; every constraint is read before any
/// comparison is parsed, so a missing field is reported ahead of a bad one.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintResult<Op = Comparison> {
    pub name: String,
    pub op: Op,
    pub lhs: Value,
    pub rhs: Value,
    pub slack: Value,
    pub binding: bool,
}
impl FromValue for ConstraintResult<String> {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            name: fields.required::<Value>("name")?.display(),
            op: fields.required::<Value>("op")?.display(),
            lhs: fields.required("lhs")?,
            rhs: fields.required("rhs")?,
            slack: fields.required("slack")?,
            binding: matches!(fields.required::<Value>("binding")?, Value::Bool(true)),
        })
    }
}
impl ConstraintResult<String> {
    pub(crate) fn parse_op(self) -> EvalResult<ConstraintResult> {
        Ok(ConstraintResult {
            op: self.op.parse().map_err(|_| LINEAR_COMPARISON)?,
            name: self.name,
            lhs: self.lhs,
            rhs: self.rhs,
            slack: self.slack,
            binding: self.binding,
        })
    }
}
impl RecordFields for ConstraintResult {
    fn fields(&self) -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("name".into(), self.name.to_value()),
            ("op".into(), Value::Text(self.op.as_str().into())),
            ("lhs".into(), self.lhs.clone()),
            ("rhs".into(), self.rhs.clone()),
            ("slack".into(), self.slack.clone()),
            ("binding".into(), self.binding.to_value()),
        ])
    }
}
impl ToValue for ConstraintResult {
    fn to_value(&self) -> Value {
        Value::Record(self.fields())
    }
}

/// The solved plan coming back out of `plan.solve_model`.
pub(crate) struct SolutionRecord {
    pub variables: Vec<(String, Value)>,
    pub constraints: Vec<ConstraintResult<String>>,
    /// The chosen cell per decision column, read by row-variable name.
    pub rows: Value,
    pub objective: Value,
}
impl FromValue for SolutionRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::new(value)?;
        Ok(Self {
            variables: fields.required("variables")?,
            constraints: fields.required("constraints")?,
            rows: fields.required("rows")?,
            objective: fields.required("objective")?,
        })
    }
}

record! {
    /// One decision cell a plan filled in, with the source geometry an editor
    /// needs to draw and rewrite it.
    pub(crate) struct CellRecord {
        pub value: Value,
        pub label: String,
        pub document: String,
        pub source: String,
        pub line: usize,
        pub anchor: Value,
        pub range: Value,
        pub width: usize,
    }
}

record! {
    /// One decision column, with the cells the plan chose for it.
    pub(crate) struct ColumnRecord {
        pub name: String,
        pub cells: Vec<CellRecord>,
    }
}

/// A solved constraint on its way to `plan.xmd`, with the source geometry when
/// the plan it came from is still in the workspace.
pub(crate) struct ConstraintRecord<'a> {
    pub result: &'a ConstraintResult,
    pub anchor: Option<Value>,
    pub range: Option<Value>,
}
impl ToValue for ConstraintRecord<'_> {
    fn to_value(&self) -> Value {
        let mut fields = self.result.fields();
        // Only a plan still present in the workspace has a place in the source,
        // and a plan elsewhere has no such keys at all rather than null ones.
        if let Some(anchor) = &self.anchor {
            fields.insert("anchor".into(), anchor.clone());
        }
        if let Some(range) = &self.range {
            fields.insert("range".into(), range.clone());
        }
        Value::Record(fields)
    }
}

/// A whole solved plan, as `plan.xmd` renders it. Presentation policy is the
/// module's; this is the typed result plus where it came from.
pub(crate) struct PlanRecord<'a> {
    pub goal: String,
    pub objective: Value,
    /// Order matters: it is also the variable order the module reports.
    pub variables: Vec<(String, Value)>,
    pub constraints: Vec<ConstraintRecord<'a>>,
    pub columns: Vec<ColumnRecord>,
}
impl ToValue for PlanRecord<'_> {
    fn to_value(&self) -> Value {
        Value::Record(BTreeMap::from([
            ("goal".into(), self.goal.to_value()),
            ("objective".into(), self.objective.clone()),
            (
                "variables".into(),
                Value::Record(self.variables.iter().cloned().collect()),
            ),
            (
                "variable_order".into(),
                Value::List(self.variables.iter().map(|(n, _)| n.to_value()).collect()),
            ),
            ("constraints".into(), self.constraints.to_value()),
            ("columns".into(), self.columns.to_value()),
        ]))
    }
}
