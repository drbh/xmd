//! Diagnostic records: the workspace's own diagnostics, as queryable rows.
use super::value as q;
use super::{
    DiagnosticSource, RecordKind,
    record::{Base, Record},
};
use lang::eval::Workspace;
use lang::eval::engine::Engine;
use lang::eval::engine::Value;
use lang::eval::record;
use lsp_types::NumberOrString;
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
    source: DiagnosticSource,
    records: &mut Vec<Record>,
) {
    let request = engine.request();
    let diagnostics = source(&request, path);
    for d in diagnostics {
        records.push(Record::typed(
            path,
            DiagnosticRecord {
                base: Base::spanning(
                    ws,
                    path,
                    RecordKind::Diagnostic,
                    &d.message,
                    d.range.start.line as usize,
                    d.range,
                    None,
                ),
                message: d.message,
                severity: analysis::severity_name(d.severity).into(),
                code: match d.code {
                    Some(NumberOrString::String(code)) => q::text(code),
                    Some(NumberOrString::Number(code)) => {
                        usize::try_from(code).map_or(Value::Number(code.into()), Value::Count)
                    }
                    None => Value::Null,
                },
            },
        ));
    }
}
