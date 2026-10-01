//! Definition, row and decision records: values, plans and tables.
use super::value as q;
use super::{
    Collection, RecordKind,
    record::{Base, LazyField, Record},
};
use lang::eval::engine::{Engine, Value};
use lang::eval::plans::PlanValue;
use lang::eval::tables::TableValue;
use lang::eval::{RecordFields, record};
use lang::eval::{Symbol, SymbolKind, Workspace};
use lang::model::Document;
use std::{collections::BTreeMap, path::Path};

record! {
    /// Values, plans and tables are one definition each. Their value, type and
    /// display (and a plan's solution) stay null until the record is evaluated.
    #[derive(Clone, Debug)]
    pub(super) struct DefinitionRecord {
        ..base: Base,
        name: String,
        expression: String,
        value: Value,
        type_name: Value => "type",
        display: Value,
        computed: bool,
    }
}

record! {
    /// Where a table or a plan's constraint table lies, how its lines split
    /// into cells and what parsing found wrong with how it is written: what
    /// a module that lays one out reads.
    #[derive(Clone, Debug)]
    struct Grid {
        header: usize,
        end_line: usize,
        grid: Vec<GridLine>,
        problems: Vec<String>,
    }
}

record! {
    /// One line of a table from its header on: its text, and the cells it
    /// splits into, trimmed, or none when it is no closed row.
    #[derive(Clone, Debug)]
    struct GridLine {
        line: usize,
        text: String,
        cells: Option<Vec<String>>,
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
    for (plan_path, doc) in ws.documents() {
        for plan in &doc.plans {
            let symbol = Symbol::new(plan_path.clone(), SymbolKind::Definition(plan.definition));
            let Ok(value) = engine.symbol(&symbol) else {
                continue;
            };
            let Some(value) = value.downcast::<PlanValue>() else {
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
                let table_doc = &ws.documents()[&row.table.path];
                let base = Base::at(
                    ws,
                    &row.table.path,
                    RecordKind::Decision,
                    name,
                    table_doc.line_span(cell.span.line),
                    Some(cell.span.range(table_doc).end),
                );
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
    collection: &Collection,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) -> Result<(), String> {
    for (i, def) in doc.definitions.iter().enumerate() {
        let plan = doc.plan_of(i).is_some();
        let table = doc.table_of(i);
        if (*collection == Collection::Plans && !plan)
            || (matches!(collection, Collection::Tables | Collection::Rows) && table.is_none())
        {
            continue;
        }
        let symbol = Symbol::new(path, SymbolKind::Definition(i));
        if *collection == Collection::Rows {
            match engine.symbol(&symbol) {
                Ok(ref value) if let Some(t) = value.downcast::<TableValue>() => {
                    for (row, values) in t.rows.iter().enumerate() {
                        records.push(Record::typed(
                            path,
                            RowRecord {
                                base: Base::line(
                                    ws,
                                    path,
                                    RecordKind::Row,
                                    &def.named.name,
                                    table.unwrap().rows[row][0].span.line,
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
        let base = Base::at(
            ws,
            path,
            if plan {
                RecordKind::Plan
            } else if table.is_some() {
                RecordKind::Table
            } else {
                RecordKind::Value
            },
            &def.named.name,
            doc.line_span(def.named.span.line),
            Some(def.end.range(doc).end),
        );
        let mut r = Record::typed(
            path,
            DefinitionRecord {
                base,
                name: def.named.name.clone(),
                expression: def.source.clone(),
                value: Value::Null,
                type_name: Value::Null,
                display: Value::Null,
                computed: def.expression,
            },
        );
        if plan {
            r.fields
                .insert(LazyField::Solution.as_str().into(), Value::Null);
        }
        let grid = match (doc.plan_of(i), table) {
            (Some(p), _) => Some((p.header, p.end_line, &p.problems)),
            (None, Some(t)) => Some((t.header, t.end_line, &t.problems)),
            (None, None) => None,
        };
        if let Some((header, end_line, problems)) = grid {
            r.fields.extend(
                Grid {
                    header,
                    end_line,
                    grid: (header..end_line)
                        .map(|line| GridLine {
                            line,
                            text: doc.line(line).into(),
                            cells: lang::eval::tables::cells(doc.line(line), line)
                                .map(|cells| cells.into_iter().map(|(cell, _)| cell).collect()),
                        })
                        .collect(),
                    problems: problems.iter().map(|p| p.message.clone()).collect(),
                }
                .fields(),
            );
        }
        r.deferred = Some(symbol);
        records.push(r);
    }
    Ok(())
}
