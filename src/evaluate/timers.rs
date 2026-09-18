//! Timers are timestamp-based values. Reading/evaluating them never mutates state.
use crate::{
    engine::{Engine, Value, timer_arguments},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset};
use lsp_types::TextEdit;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    pub limit: Option<i64>,
    pub elapsed: i64,
    pub started: Option<DateTime<FixedOffset>>,
    pub idle: bool,
    pub origin: Option<Symbol>,
}

impl Timer {
    /// stopwatch([elapsed [, started]]) / countdown(duration [, elapsed [, started]])
    pub fn new(name: &str, args: &[Value], now: DateTime<FixedOffset>) -> Result<Self, String> {
        let countdown = name == "countdown";
        let offset = usize::from(countdown);
        if args.len() < offset || args.len() > offset + 2 {
            return Err(if countdown {
                "countdown expects duration, optionally followed by elapsed and a start timestamp"
            } else {
                "stopwatch accepts optional elapsed and a start timestamp"
            }
            .into());
        }
        let duration = |v: &Value| match v {
            Value::Duration(s) if *s >= 0 => Ok(*s),
            _ => Err("Timer durations must be nonnegative whole seconds".to_string()),
        };
        let limit = if countdown {
            let s = duration(&args[0])?;
            if s == 0 {
                return Err("Countdown duration must be greater than zero".into());
            }
            Some(s)
        } else {
            None
        };
        let accumulated = args.get(offset).map(duration).transpose()?.unwrap_or(0);
        let started = args
            .get(offset + 1)
            .map(|v| match v {
                Value::DateTime(t) => Ok(*t),
                _ => Err(
                    "Timer start must be a timestamp with a time, e.g. 2026-09-16T14:00:00-04:00"
                        .to_string(),
                ),
            })
            .transpose()?;
        // A backwards wall-clock adjustment must never produce negative elapsed time.
        let since = started.map(|t| (now - t).num_seconds().max(0)).unwrap_or(0);
        let elapsed = accumulated
            .checked_add(since)
            .ok_or("Timer duration overflow")?;
        Ok(Self {
            limit,
            elapsed: limit.map(|n| elapsed.min(n)).unwrap_or(elapsed),
            started,
            idle: args.len() == offset,
            origin: None,
        })
    }
    pub fn done(&self) -> bool {
        self.limit.is_some_and(|n| self.elapsed >= n)
    }
    pub fn running(&self) -> bool {
        self.started.is_some() && !self.done()
    }
    pub fn state(&self) -> &'static str {
        if self.done() {
            "done"
        } else if self.idle {
            "idle"
        } else if self.running() {
            "running"
        } else {
            "paused"
        }
    }
    pub fn display(&self) -> String {
        let time = clock(self.limit.map(|n| n - self.elapsed).unwrap_or(self.elapsed));
        let state = format!(
            "{} {}",
            crate::glyphs::timer_state(self.state()),
            self.state()
        );
        if self.limit.is_some() {
            format!("{} {time} remaining · {state}", crate::glyphs::COUNTDOWN)
        } else {
            format!("{} {time} elapsed · {state}", crate::glyphs::STOPWATCH)
        }
    }
    pub fn property(&self, name: &str) -> Result<Value, String> {
        match name {
            "elapsed" => Ok(Value::Duration(self.elapsed)),
            "remaining" => self
                .limit
                .map(|n| Value::Duration(n - self.elapsed))
                .ok_or("Only countdowns have .remaining".into()),
            "duration" => self
                .limit
                .map(Value::Duration)
                .ok_or("Only countdowns have .duration".into()),
            "running" => Ok(Value::Bool(self.running())),
            "done" => Ok(Value::Bool(self.done())),
            "state" => Ok(Value::Text(self.state().into())),
            _ => Err(format!("Unknown timer property '{name}'")),
        }
    }
    pub fn actions(&self) -> Vec<&'static str> {
        let mut actions = match self.state() {
            "idle" => vec!["start"],
            "running" => vec!["pause"],
            "paused" => vec!["resume"],
            _ => vec![],
        };
        if !self.idle {
            actions.push("reset");
        }
        actions
    }
}
fn clock(seconds: i64) -> String {
    if seconds < 3600 {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
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
    let Value::Timer(timer) = Engine::at(workspace, now).named(path, name)? else {
        return Err("Expected a named timer".into());
    };
    if !timer.actions().contains(&action) {
        return Err(format!("Cannot {action} a {} timer", timer.state()));
    }
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
    let mut args: Vec<String> = if timer.limit.is_some() {
        vec![original[0].to_owned()]
    } else {
        vec![]
    };
    if action != "reset" {
        args.push(format!("{}s", timer.elapsed));
    }
    if matches!(action, "start" | "resume") {
        args.push(now.to_rfc3339());
    }
    let kind = if timer.limit.is_some() {
        "countdown"
    } else {
        "stopwatch"
    };
    let raw = &doc.line(def.value_span.line)[def.value_span.start..def.value_span.end];
    let leading = &raw[..raw.len() - raw.trim_start().len()];
    let trailing = &raw[raw.trim_end().len()..];
    let text = format!("{leading}{kind}({}){trailing}", args.join(", "));
    Ok((
        origin.clone(),
        TextEdit::new(def.value_span.range(&doc.text), text),
    ))
}
