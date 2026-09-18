//! Immutable request inputs and a cache shared by independent evaluator sessions.
use crate::{
    engine::{Engine, MemoEntry, MemoKey, Value},
    link_features::{self, LinkFeatures},
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, NaiveDate};
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
    pub fn engine(self, workspace: &Workspace) -> Engine<'_> {
        RequestContext::new(workspace, self.now).engine()
    }
}

/// One workspace snapshot, clock, provider registry and memo for a host request.
/// Engines share cached results and their dependencies, while keeping transient
/// errors, recursion stacks and row scopes separate. Drop this context after the
/// request; a new clock or workspace revision always starts with an empty cache.
#[derive(Clone)]
pub struct RequestContext<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) clock: Clock,
    pub(crate) today: NaiveDate,
    pub(crate) links: LinkFeatures<'a>,
    pub(crate) memo: Arc<Mutex<BTreeMap<MemoKey, MemoEntry>>>,
}
impl<'a> RequestContext<'a> {
    pub fn new(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self {
            workspace,
            clock: Clock::new(now),
            today: now.date_naive(),
            links: link_features::BUILTINS,
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
        self.today
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
    /// Compatibility for APIs that accept a calendar day separately from `now`.
    pub(crate) fn with_today(mut self, today: NaiveDate) -> Self {
        self.today = today;
        self.memo = Default::default();
        self
    }
}
