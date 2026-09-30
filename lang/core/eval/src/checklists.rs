//! The counts a named checklist heading answers: `total`, `completed`,
//! `remaining` and `effort`. Whether a task is done is the engine's
//! (`Engine::task_done`), since a task name evaluates to exactly that.
use crate::engine::{Builtin, Engine, Expr, Value};
use std::path::Path;
use values::{EvalError, EvalResult, Overflow};

/// The checklist built-ins, as `features` registers them.
pub(crate) fn counts(
    engine: &mut Engine<'_>,
    path: &Path,
    builtin: Builtin,
    args: &[Expr],
) -> EvalResult<Value> {
    let [arg] = args else {
        return Err(EvalError::Arity(builtin));
    };
    let Value::Tasks(tasks) = engine.expr(path, arg)?.plain() else {
        return Err(EvalError::Message(format!(
            "{builtin} expects a named checklist heading"
        )));
    };
    let done = tasks
        .iter()
        .filter(|(p, i)| engine.task_done(p, *i))
        .count();
    match builtin {
        Builtin::Total => Ok(Value::Count(tasks.len())),
        Builtin::Completed => Ok(Value::Count(done)),
        Builtin::Remaining => Ok(Value::Count(tasks.len() - done)),
        Builtin::Effort => {
            let mut seconds = 0i64;
            for (p, i) in tasks {
                if !engine.task_done(&p, i) {
                    let task = &engine.workspace().documents[&p].tasks[i];
                    if let Some(attr) = task.attributes.get(syntax::AttributeKey::Estimate.as_str())
                    {
                        let Value::Duration(m) = engine.eval(&p, &attr.value)? else {
                            return Err(EvalError::Message("@estimate requires a duration".into()));
                        };
                        if m < 0 {
                            return Err(EvalError::Message("Estimate cannot be negative".into()));
                        }
                        seconds = seconds
                            .checked_add(m)
                            .ok_or(EvalError::Overflowed(Overflow::Duration))?;
                    }
                }
            }
            Ok(Value::Duration(seconds))
        }
        _ => Err(EvalError::UnknownFunction(builtin.to_string())),
    }
}
