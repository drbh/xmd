//! A named checklist heading's items as data: `checklist.tasks` is one record
//! per leaf item beneath the heading, which is all the prelude's `total`,
//! `completed`, `remaining` and `effort` read. Whether an item is done is the
//! engine's (`Engine::task_done`), since an item's name evaluates to exactly
//! that.
use crate::engine::{Engine, Value, ValueType};
use values::{CHECKLIST_TASKS, EvalResult, HostObject, TaskKey, record};

/// A checklist that crossed into module code. Its tasks are references into
/// the notes, which a module's own workspace cannot resolve, so they travel
/// with the records `tasks` reads, read on the note's side of the call.
#[derive(Debug, PartialEq)]
pub(crate) struct ChecklistValue {
    keys: Vec<TaskKey>,
    tasks: Value,
}
impl HostObject for ChecklistValue {
    fn kind(&self) -> ValueType {
        ValueType::Checklist
    }
    fn display(&self) -> String {
        HostObject::display(&self.keys)
    }
    fn property(&self, key: &str) -> EvalResult<Value> {
        match key {
            CHECKLIST_TASKS => Ok(self.tasks.clone()),
            _ => self.keys.property(key),
        }
    }
    fn fields(&self) -> Vec<String> {
        self.keys.fields()
    }
    fn query(&self) -> Option<Value> {
        self.keys.query()
    }
}

impl Engine<'_> {
    /// `checklist.tasks`: `{title, line, done, attributes}` for each item,
    /// in order. `attributes` holds each attribute a module declares that
    /// the item writes, as `{text, value, error}`: `value` is what it reads
    /// as, evaluated as its kind reads it but unchecked, so a reader decides
    /// what it needs (the prelude's `effort` wants a nonnegative duration);
    /// when it fails to evaluate, `error` says why and `value` is null, so a
    /// reader that never needs it is not failed by it.
    pub(crate) fn checklist_tasks(&mut self, keys: &[TaskKey]) -> Value {
        let tasks = keys
            .iter()
            .map(|(path, index)| {
                let doc = &self.workspace().documents[path];
                let task = &doc.tasks[*index];
                let declared: Vec<_> = task
                    .attributes
                    .iter()
                    .filter_map(|(key, attribute)| {
                        Some((key.clone(), doc.attribute_value(key)?, attribute.clone()))
                    })
                    .collect();
                let (title, line) = (task.title.clone(), task.line);
                let attributes = declared
                    .into_iter()
                    .map(|(key, holds, attribute)| {
                        // Reported by whoever reads `error`, not here.
                        let failure = self.failure.take();
                        let value = self.evaluated(path, holds, &attribute);
                        self.failure = failure;
                        let (value, error) = match value {
                            Ok(value) => (value, Value::Null),
                            Err(error) => (Value::Null, Value::Text(error.to_string())),
                        };
                        let fields = record([
                            ("text", Value::Text(attribute.value.clone())),
                            ("value", value),
                            ("error", error),
                        ]);
                        (key, fields)
                    })
                    .collect();
                record([
                    ("title", Value::Text(title)),
                    ("line", Value::Number(line as f64)),
                    ("done", Value::Bool(self.task_done(path, *index))),
                    ("attributes", Value::record(attributes)),
                ])
            })
            .collect();
        Value::list(tasks)
    }
    /// A value about to be handed to module code, with any checklist in it
    /// carrying its tasks along.
    pub(crate) fn export(&mut self, value: Value) -> Value {
        match value {
            Value::Tasks(keys) => {
                let tasks = self.checklist_tasks(&keys);
                Value::Host(std::sync::Arc::new(ChecklistValue { keys, tasks }))
            }
            other => other,
        }
    }
}
