//! The numerical boundary: a bounded, unit-free linear model in, raw values out.
use crate::error::{EvalError, EvalResult};
use crate::{
    records::FromValue,
    value::{Value, from_json, json},
};
use good_lp::{Expression, ProblemVariables, ResolutionError, Solution, SolverModel, variable};
use serde::Deserialize;
use std::collections::BTreeMap;
use syntax::Comparison;

pub const LINEAR_COMPARISON: &str = "Linear comparison must be <=, >=, or ==";
/// What a solver may choose for one variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum VariableKind {
    Continuous,
    Integer,
    Binary,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variable {
    /// The record's own spelling; `VariableKind` gives it meaning.
    kind: String,
    #[serde(default)]
    lower: Option<f64>,
    #[serde(default)]
    upper: Option<f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Form {
    constant: f64,
    terms: BTreeMap<String, f64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Constraint {
    lhs: Form,
    /// The record's own spelling; `Comparison` gives it meaning.
    op: String,
    rhs: Form,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    goal: String,
    variables: BTreeMap<String, Variable>,
    #[serde(default)]
    order: Option<Vec<String>>,
    objective: Form,
    constraints: Vec<Constraint>,
}
/// A model is the one record on this boundary that is pure JSON all the way
/// down: numbers, text and nested records, with no unit or timestamp in it.
/// `serde` is therefore the decoder, and the derived field names are the
/// pinned ones; `FromValue` only puts it on the same footing as the rest.
impl FromValue for Model {
    fn from_value(value: &Value) -> EvalResult<Self> {
        serde_json::from_value(json(value)?)
            .map_err(|e| EvalError::Message(format!("Invalid linear model: {e}")))
    }
}
pub(crate) fn solve(value: &Value) -> EvalResult<Value> {
    let model = Model::from_value(value)?;
    if model.variables.is_empty() || model.variables.len() > 512 || model.constraints.len() > 2048 {
        return Err(EvalError::Message(
            "Linear models require 1..512 variables and at most 2048 constraints".into(),
        ));
    }
    let mut problem = ProblemVariables::new();
    let mut variables = BTreeMap::new();
    let order = model
        .order
        .unwrap_or_else(|| model.variables.keys().cloned().collect());
    let ordered: std::collections::BTreeSet<_> = order.iter().collect();
    if order.len() != model.variables.len()
        || ordered.len() != order.len()
        || order.iter().any(|n| !model.variables.contains_key(n))
    {
        return Err(EvalError::Message(
            "Variable order must name every variable exactly once".into(),
        ));
    }
    for name in &order {
        let v = &model.variables[name];
        let mut definition = match v
            .kind
            .parse()
            .map_err(|_| format!("Unknown variable kind '{}'", v.kind))?
        {
            VariableKind::Continuous => variable(),
            VariableKind::Integer => variable().integer(),
            VariableKind::Binary => variable().binary(),
        };
        if v.lower.zip(v.upper).is_some_and(|(l, u)| l > u) {
            return Err(EvalError::Message(format!("Reversed bounds for '{name}'")));
        }
        if let Some(n) = v.lower {
            definition = definition.min(n);
        }
        if let Some(n) = v.upper {
            definition = definition.max(n);
        }
        variables.insert(name.clone(), problem.add(definition));
    }
    let expression = |form: &Form| -> EvalResult<Expression> {
        let mut expression = Expression::from(form.constant);
        for (name, coefficient) in &form.terms {
            expression += *coefficient
                * *variables.get(name).ok_or_else(|| {
                    EvalError::Message(format!("Unknown linear variable '{name}'"))
                })?;
        }
        Ok(expression)
    };
    let objective = expression(&model.objective)?;
    let mut solver = match model.goal.as_str() {
        "maximize" => problem.maximise(objective),
        "minimize" => problem.minimise(objective),
        _ => {
            return Err(EvalError::Message(
                "Linear goal must be maximize or minimize".into(),
            ));
        }
    }
    .using(good_lp::microlp);
    for constraint in &model.constraints {
        let lhs = expression(&constraint.lhs)?;
        let rhs = expression(&constraint.rhs)?;
        solver = solver.with(
            match constraint.op.parse().map_err(|_| LINEAR_COMPARISON)? {
                Comparison::LessEqual => lhs.leq(rhs),
                Comparison::GreaterEqual => lhs.geq(rhs),
                Comparison::Equal => lhs.eq(rhs),
            },
        );
    }
    let result = match solver.solve() {
        Ok(solution) => {
            let values: BTreeMap<_, _> = variables
                .iter()
                .map(|(name, var)| (name.clone(), solution.value(*var)))
                .collect();
            if values.values().any(|v| !v.is_finite()) {
                return Err(EvalError::Message(
                    "Solver returned a nonfinite value".into(),
                ));
            }
            serde_json::json!({"status":"optimal","values":values})
        }
        Err(ResolutionError::Infeasible) => serde_json::json!({"status":"infeasible"}),
        Err(ResolutionError::Unbounded) => serde_json::json!({"status":"unbounded"}),
        Err(e) => return Err(EvalError::Message(format!("Solver error: {e}"))),
    };
    Ok(from_json(&result))
}
