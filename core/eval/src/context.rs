//! Immutable request inputs and a cache shared by independent evaluator sessions.
use crate::engine::{Engine, Value};
use crate::memo::{Evaluations, Memo};
use crate::workspace::Workspace;
use chrono::{DateTime, FixedOffset, NaiveDate};
use modules::LinkFeatures;

/// The offset supplied by the host defines calendar dates throughout the request.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub now: DateTime<FixedOffset>,
}
impl Clock {
    pub fn new(now: DateTime<FixedOffset>) -> Self {
        Self { now }
    }
    pub fn today(self) -> NaiveDate {
        self.now.date_naive()
    }
    pub fn date(self, value: &Value) -> Option<NaiveDate> {
        match value {
            Value::Date(d) => Some(*d),
            Value::DateTime(d) => Some(d.with_timezone(self.now.offset()).date_naive()),
            _ => None,
        }
    }
}

/// One workspace snapshot, clock, provider registry and memo for a host request.
/// Engines share cached results and their dependencies, while keeping transient
/// errors, recursion stacks and row scopes separate. Drop this context after the
/// request. A context of its own starts with an empty cache; one
/// [`sharing`](Self::sharing) an owner's [`Evaluations`] reads what earlier
/// requests over the same workspace revision and clock evaluated, answering
/// exactly as one of its own would.
#[derive(Clone)]
pub struct RequestContext<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) clock: Clock,
    pub(crate) links: LinkFeatures<'a>,
    pub(crate) memo: Memo,
}
impl<'a> RequestContext<'a> {
    pub fn new(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self {
            workspace,
            clock: Clock::new(now),
            links: workspace.link_features(),
            memo: Default::default(),
        }
    }
    /// A context reading and adding to `kept`, which its owner keeps only for
    /// as long as `ws` is unchanged.
    pub fn sharing(ws: &'a Workspace, now: DateTime<FixedOffset>, kept: &Evaluations) -> Self {
        let memo = kept.memo(ws, now);
        Self {
            memo,
            ..Self::new(ws, now)
        }
    }
    pub fn workspace(&self) -> &'a Workspace {
        self.workspace
    }
    pub fn now(&self) -> DateTime<FixedOffset> {
        self.clock.now
    }
    pub fn today(&self) -> NaiveDate {
        self.clock.today()
    }
    pub fn link_features(&self) -> LinkFeatures<'a> {
        self.links
    }
    pub fn engine(&self) -> Engine<'a> {
        Engine::in_request(self)
    }
}
