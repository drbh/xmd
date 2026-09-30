//! Timers are timestamp-based values. Reading/evaluating them never mutates state.
use crate::{
    contract::{Held, Presented, Snapshot, shown, timer},
    engine::{Builtin, Engine, Expr, Value, timer_arguments},
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
    /// Where the module says the timer stands, read once when it is created:
    /// a timer whose state cannot be read fails to evaluate instead.
    status: TimerState,
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
    pub(crate) fn new(engine: &mut Engine<'_>, name: &str, args: &[Value]) -> EvalResult<Self> {
        let implementation = engine
            .workspace()
            .modules
            .get("timer")
            .ok_or_else(|| EvalError::ModuleUnavailable("timer".into()))?
            .clone();
        let record = timer::create(engine, name, args.to_vec())?;
        let mut created = Self {
            state: TimerRecord::from_value(&record)?,
            status: TimerState::Idle,
            origin: None,
            implementation: std::sync::Arc::new(implementation),
            now: engine.now(),
        };
        // The state is decided once, at the clock the timer was read at, so
        // a module that cannot say where it stands fails here, where the
        // definition's diagnostic reports it.
        let status = timer::state(&mut created.held(), created.record())?;
        created.status = status.parse().map_err(|_| {
            EvalError::Message(format!(
                "timer.state returned '{status}'; expected idle, running, paused or done"
            ))
        })?;
        Ok(created)
    }
    /// The module this timer was created with, at the moment it was read.
    fn held(&self) -> Held<'_> {
        Held {
            module: &self.implementation,
            now: self.now,
        }
    }
    pub fn time_dependent(&self) -> EvalResult<bool> {
        if !self.implementation.has("time_dependent") {
            return Ok(self.implementation.live);
        }
        timer::time_dependent(&mut self.held(), self.record())
    }
    /// Where the timer stands, as the module decided when it was created.
    pub(crate) fn state(&self) -> TimerState {
        self.status
    }
    pub fn display(&self) -> String {
        shown(self.presentations().0)
    }
    pub fn hover(&self) -> String {
        shown(self.presentations().1)
    }
    /// The timer's display and hover as the module presents them, for a
    /// caller that reports a failure as well as showing the fallback.
    pub fn presentations(&self) -> (Presented, Presented) {
        (
            timer::display(&mut self.held(), self.record()),
            timer::hover(&mut self.held(), self.record()),
        )
    }
    pub fn property(&self, name: &str) -> EvalResult<Value> {
        timer::property(&mut self.held(), self.record(), name)
    }
}

/// `stopwatch(…)` and `countdown(…)`, as `features` registers them: the timer
/// module resolves the state.
pub(crate) fn call(
    engine: &mut Engine<'_>,
    path: &Path,
    builtin: Builtin,
    args: &[Expr],
) -> EvalResult<Value> {
    let values = engine.values(path, args)?;
    let time_dependent = engine.time_dependent;
    let timer = Timer::new(engine, builtin.as_str(), &values)?;
    // The module declares whether this resolved state still needs a clock.
    engine.time_dependent = time_dependent || timer.time_dependent()?;
    Ok(Value::Host(std::sync::Arc::new(timer)))
}

/// A definition written as a direct `countdown(…)` or `stopwatch(…)` names
/// its timer, which is what lets a control rewrite that definition.
pub(crate) fn adopt(value: Value, symbol: &Symbol, source: &str) -> Value {
    match value.downcast::<Timer>() {
        Some(timer) if timer.origin.is_none() && timer_arguments(source).is_some() => {
            let mut timer = timer.clone();
            timer.origin = Some(symbol.clone());
            Value::Host(std::sync::Arc::new(timer))
        }
        _ => value,
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
    let expression = timer::transition(
        &mut Snapshot {
            modules: &workspace.modules,
            now,
        },
        timer.record(),
        action.into(),
        original.first().copied().unwrap_or_default(),
    )?;
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
