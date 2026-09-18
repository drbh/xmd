//! The numerical boundary: a bounded, unit-free linear model in, raw values out.
use crate::{
    engine::{Comparison, Value},
    modules::{from_json, json},
};
use good_lp::{Expression, ProblemVariables, ResolutionError, Solution, SolverModel, variable};
use serde::Deserialize;
use std::collections::BTreeMap;
/// What a solver may choose for one variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
    Continuous,
    Integer,
    Binary,
}
impl VariableKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Integer => "integer",
            Self::Binary => "binary",
        }
    }
}
impl std::str::FromStr for VariableKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "continuous" => Ok(Self::Continuous),
            "integer" => Ok(Self::Integer),
            "binary" => Ok(Self::Binary),
            _ => Err(format!("Unknown variable kind '{s}'")),
        }
    }
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
pub fn solve(value: &Value) -> Result<Value, String> {
    let model: Model =
        serde_json::from_value(json(value)?).map_err(|e| format!("Invalid linear model: {e}"))?;
    if model.variables.is_empty() || model.variables.len() > 512 || model.constraints.len() > 2048 {
        return Err("Linear models require 1..512 variables and at most 2048 constraints".into());
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
        return Err("Variable order must name every variable exactly once".into());
    }
    for name in &order {
        let v = &model.variables[name];
        let mut definition = match v.kind.parse()? {
            VariableKind::Continuous => variable(),
            VariableKind::Integer => variable().integer(),
            VariableKind::Binary => variable().binary(),
        };
        if v.lower.zip(v.upper).is_some_and(|(l, u)| l > u) {
            return Err(format!("Reversed bounds for '{name}'"));
        }
        if let Some(n) = v.lower {
            definition = definition.min(n);
        }
        if let Some(n) = v.upper {
            definition = definition.max(n);
        }
        variables.insert(name.clone(), problem.add(definition));
    }
    let expression = |form: &Form| -> Result<Expression, String> {
        let mut expression = Expression::from(form.constant);
        for (name, coefficient) in &form.terms {
            expression += *coefficient
                * *variables
                    .get(name)
                    .ok_or_else(|| format!("Unknown linear variable '{name}'"))?;
        }
        Ok(expression)
    };
    let objective = expression(&model.objective)?;
    let mut solver = match model.goal.as_str() {
        "maximize" => problem.maximise(objective),
        "minimize" => problem.minimise(objective),
        _ => return Err("Linear goal must be maximize or minimize".into()),
    }
    .using(good_lp::microlp);
    for constraint in &model.constraints {
        let lhs = expression(&constraint.lhs)?;
        let rhs = expression(&constraint.rhs)?;
        solver = solver.with(match constraint.op.parse()? {
            Comparison::LessEqual => lhs.leq(rhs),
            Comparison::GreaterEqual => lhs.geq(rhs),
            Comparison::Equal => lhs.eq(rhs),
        });
    }
    let result = match solver.solve() {
        Ok(solution) => {
            let values: BTreeMap<_, _> = variables
                .iter()
                .map(|(name, var)| (name.clone(), solution.value(*var)))
                .collect();
            if values.values().any(|v| !v.is_finite()) {
                return Err("Solver returned a nonfinite value".into());
            }
            serde_json::json!({"status":"optimal","values":values})
        }
        Err(ResolutionError::Infeasible) => serde_json::json!({"status":"infeasible"}),
        Err(ResolutionError::Unbounded) => serde_json::json!({"status":"unbounded"}),
        Err(e) => return Err(format!("Solver error: {e}")),
    };
    Ok(from_json(&result))
}
