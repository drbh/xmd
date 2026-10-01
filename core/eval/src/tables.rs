//! Reading a table against a live workspace: resolving a column reference,
//! finding a table by name through an alias chain, checking a rename, and the
//! `TableValue` a note holds once its cells are evaluated. Parsing, typing and
//! formatting a table's shape is `document::tables`, one layer down.
use crate::{
    engine::{BinaryOp, Builtin, Engine, Expr, Parser, RowScope, Value, binary},
    workspace::{Symbol, SymbolKind, Workspace},
};
use document::{
    Reference,
    tables::{Cell, Table, scope_at},
};
use std::{collections::BTreeMap, path::Path};
use values::{EvalError, EvalResult};

#[derive(Clone, Debug, PartialEq)]
pub struct TableValue {
    pub origin: Symbol,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}
impl TableValue {
    /// Each row as its column names and cell values.
    pub(crate) fn named_rows(&self) -> impl Iterator<Item = BTreeMap<String, Value>> + '_ {
        self.rows.iter().map(|row| {
            self.columns
                .iter()
                .cloned()
                .zip(row.iter().cloned())
                .collect()
        })
    }
}

pub fn origin(ws: &Workspace, path: &Path, name: &str) -> EvalResult<Symbol> {
    let mut symbol = ws.resolve(path, name)?;
    for _ in 0..64 {
        let doc = &ws.documents[&symbol.path];
        if let SymbolKind::Definition(index) = symbol.kind {
            if doc.table_of(index).is_some() {
                return Ok(symbol);
            }
            let def = &doc.definitions[index];
            if def.expression
                && let Some(alias) = crate::imports::member_symbol(ws, &symbol.path, &def.source)
            {
                symbol = alias;
                continue;
            }
        }
        return Err(EvalError::NotATable(name.into()));
    }
    Err(EvalError::Message(
        "Table alias chain is cyclic or too deep".into(),
    ))
}
pub fn table<'a>(ws: &'a Workspace, symbol: &Symbol) -> Option<&'a Table> {
    if let SymbolKind::Definition(index) = symbol.kind {
        ws.documents[&symbol.path].table_of(index)
    } else {
        None
    }
}

pub fn resolve_reference(ws: &Workspace, path: &Path, reference: &Reference) -> EvalResult<Symbol> {
    let Some(name) = scope_at(&ws.documents[path], reference.span) else {
        return ws.resolve(path, &reference.name);
    };
    let target = origin(ws, path, &name)?;
    let doc = &ws.documents[&target.path];
    let (index, table) = doc
        .tables
        .iter()
        .enumerate()
        .find(|(_, t)| matches!(target.kind, SymbolKind::Definition(i) if i == t.definition))
        .unwrap();
    let matches: Vec<_> = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name == reference.name)
        .collect();
    match matches.as_slice() {
        [(column, _)] => Ok(Symbol::new(target.path, SymbolKind::Column(index, *column))),
        [] => Err(EvalError::UnknownColumn {
            name: reference.name.clone(),
            table: name,
        }),
        _ => Err(EvalError::Message(format!(
            "Ambiguous column '{}' in table '{name}'",
            reference.name
        ))),
    }
}
pub fn validate_rename(ws: &Workspace, symbol: &Symbol, name: &str) -> Result<(), String> {
    if !syntax::identifier(name) || matches!(name, "true" | "false") {
        return Err("Use an identifier other than true or false".into());
    }
    let conflict = if let SymbolKind::Column(t, column) = symbol.kind {
        ws.documents[&symbol.path].tables[t]
            .columns
            .iter()
            .enumerate()
            .any(|(i, c)| i != column && c.name == name)
    } else {
        ws.symbols()
            .iter()
            .any(|s| s.path == symbol.path && s != symbol && ws.named(s).name == name)
    };
    if conflict {
        Err("That name already exists in this scope".into())
    } else {
        Ok(())
    }
}

/// A plain cell's value, or why its text is not one.
pub fn literal_value(cell: &Cell) -> EvalResult<Value> {
    cell.value
        .clone()
        .map(Value::from)
        .map_err(EvalError::Message)
}

/// A table definition's value, as `features` registers it for
/// [`document::DefinitionKind::Table`]: the first problem parsing found, or its
/// rows.
pub(crate) fn evaluate(
    engine: &mut Engine<'_>,
    symbol: &Symbol,
    index: usize,
) -> EvalResult<Value> {
    let Some(table) = engine.workspace().documents[&symbol.path].table_of(index) else {
        return Err(EvalError::NotATable(
            engine.workspace().named(symbol).name.clone(),
        ));
    };
    if let Some(problem) = table.problems.first() {
        let message = EvalError::Message(problem.message.clone());
        Err(engine.fail_at(&symbol.path, problem.span, message))
    } else {
        table_value(engine, symbol, table)
    }
}

