//! Built records: the collections feature modules declare under
//! `collections` and build in their `records` hook, from whatever they read
//! of the note (their recognizers' matches, most often). One call builds
//! every collection a module declares for one note; [`Records`] keeps the
//! answer for the note's revision and day, and each collection's records
//! from it, so queries, other modules' inputs and the host all read one
//! build.
//!
//! The host keeps what the hook returns as it is, filling in only the fields
//! every record carries, and answers the lookups a record asks for from the
//! workspace's cache, remembering them for a refresh to fetch. Nothing here
//! knows what any collection means.
use crate::record::{Record, SourceRef};
use crate::{Records, value as q};
use lang::eval::engine::{Engine, Value};
use lang::eval::lookups::{LookupKey, Store};
use lang::eval::modules::{Declared, Hook, Joins, Module, ModuleKind};
use lang::eval::{ToValue, Workspace};
use lang::stdlib;
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};
use std::{collections::BTreeMap, path::Path, sync::Arc};

/// The most records one collection of one note holds.
const MAX_RECORDS: usize = 4096;
/// How deep inside a record a lookup request is looked for.
const MAX_DEPTH: usize = 16;

/// One record a module built, and the lookups it asked for.
#[derive(Clone, Debug)]
pub(crate) struct Built {
    pub(crate) fields: BTreeMap<String, Value>,
    pub(crate) wants: Vec<LookupKey>,
}

/// What one `records` call built for one note: each declared collection's
/// records by name, or why the call failed.
pub(crate) type Build = Result<BTreeMap<Arc<str>, Vec<Built>>, String>;

/// Call `module`'s `records` hook for the note at `path` and check what it
/// returns. `engine` is marked with whatever clock reading the inputs took;
/// the hook itself runs without the clock.
pub(crate) fn build(
    records: &Records,
    engine: &mut Engine<'_>,
    module: &Module,
    path: &Path,
) -> Build {
    let context = crate::context::feature_context(records, engine, module, path, true)?;
    let answer = module
        .call(
            Hook::Records,
            vec![Value::record(context)],
            lang::eval::modules::no_clock(),
        )
        .map_err(|e| match e {
            lang::eval::EvalError::Module { .. } => e.to_string(),
            other => other.in_module(&module.id, "records").to_string(),
        })?;
    let ws = engine.workspace();
    let lines = ws.documents()[path].line_count().max(1);
    let Value::Record(answer) = answer else {
        return Err(format!(
            "{}.records must return a record of collections",
            module.id
        ));
    };
    // The answer is this build's alone: its records are taken, not copied.
    let mut answer = Arc::unwrap_or_clone(answer);
    module
        .collections
        .iter()
        .map(|declared| {
            let items = match answer.remove(&*declared.name) {
                Some(Value::List(items)) => Arc::unwrap_or_clone(items).into_inner(),
                _ => {
                    return Err(format!(
                        "{}.records must return a list under '{}'",
                        module.id, declared.name
                    ));
                }
            };
            if items.len() > MAX_RECORDS {
                return Err(format!(
                    "{}.records built more than {MAX_RECORDS} {}",
                    module.id, declared.name
                ));
            }
            let built = items
                .into_iter()
                .map(|item| record(engine, ws, path, lines, declared, item))
                .collect::<Result<_, String>>()
                .map_err(|e| format!("{}.records: {e}", module.id))?;
            Ok((declared.name.clone(), built))
        })
        .collect()
}

/// One returned record, checked against the note's `lines`, with the fields
/// every record carries filled in and its lookups answered.
fn record(
    engine: &mut Engine<'_>,
    ws: &Workspace,
    path: &Path,
    lines: usize,
    declared: &Declared,
    item: Value,
) -> Result<Built, String> {
    let Value::Record(fields) = item else {
        return Err(format!("each of {} must be a record", declared.name));
    };
    let doc = &ws.documents()[path];
    let line = match fields.get("line") {
        Some(Value::Count(line)) => Some(*line),
        Some(Value::Number(n)) if n.fract() == 0.0 && *n >= 0.0 => Some(*n as usize),
        _ => None,
    }
    .filter(|line| *line < lines)
    .ok_or_else(|| format!("each of {} needs a line within the note", declared.name))?;
    let mut fields = Arc::unwrap_or_clone(fields).into_inner();
    fields.insert("line".into(), Value::Count(line));
    fields
        .entry("kind".into())
        .or_insert_with(|| q::text(&*declared.name));
    fields
        .entry("title".into())
        .or_insert_with(|| q::text(doc.line(line)));
    fields
        .entry("source".into())
        .or_insert_with(|| SourceRef::new(ws, path, doc.line_span(line)).to_value());
    fields
        .entry("anchor".into())
        .or_insert_with(|| q::position(doc.line_end(line)));
    fields
        .entry("errors".into())
        .or_insert_with(|| Value::list(vec![]));
    let mut wants = Vec::new();
    for value in fields.values_mut() {
        answer(engine, ws.lookups(), value, &mut wants, 0)?;
    }
    Ok(Built { fields, wants })
}

