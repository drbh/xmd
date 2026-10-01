//! `cached(kind, key, label)`: the one way code reads the workspace's lookup
//! cache. Values come from the cache and are never fetched here; every key
//! read, hit or miss, is recorded as wanted, so `xmd refresh` and the ⟳
//! lookups lens know what to fetch, and hovers and inlays show each one's
//! age. What a kind of lookup means — a rate, a quote, a forecast — is the
//! prelude's.
use crate::engine::{Engine, Value};
use common::Span;
use std::{path::Path, sync::Arc};
use values::{EvalError, EvalResult, LookupKey, from_json, record};

/// A lookup evaluation read, and the note text it was read for: the
/// innermost note text being evaluated when `cached` ran, whichever module
/// function made the call. A definition another one reuses keeps its reads
/// as its own, so the row that offers a refresh is the row that reads.
#[derive(Clone, Debug)]
pub struct LookupRead {
    pub key: LookupKey,
    /// The note and the span of its text, or `None` when no note text was
    /// being evaluated, such as a module hook a host called.
    pub at: Option<(Arc<Path>, Span)>,
}

impl Engine<'_> {
    /// The cached answer for `kind` and `key` as `{value, fetched_at,
    /// source}`, or null when nothing has fetched it yet. `label` is how
    /// hovers and refresh errors name it.
    pub(crate) fn cached(
        &mut self,
        kind: &str,
        key: &Value,
        label: Option<&str>,
    ) -> EvalResult<Value> {
        let key = LookupKey::new(kind, key, label).map_err(EvalError::Message)?;
        let answer = match key.lookup(&self.lookups) {
            None => Value::Null,
            Some(lookup) => record([
                ("value", from_json(&lookup.value)),
                (
                    "fetched_at",
                    Value::DateTime(lookup.fetched_at.fixed_offset()),
                ),
                ("source", Value::Text(lookup.source.clone())),
            ]),
        };
        let at = self.reading_for();
        self.wanted.push(LookupRead { key, at });
        Ok(answer)
    }
}
