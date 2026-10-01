//! What a feature module's hooks see of a note, the fields of the `feature
//! context` every hook shares: the request's day, the note with each
//! collection the module's `inputs` names, read from the shared [`Records`],
//! and which module is asking. The editor adds what one hook needs on top
//! (a range, capabilities, a position); the `records` hook takes it as it is.
use crate::{Records, View};
use chrono::{DateTime, FixedOffset, TimeZone};
use lang::eval::engine::{Engine, Value};
use lang::eval::modules::{Collection, Module};
use lang::eval::{ToValue, record};
use std::{collections::BTreeMap, path::Path, sync::Arc};

record! {
    /// The note a hook is looking at, with the record collections it declared
    /// under their own names.
    #[derive(Clone)]
    struct DocumentInput {
        ..collections: BTreeMap<String, Value>,
        path: String,
        uri: String,
        text: Value,
        lines: Value,
    }
}

record! {
    /// Which module is asking, and at what revision.
    #[derive(Clone)]
    struct ModuleInput {
        id: String,
        revision: String,
    }
}

/// The start of `now`'s day at its offset: a reference that places a day's
/// times without reading the clock.
fn midnight(now: DateTime<FixedOffset>) -> Value {
    let start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight exists");
    now.offset()
        .from_local_datetime(&start)
        .single()
        .map_or(Value::Null, Value::DateTime)
}

/// The fields every hook of `module` is handed for the note at `path`. Each
/// collection its `inputs` names comes from `records`, narrowed to the fields
/// the module asked for, and `engine` is marked with whatever clock reading
/// they took. `building` leaves out the collections modules build, `entries`
/// with them, and adds the ones the module's collections are built `from`,
/// as the `records` hook that builds them sees the note.
pub fn feature_context(
    records: &Records,
    engine: &mut Engine<'_>,
    module: &Module,
    path: &Path,
    building: bool,
) -> Result<BTreeMap<String, Value>, String> {
    let note = records.note(engine.workspace(), path)?;
    let mut document = DocumentInput {
        path: path.to_string_lossy().into(),
        uri: note.uri.clone(),
        text: note.text.clone(),
        lines: note.lines.clone(),
        collections: BTreeMap::new(),
    };
    // The `records` hook reads the collections its declared ones are built
    // from as well, as `from` names them; the other hooks only `inputs`.
    let inputs = module
        .inputs
        .iter()
        .filter(|collection| !(building && module.sources.contains_key(*collection)))
        .map(|collection| (collection, module.fields.get(collection)));
    let sources = module
        .sources
        .iter()
        .filter(|_| building)
        .map(|(collection, kept)| (collection, kept.as_ref()));
    for (collection, kept) in inputs.chain(sources) {
        if building && (collection.is_declared() || *collection == Collection::Entries) {
            continue;
        }
        let view = kept.map_or(View::Full, |fields| View::Fields(fields));
        let mut values = records.view(
            engine,
            Some(path),
            collection.clone(),
            view,
            |request, path| analysis::collect_native(request, path, false),
        )?;
        if *collection == Collection::Recognized {
            // A module sees only its own recognizers' matches: the records
            // are the note's matches that make one, in order.
            let doc = &engine.workspace().documents()[path];
            let mine = doc
                .recognized()
                .iter()
                .filter(|found| found.rule.record)
                .map(|found| found.rule.module == module.id);
            values = own(&values, mine);
        }
        if *collection == Collection::Values {
            // Preserve the original API's alias; all new fields come from the records.
            let mut definitions = values.clone();
            if let Value::List(items) = &mut definitions {
                for item in Arc::make_mut(items) {
                    if let Value::Record(fields) = item {
                        let error = match fields.get("errors") {
                            Some(Value::List(errors)) => {
                                errors.first().cloned().unwrap_or(Value::Null)
                            }
                            _ => Value::Null,
                        };
                        Arc::make_mut(fields).insert("error".into(), error);
                    }
                }
            }
            document
                .collections
                .insert("definitions".into(), definitions);
        }
        document
            .collections
            .insert(collection.as_str().into(), values);
    }
    Ok(BTreeMap::from([
        ("today".into(), engine.today().to_value()),
        ("midnight".into(), midnight(engine.now())),
        ("document".into(), document.to_value()),
        (
            "module".into(),
            ModuleInput {
                id: module.id.clone(),
                revision: module.revision(),
            }
            .to_value(),
        ),
    ]))
}

/// The items of `values` that `mine` says are the module's, in order.
fn own(values: &Value, mine: impl Iterator<Item = bool>) -> Value {
    let Value::List(values) = values else {
        return values.clone();
    };
    Value::list(
        values
            .iter()
            .zip(mine)
            .filter(|(_, mine)| *mine)
            .map(|(value, _)| value.clone())
            .collect(),
    )
}
