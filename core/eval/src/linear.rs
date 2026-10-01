//! Linearization: an expression read symbolically as `terms · variables +
//! constant` over a set of unknowns, every other name evaluated to a constant
//! and every calculation that reads an unknown walked through. It is how the
//! host reads a form's `linear` and `constraint` expressions, so the module
//! that declares the form (a plan, a goal seek) reasons about values only.
use crate::engine::{BinaryOp, Builtin, Comparison, Currency, Engine, Expr, Parser, RowScope};
use crate::engine::{UnaryOp, Unit, Value};
use crate::workspace::{Symbol, SymbolKind};
use common::Span;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use values::{CurrencyOp, EvalError, EvalResult, UnitOp};
/// `terms · variables + constant`, carrying a unit so money and durations
/// never mix silently.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Linear {
    pub(crate) terms: BTreeMap<String, f64>,
    pub(crate) constant: f64,
    /// Set when `kind` is Money, so euros and dollars never add up silently.
    pub(crate) currency: Option<Currency>,
    /// Unit of the whole form; `Any` while it is only bare variables.
    pub(crate) kind: Unit,
    /// Unit the variables were multiplied by, so a goal seek can tell whether
    /// its unknown is money, a duration, or a plain number.
    pub(crate) scale: Unit,
}
impl Linear {
    fn constant(kind: Unit, value: f64) -> Self {
        Self {
            terms: BTreeMap::new(),
            constant: value,
            currency: None,
            kind,
            scale: Unit::Number,
        }
    }
    fn variable(name: &str) -> Self {
        Self {
            terms: [(name.to_string(), 1.0)].into(),
            ..Self::constant(Unit::Any, 0.0)
        }
    }
    fn is_zero(&self) -> bool {
        self.constant == 0.0 && self.terms.values().all(|c| *c == 0.0)
    }
    /// The shared unit of two forms, treating a bare zero as unitless.
    fn combined(a: &Self, b: &Self) -> Option<Unit> {
        if a.kind == b.kind {
            if a.kind == Unit::Money && a.currency != b.currency && !a.is_zero() && !b.is_zero() {
                return None;
            }
            Some(a.kind)
        } else if a.is_zero() || a.kind == Unit::Any {
            Some(b.kind)
        } else if b.is_zero() || b.kind == Unit::Any {
            Some(a.kind)
        } else {
            None
        }
    }
    /// The unit of a variable in this form, or `None` when it is scaled by
    /// two different units.
    pub(crate) fn unknown_kind(&self) -> Option<Unit> {
        match (self.kind, self.scale) {
            (Unit::Any, _) => Some(Unit::Number),
            (kind, Unit::Number) => Some(kind),
            (kind, scale) if kind == scale => Some(Unit::Number),
            _ => None,
        }
    }
    fn scaled(mut self, factor: f64) -> Self {
        for c in self.terms.values_mut() {
            *c *= factor;
        }
        self.constant *= factor;
        self
    }
    pub(crate) fn add(&self, other: &Self, sign: f64) -> EvalResult<Self> {
        let Some(kind) = Self::combined(self, other) else {
            return Err(match (self.currency, other.currency) {
                (Some(left), Some(right)) => EvalError::currencies(CurrencyOp::Add, left, right),
                _ => mismatch(UnitOp::Add, self.kind, other.kind),
            });
        };
        let mut result = self.clone();
        result.kind = kind;
        result.currency = self.currency.or(other.currency);
        if !self.terms.is_empty() && !other.terms.is_empty() && self.scale != other.scale {
            return Err(mismatch(UnitOp::AddScaled, self.scale, other.scale));
        }
        if self.terms.is_empty() {
            result.scale = other.scale;
        }
        for (name, c) in &other.terms {
            *result.terms.entry(name.clone()).or_insert(0.0) += sign * c;
        }
        result.constant += sign * other.constant;
        Ok(result)
    }
    fn multiply(&self, other: &Self) -> EvalResult<Self> {
        let (form, factor) = if other.terms.is_empty() {
            (self, other)
        } else if self.terms.is_empty() {
            (other, self)
        } else {
            return Err("Plans must stay linear: multiply variables by constants only".into());
        };
        let kind = match (form.kind, factor.kind) {
            (k, Unit::Number) | (Unit::Number, k) => k,
            (Unit::Any, k) => k,
            (left, right) => return Err(mismatch(UnitOp::Multiply, left, right)),
        };
        let mut result = form.clone().scaled(factor.constant);
        result.kind = kind;
        result.currency = form.currency.or(factor.currency);
        if factor.kind != Unit::Number && !form.terms.is_empty() {
            if form.scale != Unit::Number {
                return Err(mismatch(UnitOp::Multiply, form.scale, factor.kind));
            }
            result.scale = factor.kind;
        }
        Ok(result)
    }
    fn divide(&self, other: &Self) -> EvalResult<Self> {
        if !other.terms.is_empty() {
            return Err("Plans must stay linear: divide by constants only".into());
        }
        if other.constant == 0.0 {
            return Err(EvalError::DivisionByZero);
        }
        let kind = match (self.kind, other.kind) {
            (k, Unit::Number) => k,
            (a, b) if a == b => Unit::Number,
            (left, right) => return Err(mismatch(UnitOp::Divide, left, right)),
        };
        let mut result = self.clone().scaled(1.0 / other.constant);
        result.kind = kind;
        if kind != Unit::Money {
            result.currency = None;
        }
        if other.kind != Unit::Number && !self.terms.is_empty() {
            result.scale = if self.scale == other.kind {
                Unit::Number
            } else {
                return Err(mismatch(UnitOp::Divide, self.scale, other.kind));
            };
        }
        Ok(result)
    }
}
/// Why two forms, or a form and a factor, in units `left` and `right`
/// cannot meet under `op`.
fn mismatch(op: UnitOp, left: Unit, right: Unit) -> EvalError {
    EvalError::UnitMismatch { op, left, right }
}
/// The unknowns a linear reading solves for.
pub(crate) type Vars = BTreeSet<String>;
/// One decision cell a linear reading met in a `sum` over its table: an
/// unknown of its own, named `table.column[row]`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowVariable {
    pub(crate) name: String,
    pub(crate) table: Symbol,
    pub(crate) column: usize,
    pub(crate) row: usize,
    pub(crate) domain: document::tables::Domain,
}
impl Engine<'_> {
    /// A linear form over `vars`; every other name is evaluated to a constant.
    pub(crate) fn linear(
        &mut self,
        path: &Path,
        source: &str,
        span: Span,
        vars: &Vars,
    ) -> EvalResult<Linear> {
        self.with_context(path, span, |engine| match Parser::parse(source) {
            Ok(expr) => engine.linear_expr(path, &expr, vars),
            Err(message) => engine.refuse((0, source.len()), EvalError::Message(message)),
        })
    }
    /// `lhs <= rhs`, `lhs >= rhs`, or `lhs == rhs` as two linear forms.
    pub(crate) fn constraint(
        &mut self,
        path: &Path,
        source: &str,
        span: Span,
        vars: &Vars,
    ) -> EvalResult<(Linear, Comparison, Linear)> {
        let whole = (0, source.len());
        self.with_context(path, span, |engine| {
            let expr = match Parser::parse(source) {
                Ok(expr) => expr,
                Err(message) => return engine.refuse(whole, EvalError::Message(message)),
            };
            let Expr::Binary(op, lhs, rhs) = expr.bare() else {
                return engine.refuse(
                    whole,
                    EvalError::from(
                        "A constraint compares two sides with <=, >=, or ==, e.g. bagels >= 12",
                    ),
                );
            };
            let Some(op) = Comparison::from_op(*op) else {
                return engine.refuse(
                    whole,
                    EvalError::from(format!("Constraints use <=, >=, or ==, not {op}")),
                );
            };
            let lhs = engine.linear_expr(path, lhs, vars)?;
            let rhs = engine.linear_expr(path, rhs, vars)?;
            if Linear::combined(&lhs, &rhs).is_none() {
                return engine.refuse(whole, mismatch(UnitOp::Compare, lhs.kind, rhs.kind));
            }
            Ok((lhs, op, rhs))
        })
    }
    /// An ordinary calculation's source, for symbolic descent. Tables, the
    /// definitions forms lay out and literals are opaque and evaluate to
    /// constants instead.
    fn definition_source(&self, path: &Path, name: &str) -> Option<(Symbol, String, Span)> {
        let symbol = self.request.workspace.resolve(path, name).ok()?;
        let SymbolKind::Definition(i) = symbol.kind else {
            return None;
        };
        let doc = &self.request.workspace.documents[&symbol.path];
        let def = &doc.definitions()[i];
        if !def.expression || doc.table_of(i).is_some() || doc.form_of(i).is_some() {
            return None;
        }
        Some((symbol.clone(), def.source.clone(), def.expression_span(doc)))
    }
    fn linear_expr(&mut self, path: &Path, expr: &Expr, vars: &Vars) -> EvalResult<Linear> {
        let constant = |value: Value| -> EvalResult<Linear> {
            if let Value::Duration(s) = value {
                return Ok(Linear::constant(Unit::Duration, s as f64));
            }
            let Some(n) = value.amount() else {
                return Err(format!(
                    "Plans work with numbers, money, and durations, not {}",
                    value.type_name()
                )
                .into());
            };
            Ok(match value.currency() {
                Some(currency) => Linear {
                    currency: Some(currency),
                    ..Linear::constant(Unit::Money, n)
                },
                None => Linear::constant(Unit::Number, n),
            })
        };
        match expr {
            Expr::Spanned(start, end, inner) => {
                let result = self.linear_expr(path, inner, vars);
                self.within((*start, *end), result)
            }
            Expr::Name(n) if vars.contains(n) && self.row().is_none() => Ok(Linear::variable(n)),
            Expr::Name(n)
                if let Some(variable) = self.row().and_then(|row| row.decisions.get(n)) =>
            {
                if variable.is_empty() {
                    return Err(EvalError::DecisionColumnBareTable(n.clone()));
                }
                Ok(Linear::variable(variable))
            }
            // Walk into calculations symbolically, so a form solving for its
            // own name sees it through any chain of definitions.
            Expr::Name(n)
                if self.row().is_none()
                    && !matches!(crate::engine::keyword(n), Some(Value::Bool(_)))
                    && let Some((symbol, source, span)) = self.definition_source(path, n) =>
            {
                if self.trace.linear.contains(&symbol) {
                    self.contextual();
                    return Err(EvalError::CycleThrough { name: n.clone() });
                }
                self.trace.linear.push(symbol.clone());
                let result = self.linear(&symbol.path, &source, span, vars);
                self.trace.linear.pop();
                result
            }
            Expr::Builtin(Builtin::Sum, args) if args.len() == 2 => {
                self.linear_sum(path, args, vars)
            }
            Expr::Unary(op, inner) => {
                let form = self.linear_expr(path, inner, vars)?;
                match op {
                    UnaryOp::Negate => Ok(form.scaled(-1.0)),
                    UnaryOp::Plus => Ok(form),
                    UnaryOp::Not => Err("Plans cannot negate booleans".into()),
                }
            }
            Expr::Binary(op, a, b)
                if matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide
                ) =>
            {
                let a = self.linear_expr(path, a, vars)?;
                let b = self.linear_expr(path, b, vars)?;
                match op {
                    BinaryOp::Add => a.add(&b, 1.0),
                    BinaryOp::Subtract => a.add(&b, -1.0),
                    BinaryOp::Multiply => a.multiply(&b),
                    _ => a.divide(&b),
                }
            }
            Expr::Binary(op, _, _) => Err(format!(
                "'{op}' belongs at the top of a constraint, not inside an expression"
            )
            .into()),
            other => constant(self.expr(path, other)?),
        }
    }
    /// `sum(table, row expression)` as a linear form: decision columns become
    /// one variable per row, other columns are constants.
    fn linear_sum(&mut self, path: &Path, args: &[Expr], vars: &Vars) -> EvalResult<Linear> {
        let (name, table) = self.summed_table(path, &args[0])?;
        let decisions = self.decision_columns(&table);
        let mut total = Linear::constant(Unit::Any, 0.0);
        for (index, values) in table.named_rows().enumerate() {
            let mut names = BTreeMap::new();
            for (column, (c, domain)) in &decisions {
                let variable = format!("{name}.{column}[{}]", index + 1);
                if !self.row_variables.iter().any(|v| v.name == variable) {
                    self.row_variables.push(RowVariable {
                        name: variable.clone(),
                        table: table.origin.clone(),
                        column: *c,
                        row: index,
                        domain: *domain,
                    });
                }
                names.insert(column.clone(), variable);
            }
            self.push_row(RowScope {
                table: name.into(),
                values,
                decisions: names,
            });
            let form = self.linear_expr(path, &args[1], vars);
            self.pop_row();
            total = total.add(&form?, 1.0)?;
        }
        Ok(total)
    }
}
