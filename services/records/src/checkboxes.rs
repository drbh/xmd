//! Checkbox records: one per checklist item, a list item with a checkbox,
//! as the language reads it. An item's structure only: its title and name,
//! its checkbox as written, whether it is done (what its name evaluates to),
//! where it nests and its attributes as written. What its attributes say is in `attributed`, and what
//! an item means beyond that, a task, is the `tasks` module's to build from
//! the two.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::document::{Document, TaskState};
use lang::eval::engine::Value;
use lang::eval::{Workspace, record};
use std::{collections::BTreeMap, path::Path};

record! {
    #[derive(Clone, Debug)]
    pub(super) struct CheckboxRecord {
        ..base: Base,
        name: Value,
        name_range: Value,
        mark: String,
        done: bool,
        parent: Value,
        children: Vec<usize>,
        indent: usize,
        checkbox: Value,
        range: Value,
        attributes: BTreeMap<String, String>,
    }
}

pub(super) fn checkboxes(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    let mut children = vec![Vec::new(); doc.tasks().len()];
    for task in doc.tasks() {
        if let Some(parent) = task.parent {
            children[parent].push(task.line);
        }
    }
    // Whether each is done, as `Engine::task_done` says, from the last up:
    // an item's subitems come after it.
    let mut done = vec![false; doc.tasks().len()];
    let mut open = vec![false; doc.tasks().len()];
    let mut parents = vec![false; doc.tasks().len()];
    for (i, task) in doc.tasks().iter().enumerate().rev() {
        done[i] = if parents[i] {
            !open[i]
        } else {
            task.state == TaskState::Done
        };
        if let Some(parent) = task.parent {
            parents[parent] = true;
            open[parent] |= !done[i];
        }
    }
    for (i, (task, children)) in doc.tasks().iter().zip(children).enumerate() {
        records.push(Record::typed(
            path,
            CheckboxRecord {
                base: Base::line(ws, path, RecordKind::Checkbox, &task.title, task.line),
                name: task
                    .named
                    .as_ref()
                    .map_or(Value::Null, |n| q::text(&n.name)),
                name_range: task
                    .named
                    .as_ref()
                    .map_or(Value::Null, |n| q::range(n.span.range(doc))),
                mark: task.state.as_str().into(),
                done: done[i],
                parent: task
                    .parent
                    .map_or(Value::Null, |p| Value::Count(doc.tasks()[p].line)),
                children,
                indent: task.indent,
                checkbox: q::range(task.checkbox.range(doc)),
                range: q::range(analysis::line_range(doc, task.line)),
                attributes: task
                    .attributes
                    .iter()
                    .map(|(key, attribute)| (key.clone(), attribute.value.clone()))
                    .collect(),
            },
        ));
    }
}
