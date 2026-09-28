//! Timers are timestamp-based values. Reading/evaluating them never mutates state.
use crate::error::{EvalError, EvalResult};
use crate::{
    engine_impl::{Value, timer_arguments},
    records::{FromValue, TimerRecord, ToValue},
    workspace::{Symbol, SymbolKind},
};
use chrono::{DateTime, FixedOffset};
use common::Span;
use std::path::Path;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, strum::IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TimerAction {
    Start,
    Pause,
    Resume,
    Reset,
}
impl TimerAction {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Where a timer stands, as the timer module reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TimerState {
    Idle,
    Running,
    Paused,
    Done,
}
impl TimerState {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[derive(Clone, Debug)]
pub struct Timer {
    pub limit: Option<i64>,
    pub elapsed: i64,
    pub started: Option<DateTime<FixedOffset>>,
    pub idle: bool,
    pub origin: Option<Symbol>,
    implementation: std::sync::Arc<crate::modules_impl::Module>,
    now: DateTime<FixedOffset>,
}
impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        (
            self.limit,
            self.elapsed,
            self.started,
            self.idle,
            &self.origin,
        ) == (
            other.limit,
            other.elapsed,
            other.started,
            other.idle,
            &other.origin,
        ) && self.implementation.revision() == other.implementation.revision()
    }
}

impl Timer {
    pub fn record(&self) -> Value {
        TimerRecord {
            limit: self.limit,
            elapsed: self.elapsed,
            started: self.started,
            idle: self.idle,
        }
        .to_value()
    }
    pub fn new(
        engine: &mut crate::engine_impl::Engine<'_>,
        name: &str,
        args: &[Value],
    ) -> EvalResult<Self> {
        let implementation = engine
            .workspace()
            .modules
            .active()
            .find(|m| m.id == "timer")
            .ok_or_else(|| EvalError::ModuleUnavailable("timer".into()))?
            .clone();
        let created = engine.call_module(
            "timer",
            "create",
            vec![Value::Text(name.into()), Value::List(args.to_vec())],
        )?;
        let TimerRecord {
            limit,
            elapsed,
            started,
            idle,
        } = TimerRecord::from_value(&created)?;
        Ok(Self {
            limit,
            elapsed,
            started,
            idle,
            origin: None,
            implementation: std::sync::Arc::new(implementation),
            now: engine.now(),
        })
    }
    fn call(&self, name: &str) -> EvalResult<Value> {
        self.implementation
            .call(name, vec![self.record()], self.now)
    }
    pub fn time_dependent(&self) -> EvalResult<bool> {
        if !self.implementation.has("time_dependent") {
            return Ok(self.implementation.live);
        }
        match self.call("time_dependent")? {
            Value::Bool(live) => Ok(live),
            _ => Err(EvalError::Message(
                "timer.time_dependent must return a boolean".into(),
            )),
        }
    }
    /// An unreadable state reads as idle: the glyphs and labels stay drawable.
    pub fn state(&self) -> TimerState {
        self.call("state")
            .ok()
            .and_then(|v| v.display().parse().ok())
            .unwrap_or(TimerState::Idle)
    }
    /// The timer module's words for this timer, or why it has none.
    fn words(&self, name: &str) -> String {
        self.call(name)
            .map(|v| v.display())
            .unwrap_or_else(|e| e.to_string())
    }
    pub fn display(&self) -> String {
        self.words("display")
    }
    pub fn hover(&self) -> String {
        self.words("hover")
    }
    pub fn property(&self, name: &str) -> EvalResult<Value> {
        self.implementation.call(
            "property",
            vec![self.record(), Value::Text(name.into())],
            self.now,
        )
    }
}

pub fn edit_in(
    request: &crate::context::RequestContext<'_>,
    path: &Path,
    name: &str,
    action: TimerAction,
) -> Result<(Symbol, Span, String), String> {
    let workspace = request.workspace();
    let now = request.now();

    let Value::Timer(timer) = request
        .engine()
        .named(path, name)
        .map_err(|e| e.to_string())?
    else {
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
    let expression = workspace
        .modules
        .call(
            "timer",
            "transition",
            vec![
                timer.record(),
                Value::Text(action.as_str().into()),
                Value::Text(original.first().copied().unwrap_or_default().into()),
            ],
            now,
        )
        .map_err(|e| e.to_string())?
        .display();
    let raw = def.value_span.source(&doc.text);
    let leading = &raw[..raw.len() - raw.trim_start().len()];
    let trailing = &raw[raw.trim_end().len()..];
    let text = format!("{leading}{expression}{trailing}");
    Ok((origin.clone(), def.value_span, text))
}
