//! Definition and row records: values and tables.
use super::value as q;
use super::{
    Collection, RecordKind,
    record::{Base, LazyField, Record},
};
use lang::document::Document;
use lang::eval::engine::{Engine, Value};
use lang::eval::tables::TableValue;
use lang::eval::{RecordFields, record};
use lang::eval::{Symbol, SymbolKind, Workspace};
use std::{collections::BTreeMap, path::Path};

record! {
    /// Values and tables are one definition each. Their value, type and
    /// display (and what a form's module records of it) stay null until the
    /// record is evaluated. `form` names the form a definition calls, when
    /// it calls one a module declares.
    #[derive(Clone, Debug)]
    pub(super) struct DefinitionRecord {
        ..base: Base,
        name: String,
        expression: String,
        value: Value,
        type_name: Value => "type",
        display: Value,
        computed: bool,
        form: Value,
    }
}

record! {
    /// Where a table, or the table a form takes, lies, how its lines split
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

/// Values, the definitions forms lay out, tables and table rows all project
/// one definition.
pub(super) fn definitions(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    collection: &Collection,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) -> Result<(), String> {
    for (i, def) in doc.definitions().iter().enumerate() {
        let table = doc.table_of(i);
        let form = doc.form_of(i);
        if (matches!(collection, Collection::Tables | Collection::Rows) && table.is_none())
            || (*collection == Collection::Forms && form.is_none())
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
            table.map_or(RecordKind::Value, |_| RecordKind::Table),
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
                form: form.map_or(Value::Null, |f| q::text(&f.form.name)),
            },
        );
        // What the form's module records of it, read when it is evaluated.
        r.fields
            .insert(LazyField::Record.as_str().into(), Value::Null);
        let grid = match (form.filter(|f| f.has_table()), table) {
            (Some(f), _) => Some((f.header, f.end_line, &f.problems)),
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
        } else if form.is_some() {
            // A form's definition has a table's fields, null when its form
            // takes no table, so a module reads every form's alike.
            r.fields.extend(
                ["header", "end_line", "grid", "problems"].map(|key| (key.to_owned(), Value::Null)),
            );
        }
        r.deferred = Some(symbol);
        records.push(r);
    }
    Ok(())
}