/// Answer every lookup `value` asks for, in place: a record with a `lookup`
/// field becomes its other fields with the cached value's `display` (as the
/// prelude's `lookup_display` words it), `source` and `fetched_at`, or null
/// when nothing is cached.
fn answer(
    engine: &mut Engine<'_>,
    store: &Store,
    value: &mut Value,
    wants: &mut Vec<LookupKey>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    match value {
        Value::Record(fields) => {
            if let Some(request) = fields.get("lookup") {
                let key = LookupKey::requested(request)?;
                *value = match key.lookup(store) {
                    None => Value::Null,
                    Some(lookup) => {
                        let mut fields = Arc::unwrap_or_clone(fields.clone()).into_inner();
                        fields.remove("lookup");
                        let display = stdlib::prelude::lookup_display(engine, &key, lookup);
                        fields.insert("display".into(), q::text(stdlib::shown(display)));
                        fields.insert("source".into(), q::text(&lookup.source));
                        fields.insert(
                            "fetched_at".into(),
                            Value::DateTime(lookup.fetched_at.fixed_offset()),
                        );
                        Value::record(fields)
                    }
                };
                wants.push(key);
                return Ok(());
            }
            for field in Arc::make_mut(fields).values_mut() {
                answer(engine, store, field, wants, depth + 1)?;
            }
        }
        Value::List(items) => {
            for item in Arc::make_mut(items) {
                answer(engine, store, item, wants, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The records of the collection `name` in the note at `path`: none when no
/// module declares it or its build failed, which the note's diagnostics say.
pub(crate) fn collection(
    records: &Records,
    engine: &mut Engine<'_>,
    path: &Path,
    name: &str,
    out: &mut Vec<Record>,
) {
    let Some((module, _)) = engine.workspace().modules().declaring(name) else {
        return;
    };
    if let Ok(built) = &*records.built(engine, module, path)
        && let Some(items) = built.get(name)
    {
        out.extend(
            items
                .iter()
                .map(|item| Record::built(path, item.fields.clone())),
        );
    }
}

/// The records of every declared collection that join `entries`: all of
/// them, or those whose field the declaration names is true.
pub(crate) fn entries(
    records: &Records,
    engine: &mut Engine<'_>,
    path: &Path,
    out: &mut Vec<Record>,
) {
    let ws = engine.workspace();
    let joining: Vec<(Arc<str>, Joins)> = ws
        .modules()
        .declared()
        .filter(|(_, declared)| declared.entries != Joins::None)
        .map(|(_, declared)| (declared.name.clone(), declared.entries.clone()))
        .collect();
    for (name, joins) in joining {
        let mut built = Vec::new();
        collection(records, engine, path, &name, &mut built);
        out.extend(built.into_iter().filter(|record| match &joins {
            Joins::Where(field) => {
                matches!(record.fields.get(&**field), Some(Value::Bool(true)))
            }
            _ => true,
        }));
    }
}

/// Every module that builds records for a note.
fn builders(ws: &Workspace) -> impl Iterator<Item = &Module> {
    ws.modules()
        .of_kind(ModuleKind::Feature)
        .filter(|m| !m.collections.is_empty())
}

/// Each lookup the records built for the note at `path` ask for, with the
/// line of the record that asks: what a refresh fetches, and where it is
/// offered.
pub(crate) fn wanted(
    records: &Records,
    engine: &mut Engine<'_>,
    path: &Path,
) -> Vec<(usize, LookupKey)> {
    let ws = engine.workspace();
    let mut wanted = Vec::new();
    for module in builders(ws) {
        if let Ok(built) = &*records.built(engine, module, path) {
            for item in built.values().flatten() {
                let line = match item.fields.get("line") {
                    Some(Value::Count(line)) => *line,
                    _ => continue,
                };
                wanted.extend(item.wants.iter().map(|key| (line, key.clone())));
            }
        }
    }
    wanted
}

/// Each module whose `records` failed for the note at `path`, as an error on
/// the first line it recognized there, or the note's first line.
pub fn problems(
    request: &lang::eval::RequestContext<'_>,
    records: &Records,
    path: &Path,
) -> Vec<Diagnostic> {
    let ws = request.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    let mut engine = request.engine();
    builders(ws)
        .filter_map(|module| {
            let build = records.built(&mut engine, module, path);
            let error = (*build).as_ref().err()?.clone();
            let range = doc
                .recognized
                .iter()
                .find(|found| found.rule.module == module.id)
                .map_or(Range::default(), |found| found.span.range(doc));
            Some(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("xmd".into()),
                code: Some(NumberOrString::String("module".into())),
                message: error,
                ..Default::default()
            })
        })
        .collect()
}
