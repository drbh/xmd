//! Typed editor actions: the wire codec every host shares, and the one shape
//! every control has. A provider proposes controls for a whole note at once
//! (each a [`Proposal`]: a line, a title and an action) and owns how the
//! action kinds it declares are prepared ([`ActionProvider`]). Preparation
//! validates against the execution-time snapshot and clock, and returns
//! effects for the host to deliver without applying them. `providers` is
//! the one place that asks every provider and routes each kind to its owner.
//!
//! The kinds whose argument already carries everything they need (edits,
//! a timer control, showing today) are prepared here, by [`Direct`].
use crate::Request;
use lang::common::file_path;
use lang::eval::engine::Engine;
use lang::eval::resources::Resource;
use lang::eval::timers::TimerAction;
use lang::model::Document;
use lang::stdlib;
use lsp_types::{Command, TextEdit};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowTarget {
    pub document: Url,
    pub row: usize,
    pub expected: String,
}
impl RowTarget {
    /// The row `row` of `doc`, as a control on it expects to find it.
    pub(crate) fn at(uri: &Url, doc: &Document, row: usize) -> Self {
        Self {
            document: uri.clone(),
            row,
            expected: doc.line(row).into(),
        }
    }
    /// The note this target names, while its row still reads as expected.
    pub(crate) fn validate<'a>(
        &self,
        request: &Request<'a>,
    ) -> Result<(PathBuf, &'a Document), String> {
        let (path, doc) = document(request, &self.document)?;
        if self.row >= doc.text.lines().count() || doc.line(self.row) != self.expected {
            return Err(SOURCE_CHANGED.into());
        }
        Ok((path, doc))
    }
}
/// Each variant travels as its own LSP command: the ID editors register, and
/// the noun an error uses when the argument decodes to some other action.
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, strum::EnumDiscriminants,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum_discriminants(
    name(CommandId),
    derive(
        strum::IntoStaticStr,
        strum::EnumString,
        strum::VariantArray,
        strum::EnumMessage
    )
)]
pub enum Action {
    #[strum_discriminants(strum(serialize = "xmd.invoke", message = "an invocation action"))]
    Invoke {
        document: Url,
        expected: String,
        module: String,
        revision: String,
        event: Value,
    },
    #[strum_discriminants(strum(serialize = "xmd.applyEdits", message = "an edit action"))]
    Edit {
        document: Url,
        expected: String,
        edits: Vec<TextEdit>,
    },
    #[strum_discriminants(strum(serialize = "xmd.task", message = "a task action"))]
    ToggleTask(RowTarget),
    #[strum_discriminants(strum(serialize = "xmd.timer", message = "a timer action"))]
    Timer {
        document: Url,
        name: String,
        action: TimerAction,
    },
    #[strum_discriminants(strum(
        serialize = "xmd.openResource",
        message = "an open-resource action"
    ))]
    OpenResource { target: RowTarget, url: Url },
    #[strum_discriminants(strum(
        serialize = "xmd.refreshResource",
        message = "a refresh-resource action"
    ))]
    RefreshResource { target: RowTarget, url: Url },
    #[strum_discriminants(strum(serialize = "xmd.refresh", message = "a refresh action"))]
    Refresh { document: Option<Url> },
    #[strum_discriminants(strum(serialize = "xmd.today", message = "a show-today action"))]
    ShowToday,
}

#[derive(Clone, Copy)]
pub struct Capabilities {
    pub refresh: bool,
    pub views: bool,
}
impl Capabilities {
    pub const NATIVE: Self = Self {
        refresh: true,
        views: true,
    };
    pub const BROWSER: Self = Self {
        refresh: false,
        views: false,
    };
    pub(crate) fn supports(self, action: &Action) -> bool {
        match action {
            Action::Refresh { .. } | Action::RefreshResource { .. } => self.refresh,
            Action::ShowToday => self.views,
            _ => true,
        }
    }
}

#[derive(Debug)]
pub enum PreparedAction {
    Edit { path: PathBuf, edits: Vec<TextEdit> },
    Open { url: Url },
    RefreshResource { resource: Resource },
    Refresh { path: Option<PathBuf> },
    ShowToday,
}

impl Action {
    /// Every command ID, in declaration order, for the server's
    /// `executeCommandProvider`.
    pub fn commands() -> impl Iterator<Item = &'static str> {
        <CommandId as strum::VariantArray>::VARIANTS
            .iter()
            .map(|command| command.into())
    }
    /// The LSP command ID this action travels under.
    pub(crate) fn id(&self) -> &'static str {
        CommandId::from(self).into()
    }
    pub fn document(&self) -> Option<&Url> {
        match self {
            Self::ToggleTask(target)
            | Self::OpenResource { target, .. }
            | Self::RefreshResource { target, .. } => Some(&target.document),
            Self::Timer { document, .. }
            | Self::Edit { document, .. }
            | Self::Invoke { document, .. } => Some(document),
            Self::Refresh { document } => document.as_ref(),
            Self::ShowToday => None,
        }
    }
    /// One command ID per variant, and the action itself as the sole argument.
    pub(crate) fn command(&self, title: impl Into<String>) -> Command {
        Command {
            title: title.into(),
            command: self.id().into(),
            arguments: Some(vec![json!(self)]),
        }
    }
    pub fn decode(command: &str, args: &[Value]) -> Result<Self, String> {
        let expected: CommandId = command
            .parse()
            .map_err(|_| format!("Unknown command: {command}"))?;
        let [argument] = args else {
            return Err(format!("{command} expects a single action argument"));
        };
        let action: Self = serde_json::from_value(argument.clone()).map_err(|e| e.to_string())?;
        if CommandId::from(&action) != expected {
            return Err(format!(
                "Expected {}",
                strum::EnumMessage::get_message(&expected).unwrap_or_default()
            ));
        }
        Ok(action)
    }
    /// What every action must pass before its owner prepares it: the host
    /// can carry it out, and the note it names is still in the workspace.
    pub(crate) fn admit(
        &self,
        request: &Request<'_>,
        capabilities: Capabilities,
    ) -> Result<(), String> {
        if !capabilities.supports(self) {
            return Err("This command is not available in this host".into());
        }
        if let Some(url) = self.document() {
            document(request, url)?;
        }
        Ok(())
    }
}