/// Rows of a table, evaluating calculated cells and checking that each column
/// keeps one type. Failures point at the offending cell.
fn table_value(engine: &mut Engine<'_>, symbol: &Symbol, table: &Table) -> EvalResult<Value> {
    use crate::engine::ValueType;
    let mut types: Vec<Option<ValueType>> = table.types.clone();
    let mut rows = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut values = Vec::with_capacity(row.len());
        for (column, cell) in row.iter().enumerate() {
            let value = match &cell.expression {
                Some((inner, span)) => {
                    let value = engine.eval_at(&symbol.path, inner, *span)?;
                    if matches!(value, Value::Host(_) | Value::Tasks(_)) {
                        let message = EvalError::Message(format!(
                            "A cell cannot hold a {}; use a scalar value",
                            value.type_name()
                        ));
                        return Err(engine.fail_at(&symbol.path, *span, message));
                    }
                    if let Some(expected) = types.get(column).copied().flatten() {
                        if expected != value.kind() {
                            let message = EvalError::Message(format!(
                                "Column '{}' expects {expected}, found {}",
                                table.columns[column].name,
                                value.type_name()
                            ));
                            return Err(engine.fail_at(&symbol.path, *span, message));
                        }
                    } else if let Some(slot) = types.get_mut(column) {
                        *slot = Some(value.kind());
                    }
                    value
                }
                None => literal_value(cell)?,
            };
            values.push(value);
        }
        rows.push(values);
    }
    Ok(Value::Host(std::sync::Arc::new(TableValue {
        origin: symbol.clone(),
        columns: table.columns.iter().map(|c| c.name.clone()).collect(),
        rows,
    })))
}

/// `sum(…)` with other than a single list: the row sum over a table, as
/// `features` registers it for [`Builtin::Sum`].
pub(crate) fn call_sum(engine: &mut Engine<'_>, path: &Path, args: &[Expr]) -> EvalResult<Value> {
    engine.sum(path, args).map(|(value, _)| value)
}

impl Engine<'_> {
    /// What each row adds to a `sum(table, row expression)`, or `None` when
    /// `source` is not one or does not evaluate.
    pub fn sum_contributions(&mut self, path: &Path, source: &str) -> Option<Vec<Value>> {
        let parsed = Parser::parse(source).ok()?;
        let Expr::Builtin(Builtin::Sum, args) = parsed.bare() else {
            return None;
        };
        self.sum(path, args).ok().map(|(_, rows)| rows)
    }
    /// The table a row `sum` walks, named by its first argument.
    pub(crate) fn summed_table<'e>(
        &mut self,
        path: &Path,
        first: &'e Expr,
    ) -> EvalResult<(&'e str, std::sync::Arc<TableValue>)> {
        let Some(name) = first.as_name() else {
            return Err(EvalError::Message(
                "The first argument to sum must be a table name".into(),
            ));
        };
        let Some(table) = self.named(path, name)?.downcast_arc::<TableValue>() else {
            return Err(EvalError::NotATable(name.into()));
        };
        Ok((name, table))
    }
    /// Decision columns of a table value: column name to (index, domain).
    pub(crate) fn decision_columns(
        &self,
        table: &TableValue,
    ) -> BTreeMap<String, (usize, document::tables::Domain)> {
        self::table(self.workspace(), &table.origin)
            .map(|t| {
                t.domains
                    .iter()
                    .enumerate()
                    .filter_map(|(i, d)| d.map(|d| (t.columns[i].name.clone(), (i, d))))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// `sum(table, row expression)`: the total, and what each row added.
    fn sum(&mut self, path: &Path, args: &[Expr]) -> EvalResult<(Value, Vec<Value>)> {
        if args.len() != 2 {
            return Err(EvalError::Message(
                "sum expects a table and a row expression: sum(groceries, quantity * price)".into(),
            ));
        }
        let (name, table) = self.summed_table(path, &args[0])?;
        let mut total = None;
        let mut contributions = Vec::new();
        let decisions = self.decision_columns(&table);
        for values in table.named_rows() {
            self.push_row(RowScope {
                table: name.into(),
                values,
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
}
