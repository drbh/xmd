//! Definition, row and decision records: values, plans and tables.
use super::value as q;
use super::{
    Collection, RecordKind,
    record::{Base, EXPRESSION, LazyField, NAME, Record},
};
use lang::eval::engine::{Engine, Value};
use lang::eval::{RecordFields, record};
use lang::eval::{Symbol, SymbolKind, Workspace};
use lang::model::Document;
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
/// Written out rather than declared with `record!`: the value, type and
/// display fields start empty and are filled in when a query reads them.
impl DefinitionRecord {
    const FIELDS: [&'static str; 6] = [
        NAME,
        EXPRESSION,
        LazyField::Value.as_str(),
        LazyField::Type.as_str(),
        LazyField::Display.as_str(),
        "computed",
    ];
}
impl RecordFields for DefinitionRecord {
    fn fields(&self) -> BTreeMap<String, Value> {
        let mut fields = self.base.fields();
        let values = [
            q::text(&self.name),
            q::text(&self.expression),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Bool(self.computed),
        ];
        fields.extend(Self::FIELDS.iter().map(|k| k.to_string()).zip(values));
        if self.solution {
            fields.insert(LazyField::Solution.as_str().into(), Value::Null);
        }
        fields
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct RowRecord {
        ..base: Base,
        table: String,
        cells: BTreeMap<String, Value>,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct DecisionRecord {
        ..base: Base,
        value: Value,
        plan: String,
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
            let symbol = Symbol::new(plan_path.clone(), SymbolKind::Definition(plan.definition));
            let Ok(Value::Plan(value)) = engine.symbol(&symbol) else {
                continue;
            };
            for (row, value) in &value.rows {
                if only.is_some_and(|path| path != row.table.path) {
                    continue;
                }
                let Some(cell) = lang::eval::tables::table(ws, &row.table)
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
                base.set_anchor(cell.span.range(&ws.documents[&row.table.path].text).end);
                records.push(Record::typed(
                    &row.table.path,
                    DecisionRecord {
                        base,
                        value: q::query_value(value.clone()),
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
        let plan = doc.plan_of(i).is_some();
        let table = doc.table_of(i);
        if (collection == Collection::Plans && !plan)
            || (matches!(collection, Collection::Tables | Collection::Rows) && table.is_none())
        {
            continue;
        }
        let symbol = Symbol::new(path, SymbolKind::Definition(i));
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
                                    .zip(values.iter().cloned().map(q::query_value))
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
        base.set_anchor(def.end.range(&doc.text).end);
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
