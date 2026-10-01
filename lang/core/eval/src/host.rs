//! The host objects the evaluator builds — tables — and what every host
//! value shows once the engine and the link registry can answer too.
//!
//! [`HostObject`] is the value layer's contract for reading an object a note
//! cannot build. The objects that layer defines implement it there; the ones
//! only the evaluator can build implement it here, and travel in a value as
//! [`Value::Host`]. The impls sit together because the split is the point:
//! one file answers "what can a note see of an evaluator-built object?".
//! Each forwards to the object's own inherent methods.
use crate::engine::{Engine, Value, ValueType};
use crate::{resources::ResourcePresenting, stdlib, tables_impl::TableValue};
use modules::LinkFeatures;
use std::path::Path;
use values::HostObject;

/// What a host value shows that needs more than the object itself: a
/// resource's link fields and presentation, a checklist's progress. Every
/// other host value answers from its own [`HostObject`] methods.
pub trait HostPresenting {
    /// The names completion offers after `value.`; `links` is what a resource
    /// target's registered features add to its own.
    fn host_fields(&self, links: LinkFeatures<'_>) -> Vec<String>;
    /// Hover detail beyond the name, kind and display line every symbol gets.
    fn host_hover(&self, engine: &mut Engine<'_>, path: &Path) -> Option<String>;
}
impl HostPresenting for Value {
    fn host_fields(&self, links: LinkFeatures<'_>) -> Vec<String> {
        match self {
            Value::Resource(resource) => {
                let mut names = vec!["url".to_owned()];
                names.extend(links.property_names(&resource.target));
                if !resource.target.starts_with("http") && !resource.target.starts_with("geo:") {
                    names.push("exists".into());
                }
                names
            }
            other => other.host().map(HostObject::fields).unwrap_or_default(),
        }
    }
    fn host_hover(&self, engine: &mut Engine<'_>, path: &Path) -> Option<String> {
        match self {
            Value::Resource(resource) => {
                let presentation = resource.presentation(engine, path);
                // A link module's details can age with its cache.
                engine.mark_time_dependent(presentation.time_dependent);
                Some(format!("\n\n{}", presentation.hover))
            }
            Value::Tasks(tasks) => {
                let done = tasks
                    .iter()
                    .filter(|(path, index)| engine.task_done(path, *index))
                    .count();
                let summary = stdlib::shown(stdlib::task::checklist(engine, done, tasks.len()));
                Some(format!("\n\n{summary}"))
            }
            other => other.host()?.hover(),
        }
    }
}
impl HostObject for TableValue {
    fn kind(&self) -> ValueType {
        ValueType::Table
    }
    fn display(&self) -> String {
        format!("{} rows · {} columns", self.rows.len(), self.columns.len())
    }
    fn query(&self) -> Option<Value> {
        Some(Value::list(self.named_rows().map(Value::record).collect()))
    }
}
