//! Task, event and stop records: the entries a query can lay on a timeline.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record, SourceRef},
};
use chrono::TimeZone;
use lang::eval::engine::{Engine, Value};
use lang::eval::timers::Timer;
use lang::eval::{Clock, Workspace};
use lang::eval::{ToValue, record};
use lang::model::{Document, TaskState};
use std::{collections::BTreeMap, path::Path};

/// The scheduling attributes a task reads, and the fields they land in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum When {
    Due,
    Scheduled,
    At,
    Estimate,
}
impl When {
    const ALL: [When; 4] = [Self::Due, Self::Scheduled, Self::At, Self::Estimate];
    const fn as_str(self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Scheduled => "scheduled",
            Self::At => "at",
            Self::Estimate => "estimate",
        }
    }
}
impl ToValue for When {
    fn to_value(&self) -> Value {
        q::text(self.as_str())
    }
}

record! {
    /// Tasks, events and stops share one shape, so a query can sort them
    /// together. The first four are named after [`When`].
    #[derive(Clone, Debug)]
    struct Scheduling {
        due: Value,
        scheduled: Value,
        at: Value,
        estimate: Value,
        at_date: Value,
        parent: Option<SourceRef>,
        tags: Vec<String>,
        blocked_by: Vec<String>,
        done: bool,
        leaf: bool,
    }
}
impl Default for Scheduling {
    fn default() -> Self {
        Self {
            due: Value::Null,
            scheduled: Value::Null,
            at: Value::Null,
            at_date: Value::Null,
            estimate: Value::Null,
            parent: None,
            tags: Vec::new(),
            blocked_by: Vec::new(),
            done: false,
            leaf: true,
        }
    }
}
impl Scheduling {
    fn set(&mut self, when: When, value: Value) {
        match when {
            When::Due => self.due = value,
            When::Scheduled => self.scheduled = value,
            When::At => self.at = value,
            When::Estimate => self.estimate = value,
        }
    }
}

