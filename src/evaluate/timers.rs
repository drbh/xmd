//! Timers are timestamp-based values. Reading/evaluating them never mutates state.
use crate::{
    engine::{Value, timer_arguments},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset};
use lsp_types::TextEdit;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimerAction {
    Start,
    Pause,
    Resume,
    Reset,
}
impl TimerAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Reset => "reset",
        }
    }
}
impl std::str::FromStr for TimerAction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "start" => Ok(Self::Start),
            "pause" => Ok(Self::Pause),
            "resume" => Ok(Self::Resume),
            "reset" => Ok(Self::Reset),
            _ => Err(format!("Unknown timer action '{s}'")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    pub limit: Option<i64>,
    pub elapsed: i64,
    pub started: Option<DateTime<FixedOffset>>,
    pub idle: bool,
    pub origin: Option<Symbol>,
}

impl Timer {
    pub fn record(&self) -> Value {
        crate::plugins::record([
            (
                "limit".into(),
                self.limit.map(Value::Duration).unwrap_or(Value::Null),
            ),
            ("elapsed".into(), Value::Duration(self.elapsed)),
            (
                "started".into(),
                self.started.map(Value::DateTime).unwrap_or(Value::Null),
            ),
            ("idle".into(), Value::Bool(self.idle)),
        ])
    }
    pub fn new(name: &str, args: &[Value], now: DateTime<FixedOffset>) -> Result<Self, String> {
        let Value::Record(fields) = crate::plugins::standard(
            "timer",
            "create",
            vec![Value::Text(name.into()), Value::List(args.to_vec())],
            now,
        )?
        else {
            return Err("Timer constructor must return a record".into());
        };
        let limit = match fields.get("limit") {
            Some(Value::Null) => None,
            Some(Value::Duration(n)) => Some(*n),
            _ => return Err("Invalid timer limit".into()),
        };
        let Some(Value::Duration(elapsed)) = fields.get("elapsed") else {
            return Err("Invalid timer elapsed time".into());
        };
        let started = match fields.get("started") {
            Some(Value::Null) => None,
            Some(Value::DateTime(t)) => Some(*t),
            _ => return Err("Invalid timer timestamp".into()),
        };
        let Some(Value::Bool(idle)) = fields.get("idle") else {
            return Err("Invalid timer state".into());
        };
        Ok(Self {
            limit,
            elapsed: *elapsed,
            started,
            idle: *idle,
            origin: None,
        })
    }
    fn call(&self, name: &str) -> Value {
        crate::plugins::standard(
            "timer",
            name,
            vec![self.record()],
            chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.fixed_offset(),
        )
        .expect("valid typed timer")
    }
    pub fn done(&self) -> bool {
        matches!(self.call("done"), Value::Bool(true))
    }
    pub fn running(&self) -> bool {
        matches!(self.call("running"), Value::Bool(true))
    }
    pub fn state(&self) -> String {
        self.call("state").display()
    }
    pub fn display(&self) -> String {
        self.call("display").display()
    }
    pub fn inlay(&self) -> String {
        self.call("inlay").display()
    }
    pub fn hover(&self) -> String {
        self.call("hover").display()
    }
    pub fn property(&self, name: &str) -> Result<Value, String> {
        crate::plugins::standard(
            "timer",
            "property",
            vec![self.record(), Value::Text(name.into())],
            chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.fixed_offset(),
        )
    }
    pub fn available_actions(&self) -> Vec<TimerAction> {
        let Value::List(actions) = self.call("actions") else {
            return vec![];
        };
        actions
            .into_iter()
            .filter_map(|v| v.display().parse().ok())
            .collect()
    }
    pub fn actions(&self) -> Vec<&'static str> {
        self.available_actions()
            .into_iter()
            .map(TimerAction::as_str)
            .collect()
    }
}

/// Compute an undoable edit at action execution time, never when the menu was opened.
pub fn edit(
    workspace: &Workspace,
    path: &Path,
    name: &str,
    action: &str,
    now: DateTime<FixedOffset>,
) -> Result<(Symbol, TextEdit), String> {
    edit_in(
        &crate::RequestContext::new(workspace, now),
        path,
        name,
        action,
    )
}
pub fn edit_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    name: &str,
    action: &str,
) -> Result<(Symbol, TextEdit), String> {
    let workspace = request.workspace();
    let now = request.now();

    let Value::Timer(timer) = request.engine().named(path, name)? else {
        return Err("Expected a named timer".into());
    };
    let origin = timer
        .origin
        .as_ref()
        .ok_or("Controls require a direct named countdown or stopwatch declaration")?;
    let SymbolKind::Definition(index) = origin.kind else {
        return Err("Expected timer definition".into());
    };
    let doc = &workspace.documents[&origin.path];
    let def = &doc.definitions[index];
    let original = timer_arguments(&def.source).ok_or("Expected timer declaration")?;
    let expression = crate::plugins::standard(
        "timer",
        "transition",
        vec![
            timer.record(),
            Value::Text(action.into()),
            Value::Text(original.first().copied().unwrap_or_default().into()),
        ],
        now,
    )?
    .display();
    let raw = def.value_span.source(&doc.text);
    let leading = &raw[..raw.len() - raw.trim_start().len()];
    let trailing = &raw[raw.trim_end().len()..];
    let text = format!("{leading}{expression}{trailing}");
    Ok((
        origin.clone(),
        TextEdit::new(def.value_span.range(&doc.text), text),
    ))
}
