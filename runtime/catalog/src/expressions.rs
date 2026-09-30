//! Calculation, reference and cell records: one evaluated expression each.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::common::Span;
use lang::eval::Workspace;
use lang::eval::engine::{Engine, Value};
use lang::eval::resources::Resource;
use lang::eval::{RecordFields, record};
use lang::model::Document;
use lsp_types::Position;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct CalculationRecord {
        ..base: Base,
        ..expression: Expression,
        bracketed: bool,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct ReferenceRecord {
        ..base: Base,
        ..expression: Expression,
        name: String,
        property: Option<String>,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct CellRecord {
        ..base: Base,
        ..expression: Expression,
        computed: bool,
        table: String,
        row: usize,
        column: Option<String>,
    }
}

pub(super) fn calculations(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for calculation in &doc.calculations {
        let end = calculation.span.end + usize::from(calculation.bracketed);
        let evaluated = Evaluated::new(
            ws,
            path,
            RecordKind::Calculation,
            &calculation.source,
            calculation.span,
            Span::new(calculation.span.line, end, end).range(doc).end,
            engine.eval_at(path, &calculation.source, calculation.span),
        );
        records.push(
            evaluated.record(path, |base, expression| CalculationRecord {
                base,
                expression,
                bracketed: calculation.bracketed,
            }),
        );
    }
}

pub(super) fn references(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for reference in doc.references.iter().filter(|r| r.bracket) {
        let source = reference.expression();
        let end = doc.reference_close(reference);
        let evaluated = Evaluated::new(
            ws,
            path,
            RecordKind::Reference,
            &source,
            reference.span,
            Span::new(reference.span.line, end, end).range(doc).end,
            engine.eval(path, &source),
        );
        records.push(evaluated.record(path, |base, expression| ReferenceRecord {
            base,
            expression,
            name: reference.name.clone(),
            property: reference.property.clone(),
        }));
    }
}

pub(super) fn cells(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for table in &doc.tables {
        for (row, cells) in table.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate() {
                let value = match &cell.expression {
                    Some((source, span)) => engine.eval_at(path, source, *span),
                    None => lang::eval::tables::literal_value(cell),
                };
                let source = cell
                    .expression
                    .as_ref()
                    .map(|(s, _)| s.as_str())
                    .unwrap_or(&cell.source);
                let evaluated = Evaluated::new(
                    ws,
                    path,
                    RecordKind::Cell,
                    source,
                    cell.span,
                    cell.span.range(doc).end,
                    value,
                );
                records.push(evaluated.record(path, |base, expression| CellRecord {
                    base,
                    expression,
                    computed: cell.calculated(),
                    table: doc.definitions[table.definition].named.name.clone(),
                    row,
                    column: table.columns.get(column).map(|c| c.name.clone()),
                }));
            }
        }
    }
}

record! {
    /// Calculations, bracketed references and table cells all project one
    /// evaluated expression; only the surrounding fields differ.
    #[derive(Clone, Debug)]
    struct Expression {
        expression: String,
        value: Value,
        type_name: Value => "type",
        display: Value,
    }
}
impl Expression {
    /// Returns the projection and the errors that belong on the record's base.
    fn new(expression: &str, value: Result<Value, String>) -> (Self, Vec<String>) {
        match value {
            Ok(v) => (
                Self {
                    expression: expression.into(),
                    type_name: q::text(v.type_name()),
                    display: q::text(v.display()),
                    value: q::query_value(v),
                },
                Vec::new(),
            ),
            Err(e) => (
                Self {
                    expression: expression.into(),
                    value: Value::Null,
                    type_name: Value::Null,
                    display: Value::Null,
                },
                vec![e],
            ),
        }
    }
}

/// A calculation, bracketed reference or table cell: one expression, evaluated.
struct Evaluated {
    base: Base,
    expression: Expression,
    /// A resource value keeps its presentation available to the record.
    resource: Option<Resource>,
}
impl Evaluated {
    fn new(
        ws: &Workspace,
        path: &Path,
        kind: RecordKind,
        source: &str,
        span: Span,
        anchor: Position,
        value: lang::eval::EvalResult<Value>,
    ) -> Self {
        let mut base = Base::at(ws, path, kind, source, span, Some(anchor));
        let resource = match &value {
            Ok(Value::Resource(resource)) => Some(resource.clone()),
            _ => None,
        };
        let (expression, errors) = Expression::new(source, value.map_err(|e| e.to_string()));
        base.errors = errors;
        Self {
            base,
            expression,
            resource,
        }
    }
    /// The record for this expression, with the fields its kind adds.
    fn record<R: RecordFields>(
        self,
        path: &Path,
        build: impl FnOnce(Base, Expression) -> R,
    ) -> Record {
        let mut record = Record::typed(path, build(self.base, self.expression));
        record.resource = self.resource;
        record
    }
}
