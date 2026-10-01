//! External data behind explicit refreshes. Notes read cached values with the
//! time they were fetched, so they keep working offline and every badge can
//! show its age. A host (`runtime/host`) stores them and fetches them on an
//! explicit refresh, through the provider for each lookup's kind.
//!
//! Nothing here knows what any kind of lookup means: a key is a kind and its
//! parts, in order. What a rate, a quote or a forecast is, and how one reads,
//! is the prelude's (`lang/stdlib/prelude.xmd`), which asks for keys with
//! `cached(kind, key)`.
use crate::value::Value;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Lookup {
    pub value: serde_json::Value,
    pub fetched_at: DateTime<Utc>,
    pub source: String,
}
impl Lookup {
    pub fn new(value: serde_json::Value, fetched_at: DateTime<Utc>, source: String) -> Self {
        Self {
            value,
            fetched_at,
            source,
        }
    }
}
/// Keyed by the spelling `LookupKey` displays, because that is the on-disk
/// format of `.xmd/lookups.json`.
pub type Store = BTreeMap<String, Lookup>;

/// What a note asked the world for: a kind (`rate`) and its named parts in
/// order (`from: EUR`, `to: USD`). `Display` writes the key
/// `.xmd/lookups.json` stores it under, `kind:part:part` (`rate:EUR:USD`),
/// and a key is equal to, and ordered like, its spelling. Its label, the
/// words hovers and refresh errors name it by (`rate EUR→USD`), is the
/// asker's and no part of what it is.
#[derive(Clone, Debug)]
pub struct LookupKey {
    kind: String,
    parts: Vec<(String, Value)>,
    spelled: String,
    label: Option<String>,
}
impl LookupKey {
    /// The key `cached(kind, key, label)` names: `key` lists the parts in the order
    /// the cache spells them, each a one-field record, so
    /// `[{place: "oaxaca"}, {date: 2026-11-20}]` is `forecast:oaxaca:2026-11-20`.
    /// A part is text, a code, a date or a number.
    pub fn new(kind: &str, key: &Value, label: Option<&str>) -> Result<Self, String> {
        if kind.is_empty() || kind.contains(':') {
            return Err(format!(
                "A lookup's kind is a name without colons, not '{kind}'"
            ));
        }
        let Value::List(items) = key else {
            return Err(
                "A lookup's key is a list of one-field records, such as [{from: EUR}, {to: USD}]"
                    .into(),
            );
        };
        let mut parts = Vec::with_capacity(items.len());
        let mut spelled = kind.to_owned();
        for item in items.iter() {
            let (name, value) = match item {
                Value::Record(fields) if fields.len() == 1 => {
                    fields.iter().next().expect("one field")
                }
                _ => return Err("Each part of a lookup's key is a one-field record".into()),
            };
            // A provider reads the parts beside the kind, in one record.
            if name == "kind" || parts.iter().any(|(seen, _)| seen == name) {
                return Err(format!(
                    "A lookup's key cannot name '{name}' twice or as its kind"
                ));
            }
            let text = match value {
                Value::Text(text) => text.clone(),
                Value::Code(code) => code.as_str().to_owned(),
                Value::Date(date) => date.to_string(),
                Value::Number(_) | Value::Count(_) => value.display(),
                other => {
                    return Err(format!(
                        "A lookup's {name} must be text, a date or a number, found {}",
                        other.type_name()
                    ));
                }
            };
            spelled.push(':');
            spelled.push_str(&text);
            parts.push((name.clone(), value.clone().plain()));
        }
        Ok(Self {
            kind: kind.to_owned(),
            parts,
            spelled,
            label: label.map(str::to_owned),
        })
    }
    /// The lookup a module's record asks for: `{kind: "forecast", key:
    /// [{place: …}, {date: …}], label: "forecast …"}`, the arguments `cached`
    /// takes; the label may be left out.
    pub fn requested(value: &Value) -> Result<Self, String> {
        let Value::Record(fields) = value else {
            return Err("A lookup is a record with a kind and a key".into());
        };
        let kind = match fields.get("kind") {
            Some(Value::Text(kind)) => kind.clone(),
            Some(Value::Code(code)) => code.as_str().to_owned(),
            _ => return Err("A lookup's kind must be text".into()),
        };
        let label = match fields.get("label") {
            None | Some(Value::Null) => None,
            Some(Value::Text(label)) => Some(label.as_str()),
            Some(_) => return Err("A lookup's label must be text".into()),
        };
        Self::new(&kind, fields.get("key").unwrap_or(&Value::Null), label)
    }
    /// The kind a provider answers: `rate`, `quote`, `forecast`, …
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// The key's parts by name, in order, as a provider command's
    /// placeholders and a provider module's key record name them.
    pub fn parts(&self) -> &[(String, Value)] {
        &self.parts
    }
    /// The parts as `cached` took them, a list of one-field records.
    pub fn key(&self) -> Value {
        Value::list(
            self.parts
                .iter()
                .map(|(name, value)| Value::record([(name.clone(), value.clone())].into()))
                .collect(),
        )
    }
    /// How hovers and refresh errors name the lookup: the label it was asked
    /// for with, or its kind and parts (`forecast oaxaca 2026-11-20`).
    pub fn label(&self) -> String {
        self.label.clone().unwrap_or_else(|| {
            std::iter::once(self.kind.clone())
                .chain(self.parts.iter().map(|(_, value)| value.display()))
                .collect::<Vec<_>>()
                .join(" ")
        })
    }
    pub fn lookup<'s>(&self, store: &'s Store) -> Option<&'s Lookup> {
        store.get(&self.spelled)
    }
}
impl std::fmt::Display for LookupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.spelled)
    }
}
impl PartialEq for LookupKey {
    fn eq(&self, other: &Self) -> bool {
        self.spelled == other.spelled
    }
}
impl Eq for LookupKey {}
/// Ordered by the spelling, so a sorted list of keys reads the same way the
/// store and the file do.
impl Ord for LookupKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.spelled.cmp(&other.spelled)
    }
}
impl PartialOrd for LookupKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
