//! The numerical boundary: a bounded, unit-free linear model in, raw values out.
use crate::{
    engine::Value,
    plugins::{from_json, json},
};
use good_lp::{Expression, ProblemVariables, ResolutionError, Solution, SolverModel, variable};
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variable {
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
    op: String,
    rhs: Form,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Model {
    goal: String,
    variables: BTreeMap<String, Variable>,
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
    for (name, v) in &model.variables {
        let mut definition = match v.kind.as_str() {
            "continuous" => variable(),
            "integer" => variable().integer(),
            "binary" => variable().binary(),
            _ => return Err(format!("Unknown variable kind '{}'", v.kind)),
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
        solver = solver.with(match constraint.op.as_str() {
            "<=" => lhs.leq(rhs),
            ">=" => lhs.geq(rhs),
            "==" => lhs.eq(rhs),
            _ => return Err("Linear comparison must be <=, >=, or ==".into()),
        });
    }
    let result = match solver.solve() {
        Ok(solution) => {
            serde_json::json!({"status":"optimal","values":variables.iter().map(|(name,var)|(name.clone(),solution.value(*var))).collect::<BTreeMap<_,_>>() })
        }
        Err(ResolutionError::Infeasible) => serde_json::json!({"status":"infeasible"}),
        Err(ResolutionError::Unbounded) => serde_json::json!({"status":"unbounded"}),
        Err(e) => return Err(format!("Solver error: {e}")),
    };
    Ok(from_json(&result))
}