/// How a host prefers to offer completing or reopening a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskToggle {
    /// A command the host executes, listed with the row's other controls.
    Command,
    /// An edit the host applies itself, shown disabled when it is blocked.
    Action,
}

/// Which rows of a note a caller wants controls for.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Rows {
    /// Every row: the note's lenses.
    All,
    /// One row: the controls among a range's code actions.
    One(usize),
}
impl Rows {
    pub(crate) fn has(self, row: usize) -> bool {
        match self {
            Self::All => true,
            Self::One(one) => one == row,
        }
    }
}

/// What a caller asks every provider for.
#[derive(Clone, Copy)]
pub(crate) struct Ask {
    pub rows: Rows,
    pub toggle: TaskToggle,
    pub capabilities: Capabilities,
}

/// One control a provider offers: an action on a line of the note, and what
/// the control is called.
pub(crate) struct Proposal {
    pub line: usize,
    pub title: String,
    pub action: Action,
}

/// What preparing an action came to: an effect for the host, or another
/// action to prepare in its place (a module's reducer answers an invocation
/// with the concrete action it stands for).
pub(crate) enum Prepared {
    Effect(PreparedAction),
    Reduced(Action),
}
impl From<PreparedAction> for Prepared {
    fn from(effect: PreparedAction) -> Self {
        Self::Effect(effect)
    }
}

/// One source of controls, built in or a feature module. Every provider has
/// the same shape: it proposes its controls once per note, and owns how the
/// action kinds it declares prepare, whoever proposed them.
pub(crate) trait ActionProvider: Sync {
    /// The action kinds whose preparation this provider owns.
    fn kinds(&self) -> &'static [CommandId];
    /// Every control it offers on `ask.rows` of the note at `path`, in row
    /// order, computed from facts gathered once for the note. What a
    /// provider proposes it has already validated against those facts.
    fn propose(&self, _request: &Request<'_>, _path: &Path, _ask: Ask) -> Vec<Proposal> {
        Vec::new()
    }
    /// Validate one of its kinds against the current snapshot and clock.
    /// [`Action::admit`] has already passed.
    fn prepare(
        &self,
        request: &Request<'_>,
        action: &Action,
        capabilities: Capabilities,
    ) -> Result<Prepared, String>;
    /// Whether an action someone else proposed would prepare, without
    /// running what preparation defers to execution.
    fn check(
        &self,
        request: &Request<'_>,
        action: &Action,
        capabilities: Capabilities,
    ) -> Result<(), String> {
        self.prepare(request, action, capabilities).map(drop)
    }
}

/// The kinds whose argument carries everything they need: a module's edits,
/// a timer control and showing today. None of them is proposed natively
/// outside a code action; feature modules propose the first two.
pub(crate) struct Direct;
impl ActionProvider for Direct {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::Edit, CommandId::Timer, CommandId::ShowToday]
    }
    fn prepare(
        &self,
        request: &Request<'_>,
        action: &Action,
        _: Capabilities,
    ) -> Result<Prepared, String> {
        match action {
            Action::Edit {
                document: url,
                expected,
                edits,
            } => {
                let (path, doc) = document(request, url)?;
                if doc.text != *expected {
                    return Err(SOURCE_CHANGED.into());
                }
                lang::model::apply_edits(&doc.text, edits)?;
                Ok(PreparedAction::Edit {
                    path,
                    edits: edits.clone(),
                }
                .into())
            }
            Action::Timer {
                document,
                name,
                action,
            } => {
                let (origin, span, text) =
                    lang::eval::timers::edit_in(request, &document_path(document)?, name, *action)?;
                let range = span.range(&request.workspace().documents()[&origin.path]);
                Ok(PreparedAction::Edit {
                    path: origin.path,
                    edits: vec![TextEdit::new(range, text)],
                }
                .into())
            }
            Action::ShowToday => Ok(PreparedAction::ShowToday.into()),
            _ => Err(NOT_MINE.into()),
        }
    }
}

/// What a provider answers for a kind it does not own; routing by
/// [`ActionProvider::kinds`] never asks it to.
pub(crate) const NOT_MINE: &str = "This action belongs to another provider";
pub(crate) const SOURCE_CHANGED: &str = "Source changed; request fresh controls";

/// A control title: a `format.glyph` and the one word that disambiguates it.
pub(crate) fn titled(engine: &mut Engine<'_>, glyph: &str, word: &str) -> String {
    let glyph = stdlib::shown(stdlib::format::glyph(engine, glyph));
    format!("{glyph} {word}")
}

/// The workspace document a URL names, while it is still there.
pub(crate) fn document<'a>(
    request: &Request<'a>,
    url: &Url,
) -> Result<(PathBuf, &'a Document), String> {
    let path = document_path(url)?;
    let doc = request
        .workspace()
        .documents()
        .get(&path)
        .ok_or("Document is no longer in the workspace")?;
    Ok((path, doc))
}
pub(crate) fn document_path(url: &Url) -> Result<PathBuf, String> {
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Document URIs cannot contain queries or fragments".into());
    }
    file_path(url)
}
