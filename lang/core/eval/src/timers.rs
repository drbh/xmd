//! Timers are timestamp-based values. Reading/evaluating them never mutates state.
use crate::{
    engine::{Value, timer_arguments},
    workspace::{Symbol, SymbolKind},
};
use chrono::{DateTime, FixedOffset};
use common::Span;
use std::path::Path;
use values::{EvalError, EvalResult};
use values::{Fields, FromValue, ToValue, record};

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

/// Where a timer stands, as the timer module reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TimerState {
    Idle,
    Running,
    Paused,
    Done,
}

#[derive(Clone, Debug)]
pub struct Timer {
    /// What the timer module stores and reads back.
    pub(crate) state: TimerRecord,
    pub origin: Option<Symbol>,
    implementation: std::sync::Arc<modules::Module>,
    now: DateTime<FixedOffset>,
}
impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        (self.state, &self.origin) == (other.state, &other.origin)
            && self.implementation.revision() == other.implementation.revision()
    }
}

impl Timer {
    pub fn record(&self) -> Value {
        self.state.to_value()
    }
    pub fn new(
        engine: &mut crate::engine::Engine<'_>,
        name: &str,
        args: &[Value],
    ) -> EvalResult<Self> {
        let implementation = engine
            .workspace()
            .modules
            .get("timer")
            .ok_or_else(|| EvalError::ModuleUnavailable("timer".into()))?
            .clone();
        let created = engine.call_module(
            "timer",
            "create",
            vec![Value::Text(name.into()), Value::List(args.to_vec())],
        )?;
        Ok(Self {
            state: TimerRecord::from_value(&created)?,
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
        values::words(self.call(name))
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

pub fn edit_timer(
    request: &crate::context::RequestContext<'_>,
    path: &Path,
    name: &str,
    action: TimerAction,
) -> EvalResult<(Symbol, Span, String)> {
    let workspace = request.workspace();
    let now = request.now();

    let value = request.engine().named(path, name)?;
    let Some(timer) = value.downcast::<Timer>() else {
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
                Value::Text(<&str>::from(action).into()),
                Value::Text(original.first().copied().unwrap_or_default().into()),
            ],
            now,
        )?
        .display();
    let raw = def.value_span.source(doc);
    let leading = &raw[..raw.len() - raw.trim_start().len()];
    let trailing = &raw[raw.trim_end().len()..];
    let text = format!("{leading}{expression}{trailing}");
    Ok((origin.clone(), def.value_span, text))
}
record! {
    /// Where a timer stands, as `timer.xmd` reads and writes it.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) struct TimerRecord {
        pub limit: Option<i64>,
        pub elapsed: i64,
        pub started: Option<DateTime<FixedOffset>>,
        pub idle: bool,
    }
}
impl FromValue for TimerRecord {
    fn from_value(value: &Value) -> EvalResult<Self> {
        let fields = Fields::expect(value, "Timer constructor must return a record")?;
        Ok(Self {
            limit: fields.required_or("limit", "Invalid timer limit")?,
            elapsed: fields.required_or("elapsed", "Invalid timer elapsed time")?,
            started: fields.required_or("started", "Invalid timer timestamp")?,
            idle: fields.required_or("idle", "Invalid timer state")?,
        })
    }
}
