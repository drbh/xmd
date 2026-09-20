//! Definition, row and decision records: values, plans and tables.
use super::{
    Collection, QueryValue, RecordKind,
    record::{Base, EXPRESSION, Fields, LazyField, NAME, Record, projected},
};
use crate::{
    document::Document,
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use std::{collections::BTreeMap, path::Path};

/// Values, plans and tables are one definition each. Their value, type, display
/// and (for a plan) solution stay null until the record is evaluated.
#[derive(Clone, Debug)]
pub(super) struct DefinitionRecord {
    base: Base,
    name: String,
    expression: String,
    computed: bool,
    solution: bool,
}
impl DefinitionRecord {
    pub(super) const FIELDS: [&'static str; 6] = [
        NAME,
        EXPRESSION,
        LazyField::Value.as_str(),
        LazyField::Type.as_str(),
        LazyField::Display.as_str(),
        "computed",
    ];
    /// Only a plan carries a solution, so it is not part of every definition.
    pub(super) const SOLUTION: [&'static str; 1] = [LazyField::Solution.as_str()];
}
impl Fields for DefinitionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(
            DefinitionRecord::FIELDS,
            [
                QueryValue::text(self.name),
                QueryValue::text(self.expression),
                QueryValue::Null,
                QueryValue::Null,
                QueryValue::Null,
                QueryValue::boolean(self.computed),
            ],
        ));
        if self.solution {
            fields.extend(projected(DefinitionRecord::SOLUTION, [QueryValue::Null]));
        }
        fields
    }
}

#[derive(Clone, Debug)]
pub(super) struct RowRecord {
    base: Base,
    table: String,
    cells: BTreeMap<String, QueryValue>,
}
impl RowRecord {
    pub(super) const FIELDS: [&'static str; 2] = ["table", "cells"];
}
impl Fields for RowRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(
            RowRecord::FIELDS,
            [QueryValue::text(self.table), QueryValue::Object(self.cells)],
        ));
        fields
    }
}

#[derive(Clone, Debug)]
pub(super) struct DecisionRecord {
    base: Base,
    value: QueryValue,
    plan: String,
}
impl DecisionRecord {
    pub(super) const FIELDS: [&'static str; 2] = [LazyField::Value.as_str(), "plan"];
}
impl Fields for DecisionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(
            DecisionRecord::FIELDS,
            [self.value, QueryValue::text(self.plan)],
        ));
        fields
    }
}

pub(super) fn decisions(
    ws: &Workspace,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
    records: &mut Vec<Record>,
) {
    for (plan_path, doc) in &ws.documents {
        for plan in &doc.plans {
            let symbol = Symbol {
                path: plan_path.clone(),
                kind: SymbolKind::Definition(plan.definition),
            };
            let Ok(Value::Plan(value)) = engine.symbol(&symbol) else {
                continue;
            };
            for (row, value) in &value.rows {
                if only.is_some_and(|path| path != row.table.path) {
                    continue;
                }
                let Some(cell) = crate::tables::table(ws, &row.table)
                    .and_then(|t| t.rows.get(row.row))
                    .and_then(|r| r.get(row.column))
                else {
                    continue;
                };
                let name = &doc.definitions[plan.definition].named.name;
                let mut base = Base::new(
                    ws,
                    &row.table.path,
                    cell.span.line,
                    RecordKind::Decision,
                    name,
                );
                base.anchor = cell.span.range(&ws.documents[&row.table.path].text).end;
                records.push(Record::typed(
                    &row.table.path,
                    DecisionRecord {
                        base,
                        value: QueryValue::from_value(value.clone()),
                        plan: name.clone(),
                    },
                ));
            }
        }
    }
}

/// Values, plans, tables and table rows all project one definition.
pub(super) fn definitions(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    collection: Collection,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) -> Result<(), String> {
    for (i, def) in doc.definitions.iter().enumerate() {
        let plan = doc.plans.iter().any(|p| p.definition == i);
        let table = doc.tables.iter().find(|t| t.definition == i);
        if (collection == Collection::Plans && !plan)
            || (matches!(collection, Collection::Tables | Collection::Rows) && table.is_none())
        {
            continue;
        }
        let symbol = Symbol {
            path: path.to_path_buf(),
            kind: SymbolKind::Definition(i),
        };
        if collection == Collection::Rows {
            match engine.symbol(&symbol) {
                Ok(Value::Table(t)) => {
                    for (row, values) in t.rows.iter().enumerate() {
                        records.push(Record::typed(
                            path,
                            RowRecord {
                                base: Base::new(
                                    ws,
                                    path,
                                    table.unwrap().rows[row][0].span.line,
                                    RecordKind::Row,
                                    &def.named.name,
                                ),
                                table: def.named.name.clone(),
                                cells: t
                                    .columns
                                    .iter()
                                    .cloned()
                                    .zip(values.iter().cloned().map(QueryValue::from_value))
                                    .collect(),
                            },
                        ));
                    }
                }
                Err(e) => {
                    return Err(format!(
                        "{}:{}: {e}",
                        path.display(),
                        def.named.span.line + 1
                    ));
                }
                _ => return Err("Expected a table".into()),
            }
            continue;
        }
        let mut base = Base::new(
            ws,
            path,
            def.named.span.line,
            if plan {
                RecordKind::Plan
            } else if table.is_some() {
                RecordKind::Table
            } else {
                RecordKind::Value
            },
            &def.named.name,
        );
        base.anchor = def.end.range(&doc.text).end;
        let mut r = Record::typed(
            path,
            DefinitionRecord {
                base,
                name: def.named.name.clone(),
                expression: def.source.clone(),
                computed: def.expression,
                solution: plan,
            },
        );
        r.deferred = Some(symbol);
        records.push(r);
    }
    Ok(())
}
