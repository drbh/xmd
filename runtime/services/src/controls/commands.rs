//! Typed editor actions. The wire codec and validation are shared by every host;
//! preparation returns effects for the host to deliver without applying them.
use crate::controls::code_actions;
use lang::common::file_path;
use lang::eval::RequestContext;
use lang::eval::resources::Resource;
use lang::eval::timers::TimerAction;
use lsp_types::{Command, TextEdit};
use serde_json::{Value, json};
use std::path::PathBuf;
use url::Url;

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

impl Action {
    /// Every command ID, in declaration order, for the server's
    /// `executeCommandProvider`.
    pub fn commands() -> impl Iterator<Item = &'static str> {
        <CommandId as strum::VariantArray>::VARIANTS
            .iter()
            .map(|command| command.into())
    }
    /// The LSP command ID this action travels under.
    pub fn id(&self) -> &'static str {
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
    pub fn command(&self, title: impl Into<String>) -> Command {
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
        if !module.has(lang::eval::modules::Hook::Reduce) {
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
                crate::providers::reduce(
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
                code_actions::apply_edits(&doc.text, edits)?;
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
                let edits = crate::controls::code_actions::toggle_task(request, &path, index)?;
                Ok(PreparedAction::Edit { path, edits })
            }
            Self::Timer {
                document,
                name,
                action,
            } => {
                let (origin, span, text) =
                    lang::eval::timers::edit_in(request, &document_path(document)?, name, *action)?;
                let range = span.range(&request.workspace().documents[&origin.path].text);
                Ok(PreparedAction::Edit {
                    path: origin.path,
                    edits: vec![TextEdit::new(range, text)],
                })
            }
            Self::OpenResource { target, url } | Self::RefreshResource { target, url } => {
                let path = target.validate(request)?;
                let resource = crate::controls::rows::resources_at(request, &path, target.row)
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
    file_path(url)
}
