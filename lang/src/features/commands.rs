//! Typed editor actions. The wire codec and validation are shared by every host;
//! preparation returns effects for the host to deliver without applying them.
use crate::{RequestContext, actions, paths, resources::Resource, timers::TimerAction};
use lsp_types::{Command, TextEdit, Url};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RowTarget {
    pub document: Url,
    pub row: usize,
    pub expected: String,
}
impl RowTarget {
    fn validate(&self, request: &RequestContext<'_>) -> Result<PathBuf, String> {
        let path = document_path(&self.document)?;
        let doc = request
            .workspace()
            .documents
            .get(&path)
            .ok_or("Document is no longer in the workspace")?;
        if self.row >= doc.text.lines().count() || doc.line(self.row) != self.expected {
            return Err("Source changed; request fresh controls".into());
        }
        Ok(path)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Invoke {
        document: Url,
        expected: String,
        module: String,
        revision: String,
        event: Value,
    },
    Edit {
        document: Url,
        expected: String,
        edits: Vec<TextEdit>,
    },
    ToggleTask(RowTarget),
    Timer {
        document: Url,
        name: String,
        action: TimerAction,
    },
    OpenResource {
        target: RowTarget,
        url: Url,
    },
    RefreshResource {
        target: RowTarget,
        url: Url,
    },
    Refresh {
        document: Option<Url>,
    },
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
    pub fn supports(self, action: &Action) -> bool {
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

/// An LSP command ID, the test for the `Action` variant it carries, and the
/// noun used when the argument decodes to some other kind of action.
pub type CommandId = (&'static str, fn(&Action) -> bool, &'static str);

impl Action {
    /// The one place an LSP command ID is tied to a variant: the ID editors
    /// register, how an action recognizes itself, and what to say when the
    /// argument decodes to some other kind of action.
    pub const COMMAND_IDS: &'static [CommandId] = &[
        (
            "wtf.invoke",
            |a| matches!(a, Self::Invoke { .. }),
            "an invocation action",
        ),
        (
            "wtf.applyEdits",
            |a| matches!(a, Self::Edit { .. }),
            "an edit action",
        ),
        (
            "wtf.task",
            |a| matches!(a, Self::ToggleTask(_)),
            "a task action",
        ),
        (
            "wtf.timer",
            |a| matches!(a, Self::Timer { .. }),
            "a timer action",
        ),
        (
            "wtf.openResource",
            |a| matches!(a, Self::OpenResource { .. }),
            "an open-resource action",
        ),
        (
            "wtf.refreshResource",
            |a| matches!(a, Self::RefreshResource { .. }),
            "a refresh-resource action",
        ),
        (
            "wtf.refresh",
            |a| matches!(a, Self::Refresh { .. }),
            "a refresh action",
        ),
        (
            "wtf.today",
            |a| matches!(a, Self::ShowToday),
            "a show-today action",
        ),
    ];
    /// Those IDs, in order, for the server's `executeCommandProvider`.
    pub const COMMANDS: [&'static str; Self::COMMAND_IDS.len()] = {
        let mut ids = [""; Self::COMMAND_IDS.len()];
        let mut i = 0;
        while i < ids.len() {
            ids[i] = Self::COMMAND_IDS[i].0;
            i += 1;
        }
        ids
    };
    /// The LSP command ID this action travels under.
    pub fn id(&self) -> &'static str {
        Self::COMMAND_IDS
            .iter()
            .find(|(_, is_kind, _)| is_kind(self))
            .expect("every action variant has a command ID")
            .0
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
    pub fn command(&self, title: impl Into<String>) -> Command {
        Command {
            title: title.into(),
            command: self.id().into(),
            arguments: Some(vec![json!(self)]),
        }
    }
    pub fn decode(command: &str, args: &[Value]) -> Result<Self, String> {
        let (_, is_kind, expected) = Self::COMMAND_IDS
            .iter()
            .find(|(id, _, _)| *id == command)
            .ok_or_else(|| format!("Unknown command: {command}"))?;
        let [argument] = args else {
            return Err(format!("{command} expects a single action argument"));
        };
        let action: Self = serde_json::from_value(argument.clone()).map_err(|e| e.to_string())?;
        if !is_kind(&action) {
            return Err(format!("Expected {expected}"));
        }
        Ok(action)
    }
    pub(crate) fn validate_invocation(&self, request: &RequestContext<'_>) -> Result<(), String> {
        let Self::Invoke {
            document,
            expected,
            module,
            revision,
            ..
        } = self
        else {
            return Err("Expected a module invocation".into());
        };
        let path = document_path(document)?;
        let doc = request
            .workspace()
            .documents
            .get(&path)
            .ok_or("Document is no longer in the workspace")?;
        if doc.text != *expected {
            return Err("Source changed; request fresh controls".into());
        }
        let module = request
            .workspace()
            .modules
            .active()
            .find(|m| m.id == *module)
            .ok_or("Module is no longer available")?;
        if module.revision() != *revision {
            return Err("Module changed; request fresh controls".into());
        }
        if !module.has(crate::modules::Hook::Reduce) {
            return Err("Module has no reducer".into());
        }
        Ok(())
    }
    /// Validate against the current workspace and execution-time clock. The host
    /// still owns version checks, applying edits, opening URLs and refreshing data.
    pub fn prepare(
        &self,
        request: &RequestContext<'_>,
        capabilities: Capabilities,
    ) -> Result<PreparedAction, String> {
        if !capabilities.supports(self) {
            return Err("This command is not available in this host".into());
        }
        if let Some(document) = self.document() {
            let path = document_path(document)?;
            if !request.workspace().documents.contains_key(&path) {
                return Err("Document is no longer in the workspace".into());
            }
        }
        match self {
            Self::Invoke {
                document,
                module,
                event,
                ..
            } => {
                self.validate_invocation(request)?;
                let module = request
                    .workspace()
                    .modules
                    .active()
                    .find(|m| m.id == *module)
                    .ok_or("Module is no longer available")?;
                super::modules::reduce(
                    request,
                    &document_path(document)?,
                    module,
                    event,
                    capabilities,
                )?
                .prepare(request, capabilities)
            }
            Self::Edit {
                document,
                expected,
                edits,
            } => {
                let path = document_path(document)?;
                let doc = &request.workspace().documents[&path];
                if doc.text != *expected {
                    return Err("Source changed; request fresh controls".into());
                }
                actions::apply_edits(&doc.text, edits)?;
                Ok(PreparedAction::Edit {
                    path,
                    edits: edits.clone(),
                })
            }
            Self::ToggleTask(target) => {
                let path = target.validate(request)?;
                let index = request.workspace().documents[&path]
                    .tasks
                    .iter()
                    .position(|t| t.line == target.row)
                    .ok_or("No task at this line")?;
                let edits = request.toggle_task(&path, index)?;
                Ok(PreparedAction::Edit { path, edits })
            }
            Self::Timer {
                document,
                name,
                action,
            } => {
                let (origin, edit) =
                    crate::timers::edit_in(request, &document_path(document)?, name, *action)?;
                Ok(PreparedAction::Edit {
                    path: origin.path,
                    edits: vec![edit],
                })
            }
            Self::OpenResource { target, url } | Self::RefreshResource { target, url } => {
                let path = target.validate(request)?;
                let resource = request
                    .resources_at(&path, target.row)
                    .into_iter()
                    .find(|r| r.url(&path).is_ok_and(|u| u == *url))
                    .ok_or("Resource changed; request fresh controls")?;
                if matches!(self, Self::OpenResource { .. }) {
                    Ok(PreparedAction::Open { url: url.clone() })
                } else if request
                    .link_features()
                    .refresh_request(&resource.target)
                    .is_some()
                {
                    Ok(PreparedAction::RefreshResource { resource })
                } else {
                    Err("This resource does not support refresh".into())
                }
            }
            Self::Refresh { document } => Ok(PreparedAction::Refresh {
                path: document.as_ref().map(document_path).transpose()?,
            }),
            Self::ShowToday => Ok(PreparedAction::ShowToday),
        }
    }
}
fn document_path(url: &Url) -> Result<PathBuf, String> {
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Document URIs cannot contain queries or fragments".into());
    }
    paths::file_path(url)
}
