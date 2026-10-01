//! A checklist's items as the engine reads them. Whether an item is done
//! (`Engine::task_done`) is what its name evaluates to, and what it still
//! waits on (`Engine::blocked`) is what its dependencies attributes do not
//! meet yet, a cycle through other items being an error that names them.
//! A named checklist heading's items as data: `checklist.tasks` is one record
//! per leaf item beneath the heading, which is all the prelude's `total`,
//! `completed`, `remaining` and `effort` read.
use crate::engine::{Engine, Value, ValueType};
use crate::workspace::{Symbol, SymbolKind};
use document::TaskState;
use std::path::Path;
use values::{CHECKLIST_TASKS, Depth, EvalError, EvalResult, HostObject, TaskKey, record};

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

/// The conditions not met yet, and the items whose are being read.
type Unmet = EvalResult<Vec<String>>;
type Stack = Vec<TaskKey>;
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
                let task = &doc.tasks()[*index];
                let attributes = task
                    .attributes
                    .iter()
                    .filter_map(|(key, attribute)| {
                        let holds = doc.attribute_value(key)?;
                        // Reported by whoever reads `error`, not here.
                        let failure = self.failure.take();
                        let value = self.evaluated(path, holds, attribute);
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
                        Some((key.clone(), fields))
                    })
                    .collect();
                record([
                    ("title", Value::Text(task.title.clone())),
                    ("line", Value::Number(task.line as f64)),
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
    /// Whether checklist item `i` is done, which is what its name evaluates
    /// to: its checkbox is checked, or it has subitems and every one is done.
    pub fn task_done(&self, path: &Path, i: usize) -> bool {
        let doc = &self.request.workspace.documents[path];
        let mut children = (0..doc.tasks().len())
            .filter(|&j| doc.tasks()[j].parent == Some(i))
            .peekable();
        match children.peek() {
            None => doc.tasks()[i].state == TaskState::Done,
            Some(_) => children.all(|j| self.task_done(path, j)),
        }
    }
    /// The conditions checklist item `i` still waits on: those of its
    /// attributes that hold dependencies (a declared attribute of the
    /// `dependencies` kind) not met yet. A condition that names another item
    /// brings that item's dependencies in, so a cycle is an error naming it.
    pub fn blocked(&mut self, path: &Path, i: usize) -> Unmet {
        self.blocked_inner(path, i, &mut Vec::new())
    }
    /// The conditions one dependencies attribute does not meet yet: on
    /// checklist item `at`, with its cycles checked as [`Self::blocked`]
    /// checks them.
    pub(crate) fn unmet(&mut self, path: &Path, at: Option<usize>, key: &str, text: &str) -> Unmet {
        let mut stack: Vec<TaskKey> = at.map(|i| (path.to_path_buf(), i)).into_iter().collect();
        self.conditions(path, key, text, &mut stack)
    }
    /// The attributes of item `i` that hold dependencies, by key.
    fn dependencies(&self, path: &Path, i: usize) -> Vec<(String, document::Attribute)> {
        let doc = &self.request.workspace.documents[path];
        doc.tasks()[i]
            .attributes
            .iter()
            .filter(|(key, _)| {
                doc.attribute_value(key) == Some(syntax::AttributeValue::Dependencies)
            })
            .map(|(key, attribute)| (key.clone(), attribute.clone()))
            .collect()
    }
    fn blocked_inner(&mut self, path: &Path, i: usize, stack: &mut Vec<TaskKey>) -> Unmet {
        let key = (path.to_path_buf(), i);
        if let Some(start) = stack.iter().position(|k| k == &key) {
            let related: Vec<_> = stack[start..]
                .iter()
                .chain(std::iter::once(&key))
                .filter(|(p, index)| {
                    self.request.workspace.documents[p].tasks()[*index]
                        .named
                        .is_some()
                })
                .map(|(p, index)| Symbol::new(p.clone(), SymbolKind::Task(*index)))
                .collect();
            let span = self
                .dependencies(path, i)
                .first()
                .map(|(_, a)| a.value_span)
                .unwrap_or(self.request.workspace.documents[path].tasks()[i].checkbox);
            return self.cycle(path, span, related, |names| EvalError::TaskCycle { names });
        }
        if stack.len() > 64 {
            self.contextual();
            return Err(EvalError::DepthExceeded(Depth::Task));
        }
        stack.push(key);
        let mut blocked = Vec::new();
        for (name, attribute) in self.dependencies(path, i) {
            blocked.extend(self.conditions(path, &name, &attribute.value, stack)?);
        }
        stack.pop();
        Ok(blocked)
    }
    /// Each comma-separated condition of `text` that is not met: a Boolean
    /// that is false, or a checklist with an item not done. One that names a
    /// checklist item brings that item's own dependencies in first.
    fn conditions(&mut self, path: &Path, key: &str, text: &str, stack: &mut Stack) -> Unmet {
        let mut blocked = Vec::new();
        for name in text.split(',').map(str::trim) {
            if let Ok(s) = self.request.workspace.resolve(path, name)
                && let SymbolKind::Task(j) = s.kind
            {
                self.blocked_inner(&s.path, j, stack)?;
            }
            let ready = match self.eval(path, name)? {
                Value::Bool(b) => b,
                Value::Tasks(ts) => ts.iter().all(|(p, j)| self.task_done(p, *j)),
                _ => {
                    return Err(format!(
                        "@{key} requires task names, checklists, or boolean expressions"
                    )
                    .into());
                }
            };
            if !ready {
                blocked.push(name.to_string());
            }
        }
        Ok(blocked)
    }
}
