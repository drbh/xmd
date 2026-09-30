//! The one list of task and appointment attributes. Each row names a variant,
//! the key a note writes after `@`, what its value holds, which lines take it
//! and how signature help describes it, so the parser's highlighting and its
//! "Unknown attribute" check, the diagnostics, the editor's highlighting and
//! completion, the code actions that write attributes and the generated
//! reference all read the same facts and a new attribute cannot be half-added.

use chrono::NaiveDate;

/// What an attribute's value holds, which decides how it is highlighted, how
/// it is checked and whether it is an expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    /// A date or timestamp: relative text such as `tomorrow` or `next friday`,
    /// or an expression that evaluates to one (`2026-09-18`, `launch - 2d`).
    When,
    /// A calendar date the editor stamps, `YYYY-MM-DD` and nothing else: it is
    /// written by an action and read back verbatim, never evaluated.
    Stamp,
    /// An expression that evaluates to a duration.
    Duration,
    /// Comma-separated expressions, each a Boolean or a checklist.
    Dependencies,
    /// The name of a stopwatch or countdown, written as an expression.
    Timer,
    /// Recurrence text: `day`, `week`, `month`, `year` or a whole-day duration.
    Recurrence,
    /// Comma-separated tag names.
    Tags,
}
impl AttributeValue {
    /// Whether the value is an expression the engine evaluates: it is lexed
    /// and highlighted as one, and rename and extract refactors look inside it.
    /// A `When` may instead be relative text, which callers check first.
    pub const fn is_expression(self) -> bool {
        matches!(
            self,
            Self::When | Self::Duration | Self::Dependencies | Self::Timer
        )
    }
    /// Whether the value is a date, which highlighting paints as one when it
    /// reads as a date without evaluating anything.
    pub const fn is_date(self) -> bool {
        matches!(self, Self::When | Self::Stamp)
    }
}

/// Which lines take an attribute. A line with `@at` and no checkbox is an
/// appointment; every other attribute belongs on a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    Task,
    TaskOrAppointment,
}
impl Applies {
    /// How signature help and the reference name what the attribute is.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task attribute",
            Self::TaskOrAppointment => "task or appointment attribute",
        }
    }
}

/// The date a `Stamp` attribute holds, when its text is one.
pub fn stamp(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

/// Declare the attributes once. A row is
/// `Variant => "key", Value, Applies, [params], documentation, example;`.
macro_rules! attributes {
    ($($variant:ident => $name:literal, $value:ident, $applies:ident, [$($param:literal),*], $documentation:expr, $example:literal;)*) => {
        /// An attribute key, named rather than spelled out at every call site.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum AttributeKey {
            $($variant,)*
        }
        impl AttributeKey {
            /// Every attribute, in the order the editor presents them.
            pub const ALL: &'static [AttributeKey] = &[$(AttributeKey::$variant,)*];
            /// The key a note writes after `@`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(AttributeKey::$variant => $name,)*
                }
            }
            /// How a note writes it, `@` and all, as signature help and
            /// completion name it.
            pub const fn spelling(self) -> &'static str {
                match self {
                    $(AttributeKey::$variant => concat!("@", $name),)*
                }
            }
            /// What the value holds.
            pub const fn value(self) -> AttributeValue {
                match self {
                    $(AttributeKey::$variant => AttributeValue::$value,)*
                }
            }
            /// Which lines take it.
            pub const fn applies(self) -> Applies {
                match self {
                    $(AttributeKey::$variant => Applies::$applies,)*
                }
            }
            /// Its parameters, as signature help shows them.
            pub const fn params(self) -> &'static [&'static str] {
                match self {
                    $(AttributeKey::$variant => &[$($param),*],)*
                }
            }
            /// What it means, for signature help, completion and the reference.
            pub const fn documentation(self) -> &'static str {
                match self {
                    $(AttributeKey::$variant => $documentation,)*
                }
            }
            /// The value signature help and completion fill in.
            pub const fn example(self) -> &'static str {
                match self {
                    $(AttributeKey::$variant => $example,)*
                }
            }
        }
        impl std::str::FromStr for AttributeKey {
            type Err = ();
            fn from_str(name: &str) -> Result<Self, Self::Err> {
                match name {
                    $($name => Ok(AttributeKey::$variant),)*
                    _ => Err(()),
                }
            }
        }
    };
}

attributes! {
    Timer => "timer", Timer, Task, ["timer: Timer"],
        "Associate a named timer with this task. Completion does not stop the timer.",
        "focus";
    Due => "due", When, Task, ["date: Date or DateTime"],
        "Deadline. Accepts a named date, an expression, or relative input such as tomorrow. Use Freeze relative date to capture it.",
        "tomorrow";
    Scheduled => "scheduled", When, Task, ["date: Date or DateTime"],
        "Planned work date; separate from the deadline.",
        "tomorrow";
    At => "at", When, TaskOrAppointment, ["time: Date or DateTime"],
        "Appointment time; a line with @at and no checkbox is an appointment. Include an explicit UTC offset for ambiguous local times.",
        "2026-09-18T14:00-04:00";
    Estimate => "estimate", Duration, Task, ["effort: Duration"],
        "A nonnegative estimate. Examples: 30s, 20m, 2h.",
        "20m";
    After => "after", Dependencies, Task, ["dependency: Boolean or Checklist", "more dependencies..."],
        "Block this task until all dependencies are satisfied. Cycles are reported with source locations.",
        "task_name";
    Every => "every", Recurrence, Task, ["interval: recurrence"],
        "Repeat a leaf task: day, week, month, year, or a positive whole-day duration such as 2w.",
        "week";
    Tag => "tag", Tags, Task, ["tag: name"],
        "Tag a task for filtering. Multiple tags may be comma-separated.",
        "errands";
    RepeatFrom => "repeat_from", Stamp, Task, ["anchor: Date"],
        "The date a recurring task's @every counts from, as YYYY-MM-DD. Completing the task writes it once, from @due or today, so each later @due stays on the same cycle (a monthly bill anchored on the 31st comes back on the last day of every month) however early or late it is checked off.",
        "2026-01-31";
    Completed => "completed", Stamp, Task, ["date: Date"],
        "The day the task was checked off, as YYYY-MM-DD. Checking a task stamps today's date on it and its subtasks; unchecking removes the stamp. The checkbox, not the stamp, decides whether the task is done.",
        "2026-09-18";
}

impl std::fmt::Display for AttributeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_parses_back() {
        for key in AttributeKey::ALL {
            assert_eq!(key.as_str().parse::<AttributeKey>(), Ok(*key));
            assert_eq!(key.spelling(), format!("@{key}"));
        }
    }
}
