//! Calculation, reference and cell records: one evaluated expression each.
use super::{
    QueryValue, RecordKind,
    record::{Base, Expression, Fields, NAME, PROPERTY, Record, SourceRef, projected},
};
use common::Span;
use eval::Workspace;
use eval::engine::{Engine, Value};
use model::Document;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub(super) struct CalculationRecord {
    base: Base,
    expression: Expression,
    bracketed: bool,
}
impl CalculationRecord {
    pub(super) const FIELDS: [&'static str; 1] = ["bracketed"];
}
impl Fields for CalculationRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.expression.fields());
        fields.extend(projected(
            CalculationRecord::FIELDS,
            [QueryValue::boolean(self.bracketed)],
        ));
        fields
    }
}

#[derive(Clone, Debug)]
pub(super) struct ReferenceRecord {
    base: Base,
    expression: Expression,
    name: String,
    property: QueryValue,
}
impl ReferenceRecord {
    pub(super) const FIELDS: [&'static str; 2] = [NAME, PROPERTY];
}
impl Fields for ReferenceRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.expression.fields());
        fields.extend(projected(
            ReferenceRecord::FIELDS,
            [QueryValue::text(self.name), self.property],
        ));
        fields
    }
}

#[derive(Clone, Debug)]
pub(super) struct CellRecord {
    base: Base,
    expression: Expression,
    computed: bool,
    table: String,
    row: usize,
    column: QueryValue,
}
impl CellRecord {
    pub(super) const FIELDS: [&'static str; 4] = ["computed", "table", "row", "column"];
}
impl Fields for CellRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.expression.fields());
        fields.extend(projected(
            CellRecord::FIELDS,
            [
                QueryValue::boolean(self.computed),
                QueryValue::text(self.table),
                QueryValue::count(self.row),
                self.column,
            ],
        ));
        fields
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
        let (mut base, expression, resource) = expression_base(
            ws,
            path,
            RecordKind::Calculation,
            &calculation.source,
            calculation.span,
            engine.eval_at(path, &calculation.source, calculation.span),
        );
        let end = calculation.span.end + usize::from(calculation.bracketed);
        base.anchor = Span::new(calculation.span.line, end, end)
            .range(&doc.text)
            .end;
        let mut r = Record::typed(
            path,
            CalculationRecord {
                base,
                expression,
                bracketed: calculation.bracketed,
            },
        );
        r.resource = resource;
        records.push(r);
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
        let end = reference.end()
            + doc.line(reference.span.line)[reference.end()..]
                .find(']')
                .unwrap_or(0)
            + 1;
        let (mut base, expression, resource) = expression_base(
            ws,
            path,
            RecordKind::Reference,
            &source,
            reference.span,
            engine.eval(path, &source),
        );
        base.anchor = Span::new(reference.span.line, end, end)
            .range(&doc.text)
            .end;
        let mut r = Record::typed(
            path,
            ReferenceRecord {
                base,
                expression,
                name: reference.name.clone(),
                property: reference
                    .property
                    .as_ref()
                    .map(QueryValue::text)
                    .unwrap_or(QueryValue::Null),
            },
        );
        r.resource = resource;
        records.push(r);
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
                    None => cell
                        .value
                        .clone()
                        .map(Value::from)
                        .map_err(eval::EvalError::Message),
                };
                let source = cell
                    .expression
                    .as_ref()
                    .map(|(s, _)| s.as_str())
                    .unwrap_or(&cell.source);
                let (mut base, expression, resource) =
                    expression_base(ws, path, RecordKind::Cell, source, cell.span, value);
                base.anchor = cell.span.range(&doc.text).end;
                let mut r = Record::typed(
                    path,
                    CellRecord {
                        base,
                        expression,
                        computed: cell.calculated(),
                        table: doc.definitions[table.definition].named.name.clone(),
                        row,
                        column: table
                            .columns
                            .get(column)
                            .map(|c| QueryValue::text(&c.name))
                            .unwrap_or(QueryValue::Null),
                    },
                );
                r.resource = resource;
                records.push(r);
            }
        }
    }
}

/// A calculation, bracketed reference or table cell: one expression, evaluated.
fn expression_base(
    ws: &Workspace,
    path: &Path,
    kind: RecordKind,
    source: &str,
    span: Span,
    value: eval::EvalResult<Value>,
) -> (Base, Expression, Option<eval::resources::Resource>) {
    let mut base = Base::new(ws, path, span.line, kind, source);
    base.source = SourceRef::new(ws, path, span);
    let resource = match &value {
        Ok(Value::Resource(resource)) => Some(resource.clone()),
        _ => None,
    };
    let (expression, errors) = Expression::new(source, value.map_err(|e| e.to_string()));
    base.errors = errors;
    (base, expression, resource)
}
