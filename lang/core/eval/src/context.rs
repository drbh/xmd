//! Immutable request inputs and a cache shared by independent evaluator sessions.
use crate::{
    engine::{Engine, MemoEntry, MemoKey, Value},
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, NaiveDate};
use modules::LinkFeatures;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

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

/// Evaluated results by what they are the value of, shared by every engine
/// of one request.
pub(crate) type Memo = Arc<Mutex<BTreeMap<MemoKey, MemoEntry>>>;

/// One workspace snapshot, clock, provider registry and memo for a host request.
/// Engines share cached results and their dependencies, while keeping transient
/// errors, recursion stacks and row scopes separate. Drop this context after the
/// request; a new clock or workspace revision always starts with an empty cache.
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
    pub fn workspace(&self) -> &'a Workspace {
        self.workspace
    }
    pub fn now(&self) -> DateTime<FixedOffset> {
        self.clock.now
    }
    pub fn today(&self) -> NaiveDate {
        self.clock.today()
    }
    pub fn clock(&self) -> Clock {
        self.clock
    }
    pub fn link_features(&self) -> LinkFeatures<'a> {
        self.links
    }
    pub fn engine(&self) -> Engine<'a> {
        Engine::in_request(self)
    }
    pub fn with_link_features(mut self, links: LinkFeatures<'a>) -> Self {
        self.links = links;
        self.memo = Default::default();
        self
    }
}
