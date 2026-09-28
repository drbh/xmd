//! Diagnostic records: the workspace's own diagnostics, as queryable rows.
use super::{
    QueryValue, RecordKind,
    record::{Base, Fields, Record, projected},
};
use eval::Workspace;
use eval::engine::Engine;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub(super) struct DiagnosticRecord {
    base: Base,
    message: String,
    severity: String,
    code: QueryValue,
}
impl DiagnosticRecord {
    pub(super) const FIELDS: [&'static str; 3] = ["message", "severity", "code"];
}
impl Fields for DiagnosticRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(
            DiagnosticRecord::FIELDS,
            [
                QueryValue::text(self.message),
                QueryValue::text(self.severity),
                self.code,
            ],
        ));
        fields
    }
}

pub(super) fn diagnostics(
    ws: &Workspace,
    path: &Path,
    engine: &mut Engine<'_>,
    module_diagnostics: bool,
    records: &mut Vec<Record>,
) {
    let request = engine.request();
    let diagnostics = if module_diagnostics {
        crate::diagnostics_impl::collect(&request, path, false)
    } else {
        crate::diagnostics_impl::collect_native(&request, path, false)
    };
    for d in diagnostics {
        let mut base = Base::new(
            ws,
            path,
            d.range.start.line as usize,
            RecordKind::Diagnostic,
            &d.message,
        );
        base.source.range = d.range;
        records.push(Record::typed(
            path,
            DiagnosticRecord {
                base,
                message: d.message,
                severity: crate::diagnostics_impl::severity_name(d.severity).into(),
                code: QueryValue::from_json(json!(d.code)),
            },
        ));
    }
}
