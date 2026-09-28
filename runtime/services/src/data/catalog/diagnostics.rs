//! Diagnostic records: the workspace's own diagnostics, as queryable rows.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use crate::providers::Provider;
use lang::eval::Workspace;
use lang::eval::engine::Engine;
use lang::eval::engine::Value;
use lang::eval::record;
use serde_json::json;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct DiagnosticRecord {
        ..base: Base,
        message: String,
        severity: String,
        code: Value,
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
        crate::providers::diagnostics(&request, path, false)
    } else {
        crate::providers::Builtin.diagnostics(&request, path, false)
    };
    for d in diagnostics {
        let mut base = Base::new(
            ws,
            path,
            d.range.start.line as usize,
            RecordKind::Diagnostic,
            &d.message,
        );
        base.source.set_range(d.range);
        records.push(Record::typed(
            path,
            DiagnosticRecord {
                base,
                message: d.message,
                severity: crate::language::diagnostics::severity_name(d.severity).into(),
                code: q::from_json(json!(d.code)),
            },
        ));
    }
}
