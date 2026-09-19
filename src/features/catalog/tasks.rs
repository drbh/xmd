//! Task, event and stop records: the entries a query can lay on a timeline.
use super::{
    QueryContext, QueryValue, RecordKind,
    record::{
        Base, ChildTask, Fields, Record, ScheduleEntry, Scheduling, When, date_field, entries,
        list, source,
    },
};
use crate::{
    document::Document,
    engine::{Engine, Value},
    workspace::Workspace,
};
use chrono::TimeZone;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
struct TaskRecord {
    base: Base,
    scheduling: Scheduling,
    checked: bool,
    name: QueryValue,
    attributes: BTreeMap<String, QueryValue>,
    blocked_error: QueryValue,
    schedule: Vec<ScheduleEntry>,
    children: Vec<ChildTask>,
    timer: QueryValue,
}
impl Fields for TaskRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.scheduling.fields());
        fields.extend(entries([
            ("checked", QueryValue::boolean(self.checked)),
            ("name", self.name),
            ("attributes", QueryValue::Object(self.attributes)),
            ("blocked_error", self.blocked_error),
            ("schedule", list(self.schedule)),
            ("children", list(self.children)),
            ("timer", self.timer),
        ]));
        fields
    }
}

#[derive(Clone, Debug)]
struct EventRecord {
    base: Base,
    scheduling: Scheduling,
}
impl Fields for EventRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.scheduling.fields());
        fields
    }
}

#[derive(Clone, Debug)]
struct StopRecord {
    base: Base,
    scheduling: Scheduling,
}
impl Fields for StopRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(self.scheduling.fields());
        fields
    }
}

pub(super) fn tasks(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    leaves_only: bool,
    records: &mut Vec<Record>,
) {
    let parents: std::collections::BTreeSet<_> =
        doc.tasks.iter().filter_map(|t| t.parent).collect();
    for (i, task) in doc.tasks.iter().enumerate() {
        let leaf = !parents.contains(&i);
        if leaves_only && !leaf {
            continue;
        }
        let mut record = TaskRecord {
            base: Base::new(ws, path, task.line, RecordKind::Task, &task.title),
            scheduling: Scheduling {
                leaf,
                done: engine.task_done(path, i),
                parent: task
                    .parent
                    .map(|i| source(ws, path, doc.tasks[i].checkbox))
                    .unwrap_or(QueryValue::Null),
                tags: task.tags.clone(),
                ..Scheduling::default()
            },
            checked: task.checked,
            name: task
                .named
                .as_ref()
                .map(|n| QueryValue::text(&n.name))
                .unwrap_or(QueryValue::Null),
            attributes: task
                .attributes
                .iter()
                .map(|(k, a)| (k.clone(), QueryValue::text(&a.value)))
                .collect(),
            blocked_error: QueryValue::Null,
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
            timer: QueryValue::Null,
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
                        .and_then(|v| ctx.date(v))
                        .map(|v| QueryValue::Scalar(Value::Date(v)))
                        .unwrap_or(QueryValue::Null),
                    error: value
                        .as_ref()
                        .err()
                        .map(QueryValue::text)
                        .unwrap_or(QueryValue::Null),
                });
            }
            match value {
                Ok(Value::Duration(s)) if when == When::Estimate && s >= 0 => {
                    record
                        .scheduling
                        .set(when, QueryValue::Scalar(Value::Duration(s)));
                }
                Ok(v) if when != When::Estimate && ctx.date(&v).is_some() => {
                    let date = ctx.date(&v);
                    let at = when == When::At;
                    record.scheduling.set(
                        when,
                        if at {
                            QueryValue::from_value(v)
                        } else {
                            date_field(date)
                        },
                    );
                    if at {
                        record.scheduling.at_date = date_field(date);
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
        if record.scheduling.due == QueryValue::Null && task.attributes.contains_key("every") {
            record.scheduling.due = date_field(Some(ctx.today()));
        }
        match engine.blocked(path, i) {
            Ok(v) => record.scheduling.blocked_by = v,
            Err(e) => {
                record.blocked_error = QueryValue::text(&e);
                errors.push(e);
            }
        }
        record.timer = match task
            .attributes
            .get("timer")
            .and_then(|a| engine.eval(path, &a.value).ok())
        {
            Some(Value::Timer(timer)) => QueryValue::from_value(timer.record()),
            _ => QueryValue::Null,
        };
        record.base.errors = errors;
        records.push(Record::typed(path, record));
    }
}

pub(super) fn events(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    for event in &doc.events {
        let mut base = Base::new(ws, path, event.line, RecordKind::Event, &event.title);
        let mut scheduling = Scheduling::default();
        match engine.when(path, &event.attributes[When::At.as_str()].value) {
            Ok(v) if ctx.date(&v).is_some() => {
                scheduling.at_date = date_field(ctx.date(&v));
                scheduling.at = QueryValue::from_value(v);
            }
            other => {
                base.errors = vec![
                    other
                        .err()
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
    ctx: QueryContext,
    records: &mut Vec<Record>,
) {
    let dates = crate::itinerary::dates(&ws.modules, &doc.days, ctx.today());
    for (day, date) in doc.days.iter().zip(dates) {
        for stop in &day.stops {
            let at = date.and_then(|d| {
                ctx.now
                    .offset()
                    .from_local_datetime(&d.and_time(stop.time))
                    .single()
            });
            records.push(Record::typed(
                path,
                StopRecord {
                    base: Base::new(
                        ws,
                        path,
                        stop.line,
                        RecordKind::Stop,
                        crate::itinerary::label(&ws.modules, stop),
                    ),
                    scheduling: Scheduling {
                        at_date: date_field(date),
                        at: at
                            .map(|d| QueryValue::Scalar(Value::DateTime(d)))
                            .unwrap_or_else(|| {
                                QueryValue::text(stop.time.format("%H:%M").to_string())
                            }),
                        ..Scheduling::default()
                    },
                },
            ));
        }
    }
}
