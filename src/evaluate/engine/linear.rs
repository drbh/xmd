//! Linearization: a definition read symbolically as `terms · variables +
//! constant`, so plans and goal seeks can reason about an unknown.
use super::{BinaryOp, Comparison, Currency, Engine, Expr, Parser, RowScope, UnaryOp, Value};
use crate::{
    document::Span,
    workspace::{Symbol, SymbolKind},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
/// The unit a linear form carries, so money, durations and plain numbers never
/// mix silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Not yet known: the form is only bare variables.
    Any,
    Number,
    Money,
    Duration,
}
impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Number => "Number",
            Self::Money => "Money",
            Self::Duration => "Duration",
        }
    }
}
impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// `terms · variables + constant`, carrying a unit so money and durations
/// never mix silently.
#[derive(Clone, Debug, PartialEq)]
pub struct Linear {
    pub terms: BTreeMap<String, f64>,
    pub constant: f64,
    /// Set when `kind` is Money, so euros and dollars never add up silently.
    pub currency: Option<Currency>,
    /// Unit of the whole form; `Any` while it is only bare variables.
    pub kind: Unit,
    /// Unit the variables were multiplied by, so a goal seek can tell whether
    /// its unknown is money, a duration, or a plain number.
    pub scale: Unit,
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
            constant: 0.0,
            currency: None,
            kind: Unit::Any,
            scale: Unit::Number,
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
    pub fn minus(&self, other: &Self) -> Result<Self, String> {
        self.add(other, -1.0)
    }
    /// The unit of a variable in this form, or `None` when it is scaled by
    /// two different units.
    pub fn unknown_kind(&self) -> Option<Unit> {
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
    fn add(&self, other: &Self, sign: f64) -> Result<Self, String> {
        let kind = Self::combined(self, other).ok_or_else(|| {
            if let (Some(a), Some(b)) = (self.currency, other.currency) {
                format!("Cannot add {a} and {b}; convert with to(value, {b})")
            } else {
                format!("Cannot add {} and {}", self.kind, other.kind)
            }
        })?;
        let mut result = self.clone();
        result.kind = kind;
        result.currency = self.currency.or(other.currency);
        if !self.terms.is_empty() && !other.terms.is_empty() && self.scale != other.scale {
            return Err(format!(
                "Cannot add terms scaled by {} and {}",
                self.scale, other.scale
            ));
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
    fn multiply(&self, other: &Self) -> Result<Self, String> {
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
            (a, b) => return Err(format!("Cannot multiply {a} by {b}")),
        };
        let mut result = form.clone().scaled(factor.constant);
        result.kind = kind;
        result.currency = form.currency.or(factor.currency);
        if factor.kind != Unit::Number && !form.terms.is_empty() {
            if form.scale != Unit::Number {
                return Err(format!("Cannot multiply {} by {}", form.scale, factor.kind));
            }
            result.scale = factor.kind;
        }
        Ok(result)
    }
    fn divide(&self, other: &Self) -> Result<Self, String> {
        if !other.terms.is_empty() {
            return Err("Plans must stay linear: divide by constants only".into());
        }
        if other.constant == 0.0 {
            return Err("Division by zero".into());
        }
        let kind = match (self.kind, other.kind) {
            (k, Unit::Number) => k,
            (a, b) if a == b => Unit::Number,
            (a, b) => return Err(format!("Cannot divide {a} by {b}")),
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
                return Err(format!("Cannot divide {} by {}", self.scale, other.kind));
            };
        }
        Ok(result)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct RowVariable {
    pub name: String,
    pub table: Symbol,
    pub column: usize,
    pub row: usize,
    pub domain: crate::tables::Domain,
}
impl Engine<'_> {
    /// A linear form over `vars`; every other name is evaluated to a constant.
    pub fn linear(
        &mut self,
        path: &Path,
        source: &str,
        span: Span,
        vars: &BTreeSet<String>,
    ) -> Result<Linear, String> {
        self.contexts.push((path.into(), span));
        let result = match Parser::parse(source) {
            Ok(expr) => self.linear_expr(path, &expr, vars),
            Err(message) => {
                self.fail((0, source.len()), &message);
                Err(message)
            }
        };
        self.contexts.pop();
        result
    }
    /// `lhs <= rhs`, `lhs >= rhs`, or `lhs == rhs` as two linear forms.
    pub fn constraint(
        &mut self,
        path: &Path,
        source: &str,
        span: Span,
        vars: &BTreeSet<String>,
    ) -> Result<(Linear, Comparison, Linear), String> {
        self.contexts.push((path.into(), span));
        let result = (|| {
            let expr = Parser::parse(source).inspect_err(|m| self.fail((0, source.len()), m))?;
            let Expr::Binary(op, lhs, rhs) = expr.bare() else {
                let message =
                    "A constraint compares two sides with <=, >=, or ==, e.g. bagels >= 12";
                self.fail((0, source.len()), message);
                return Err(message.into());
            };
            let Some(op) = Comparison::from_op(*op) else {
                let message = format!("Constraints use <=, >=, or ==, not {op}");
                self.fail((0, source.len()), &message);
                return Err(message);
            };
            let lhs = self.linear_expr(path, lhs, vars)?;
            let rhs = self.linear_expr(path, rhs, vars)?;
            if Linear::combined(&lhs, &rhs).is_none() {
                let message = format!("Cannot compare {} with {}", lhs.kind, rhs.kind);
                self.fail((0, source.len()), &message);
                return Err(message);
            }
            Ok((lhs, op, rhs))
        })();
        self.contexts.pop();
        result
    }
    /// An ordinary calculation's source, for symbolic descent. Tables, plans,
    /// goal seeks and literals are opaque and evaluate to constants instead.
    fn definition_source(&self, path: &Path, name: &str) -> Option<(Symbol, String, Span)> {
        let symbol = self.workspace.resolve(path, name).ok()?;
        let SymbolKind::Definition(i) = symbol.kind else {
            return None;
        };
        let doc = &self.workspace.documents[&symbol.path];
        let def = &doc.definitions[i];
        if !def.expression
            || doc.tables.iter().any(|t| t.definition == i)
            || doc.plans.iter().any(|p| p.definition == i)
            || crate::plans::seek_body(&def.source).is_some()
            || crate::plans::goal(&def.source).is_some()
        {
            return None;
        }
        let raw = def.value_span.source(&doc.text);
        let offset = raw.len() - raw.trim_start().len();
        Some((
            symbol.clone(),
            def.source.clone(),
            Span::new(
                def.value_span.line,
                def.value_span.start + offset,
                def.value_span.end,
            ),
        ))
    }
    fn linear_expr(
        &mut self,
        path: &Path,
        expr: &Expr,
        vars: &BTreeSet<String>,
    ) -> Result<Linear, String> {
        let constant = |value: Value| -> Result<Linear, String> {
            match value {
                Value::Number(n) | Value::Ratio(n) => Ok(Linear::constant(Unit::Number, n)),
                Value::Count(n) => Ok(Linear::constant(Unit::Number, n as f64)),
                Value::Money(n, currency) => {
                    let mut form = Linear::constant(Unit::Money, n);
                    form.currency = Some(currency);
                    Ok(form)
                }
                Value::Duration(s) => Ok(Linear::constant(Unit::Duration, s as f64)),
                other => Err(format!(
                    "Plans work with numbers, money, and durations, not {}",
                    other.type_name()
                )),
            }
        };
        match expr {
            Expr::Spanned(start, end, inner) => {
                let result = self.linear_expr(path, inner, vars);
                if let Err(message) = &result {
                    self.fail((*start, *end), message);
                }
                result
            }
            Expr::Name(n) if vars.contains(n) && self.row_values.is_empty() => {
                Ok(Linear::variable(n))
            }
            Expr::Name(n)
                if self
                    .row_values
                    .last()
                    .is_some_and(|scope| scope.decisions.contains_key(n)) =>
            {
                let variable = self.row_values.last().unwrap().decisions[n].clone();
                if variable.is_empty() {
                    return Err(format!("'{n}' is a decision column; use it inside a plan"));
                }
                Ok(Linear::variable(&variable))
            }
            // Walk into calculations symbolically, so a goal seek can see its
            // own name through any chain of definitions.
            Expr::Name(n)
                if self.row_values.is_empty()
                    && !matches!(n.as_str(), "true" | "false")
                    && self.definition_source(path, n).is_some() =>
            {
                let (symbol, source, span) = self.definition_source(path, n).unwrap();
                if self.linear_stack.contains(&symbol) {
                    return Err(format!("Dependency cycle through {n}"));
                }
                self.linear_stack.push(symbol.clone());
                let result = self.linear(&symbol.path, &source, span, vars);
                self.linear_stack.pop();
                result
            }
            Expr::Call(n, args) if n == "sum" && args.len() == 2 => {
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
            )),
            other => constant(self.expr(path, other)?),
        }
    }
    /// Decision columns of a table value: column name to (index, domain).
    pub(super) fn decision_columns(
        &self,
        table: &crate::tables::TableValue,
    ) -> BTreeMap<String, (usize, crate::tables::Domain)> {
        crate::tables::table(self.workspace, &table.origin)
            .map(|t| {
                t.domains
                    .iter()
                    .enumerate()
                    .filter_map(|(i, d)| d.map(|d| (t.columns[i].name.clone(), (i, d))))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// `sum(table, row expression)` as a linear form: decision columns become
    /// one variable per row, other columns are constants.
    fn linear_sum(
        &mut self,
        path: &Path,
        args: &[Expr],
        vars: &BTreeSet<String>,
    ) -> Result<Linear, String> {
        let Some(Expr::Name(name)) = args.first().map(Expr::bare) else {
            return Err("The first argument to sum must be a table name".into());
        };
        let Value::Table(table) = self.named(path, name)? else {
            return Err(format!("'{name}' is not a table"));
        };
        let decisions = self.decision_columns(&table);
        let mut total = Linear::constant(Unit::Any, 0.0);
        for (index, row) in table.rows.iter().enumerate() {
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
            self.row_values.push(RowScope {
                table: name.clone(),
                values: table
                    .columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect(),
                decisions: names,
            });
            let form = self.linear_expr(path, &args[1], vars);
            self.row_values.pop();
            total = total.add(&form?, 1.0)?;
        }
        Ok(total)
    }
}
