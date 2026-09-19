//! Definition, row and decision records: values, plans and tables.
use super::{
    Collection, QueryValue, RecordKind,
    record::{Base, EXPRESSION, Fields, LazyField, NAME, Record, entries},
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
struct DefinitionRecord {
    base: Base,
    name: String,
    expression: String,
    computed: bool,
    solution: bool,
}
impl Fields for DefinitionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            (NAME, QueryValue::text(self.name)),
            (EXPRESSION, QueryValue::text(self.expression)),
            (LazyField::Value.as_str(), QueryValue::Null),
            (LazyField::Type.as_str(), QueryValue::Null),
            (LazyField::Display.as_str(), QueryValue::Null),
            ("computed", QueryValue::boolean(self.computed)),
        ]));
        if self.solution {
            fields.insert(LazyField::Solution.as_str().into(), QueryValue::Null);
        }
        fields
    }
}

#[derive(Clone, Debug)]
struct RowRecord {
    base: Base,
    table: String,
    cells: BTreeMap<String, QueryValue>,
}
impl Fields for RowRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("table", QueryValue::text(self.table)),
            ("cells", QueryValue::Object(self.cells)),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct DecisionRecord {
    base: Base,
    value: QueryValue,
    plan: String,
}
impl Fields for DecisionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            (LazyField::Value.as_str(), self.value),
            ("plan", QueryValue::text(self.plan)),
        ]));
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