record! {
    /// One `@due`/`@scheduled`/`@at` attribute as the task read it.
    #[derive(Clone, Debug)]
    struct ScheduleEntry {
        key: When,
        value: Value,
        error: Value,
    }
}
record! {
    #[derive(Clone, Debug)]
    struct ChildTask {
        line: usize,
        done: bool,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct TaskRecord {
        ..base: Base,
        ..scheduling: Scheduling,
        checked: bool,
        in_progress: bool,
        name: Value,
        attributes: BTreeMap<String, Value>,
        blocked_error: Value,
        schedule: Vec<ScheduleEntry>,
        children: Vec<ChildTask>,
        timer: Value,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct EventRecord {
        ..base: Base,
        ..scheduling: Scheduling,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct StopRecord {
        ..base: Base,
        ..scheduling: Scheduling,
    }
}

pub(super) fn tasks(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    leaves_only: bool,
    records: &mut Vec<Record>,
) {
    let clock = Clock::new(engine.now());
    let parents: std::collections::BTreeSet<_> =
        doc.tasks.iter().filter_map(|t| t.parent).collect();
    for (i, task) in doc.tasks.iter().enumerate() {
        let leaf = !parents.contains(&i);
        if leaves_only && !leaf {
            continue;
        }
        let mut record = TaskRecord {
            base: Base::line(ws, path, RecordKind::Task, &task.title, task.line),
            scheduling: Scheduling {
                leaf,
                done: engine.task_done(path, i),
                parent: task
                    .parent
                    .map(|i| SourceRef::new(ws, path, doc.tasks[i].checkbox)),
                tags: task.tags.clone(),
                ..Scheduling::default()
            },
            checked: task.state == TaskState::Done,
            in_progress: engine.task_in_progress(path, i),
            name: task
                .named
                .as_ref()
                .map(|n| q::text(&n.name))
                .unwrap_or(Value::Null),
            attributes: task
                .attributes
                .iter()
                .map(|(k, a)| (k.clone(), q::text(&a.value)))
                .collect(),
            blocked_error: Value::Null,
            schedule: Vec::new(),
            children: doc
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, child)| child.parent == Some(i))
                .map(|(index, child)| ChildTask {
                    line: child.line,
                    done: engine.task_done(path, index),
                })
                .collect(),
            timer: Value::Null,
        };
        let mut errors = Vec::new();
        for when in When::ALL {
            let Some(attribute) = task.attributes.get(when.as_str()) else {
                continue;
            };
            let value = if when == When::Estimate {
                engine.eval(path, &attribute.value)
            } else {
                engine.when(path, &attribute.value)
            };
            if when != When::Estimate {
                record.schedule.push(ScheduleEntry {
                    key: when,
                    value: value
                        .as_ref()
                        .ok()
                        .and_then(|v| clock.date(v))
                        .map(Value::Date)
                        .unwrap_or(Value::Null),
                    error: value
                        .as_ref()
                        .err()
                        .map(|e| q::text(e.to_string()))
                        .unwrap_or(Value::Null),
                });
            }
            match value {
                Ok(Value::Duration(s)) if when == When::Estimate && s >= 0 => {
                    record.scheduling.set(when, Value::Duration(s));
                }
                Ok(v) if when != When::Estimate && clock.date(&v).is_some() => {
                    let date = clock.date(&v);
                    let at = when == When::At;
                    record.scheduling.set(
                        when,
                        if at {
                            q::query_value(v)
                        } else {
                            date.to_value()
                        },
                    );
                    if at {
                        record.scheduling.at_date = date.to_value();
                    }
                }
                Ok(_) => errors.push(format!(
                    "@{}: expected {}",
                    when.as_str(),
                    if when == When::Estimate {
                        "a nonnegative duration"
                    } else {
                        "a date or timestamp"
                    }
                )),
                Err(e) => errors.push(format!("@{}: {e}", when.as_str())),
            }
        }
        // A repeating task with no explicit due date is due today.
        if record.scheduling.due == Value::Null && task.attributes.contains_key("every") {
            record.scheduling.due = clock.today().to_value();
        }
        match engine.blocked(path, i) {
            Ok(v) => record.scheduling.blocked_by = v,
            Err(e) => {
                record.blocked_error = q::text(e.to_string());
                errors.push(e.to_string());
            }
        }
        record.timer = match task
            .attributes
            .get("timer")
            .and_then(|a| engine.eval(path, &a.value).ok())
            .as_ref()
            .and_then(Value::downcast::<Timer>)
        {
            Some(timer) => q::query_value(timer.record()),
            None => Value::Null,
        };
        record.base.errors = errors;
        records.push(Record::typed(path, record));
    }
}

pub(super) fn events(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    let clock = Clock::new(engine.now());
    for event in &doc.events {
        let mut base = Base::line(ws, path, RecordKind::Event, &event.title, event.line);
        let mut scheduling = Scheduling::default();
        match engine.when(path, &event.attributes[When::At.as_str()].value) {
            Ok(v) if clock.date(&v).is_some() => {
                scheduling.at_date = clock.date(&v).to_value();
                scheduling.at = q::query_value(v);
            }
            other => {
                base.errors = vec![
                    other
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "@at requires a date or timestamp".into()),
                ];
            }
        }
        records.push(Record::typed(path, EventRecord { base, scheduling }));
    }
}

pub(super) fn stops(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &Engine<'_>,
    records: &mut Vec<Record>,
) {
    let clock = Clock::new(engine.now());
    let dates = lang::eval::itinerary::dates(ws.modules(), &doc.days, clock.today());
    for (day, date) in doc.days.iter().zip(dates) {
        for stop in &day.stops {
            let at = date.and_then(|d| {
                clock
                    .now
                    .offset()
                    .from_local_datetime(&d.and_time(stop.time))
                    .single()
            });
            records.push(Record::typed(
                path,
                StopRecord {
                    base: Base::line(
                        ws,
                        path,
                        RecordKind::Stop,
                        lang::eval::itinerary::label(ws.modules(), stop),
                        stop.line,
                    ),
                    scheduling: Scheduling {
                        at_date: date.to_value(),
                        at: at
                            .map(Value::DateTime)
                            .unwrap_or_else(|| q::text(stop.time.format("%H:%M").to_string())),
                        ..Scheduling::default()
                    },
                },
            ));
        }
    }
}
