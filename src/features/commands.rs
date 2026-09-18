//! Typed editor actions. The wire codec and validation are shared by every host;
//! preparation returns effects for the host to deliver without applying them.
use crate::{
    RequestContext, actions, interaction, paths, resources::Resource, timers::TimerAction,
};
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
    fn arguments(&self) -> Vec<Value> {
        vec![json!(self.document), json!(self.row), json!(self.expected)]
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
    Refresh,
    ShowToday,
}

impl Action {
    pub const COMMANDS: &'static [&'static str] = &[
        "wtf.invoke",
        "wtf.applyEdits",
        "wtf.task",
        "wtf.timer",
        "wtf.openResource",
        "wtf.refreshResource",
        "wtf.refresh",
        "wtf.today",
    ];
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
    /// Retain the existing LSP command IDs and argument arrays at the boundary.
    pub fn command(&self, title: impl Into<String>) -> Command {
        let (command, arguments) = match self {
            Self::Invoke { .. } => ("wtf.invoke", vec![json!(self)]),
            Self::Edit { .. } => ("wtf.applyEdits", vec![json!(self)]),
            Self::ToggleTask(target) => ("wtf.task", target.arguments()),
            Self::Timer {
                document,
                name,
                action,
            } => (
                "wtf.timer",
                vec![json!(document), json!(name), json!(action.as_str())],
            ),
            Self::OpenResource { target, url } | Self::RefreshResource { target, url } => {
                let mut args = target.arguments();
                args.push(json!(url));
                (
                    if matches!(self, Self::OpenResource { .. }) {
                        "wtf.openResource"
                    } else {
                        "wtf.refreshResource"
                    },
                    args,
                )
            }
            Self::Refresh { document } => {
                ("wtf.refresh", document.iter().map(|u| json!(u)).collect())
            }
            Self::ShowToday => ("wtf.today", vec![]),
        };
        Command {
            title: title.into(),
            command: command.into(),
            arguments: Some(arguments),
        }
    }
    pub fn decode(command: &str, args: &[Value]) -> Result<Self, String> {
        let exact = |count| {
            if args.len() == count {
                Ok(())
            } else {
                Err(format!("{command} expects {count} arguments"))
            }
        };
        let text = |i: usize| {
            args.get(i)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Expected text argument {} for {command}", i + 1))
        };
        let document = || {
            let url = Url::parse(text(0)?).map_err(|e| e.to_string())?;
            document_path(&url)?;
            Ok::<_, String>(url)
        };
        let row_target = || {
            Ok::<_, String>(RowTarget {
                document: document()?,
                row: args
                    .get(1)
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("Expected line number")?,
                expected: text(2)?.into(),
            })
        };
        match command {
            "wtf.invoke" => {
                exact(1)?;
                let action: Self =
                    serde_json::from_value(args[0].clone()).map_err(|e| e.to_string())?;
                if !matches!(action, Self::Invoke { .. }) {
                    return Err("Expected an invocation action".into());
                }
                Ok(action)
            }
            "wtf.applyEdits" => {
                exact(1)?;
                let action: Self =
                    serde_json::from_value(args[0].clone()).map_err(|e| e.to_string())?;
                if !matches!(action, Self::Edit { .. }) {
                    return Err("Expected an edit action".into());
                }
                Ok(action)
            }
            "wtf.task" => {
                exact(3)?;
                Ok(Self::ToggleTask(row_target()?))
            }
            "wtf.timer" => {
                exact(3)?;
                Ok(Self::Timer {
                    document: document()?,
                    name: text(1)?.into(),
                    action: text(2)?.parse()?,
                })
            }
            "wtf.openResource" | "wtf.refreshResource" => {
                exact(4)?;
                let target = row_target()?;
                let url = Url::parse(text(3)?).map_err(|e| e.to_string())?;
                Ok(if command == "wtf.openResource" {
                    Self::OpenResource { target, url }
                } else {
                    Self::RefreshResource { target, url }
                })
            }
            "wtf.refresh" => {
                if args.len() > 1 {
                    return Err("wtf.refresh expects zero arguments or a document URI".into());
                }
                Ok(Self::Refresh {
                    document: if args.is_empty() {
                        None
                    } else {
                        Some(document()?)
                    },
                })
            }
            "wtf.today" => {
                exact(0)?;
                Ok(Self::ShowToday)
            }
            _ => Err(format!("Unknown command: {command}")),
        }
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
                super::module_inlays::reduce(
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
                let edits = actions::toggle_task_in(request, &path, index)?;
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
                let resource = interaction::resources_at_in(request, &path, target.row)
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
            Self::Refresh { .. } => Ok(PreparedAction::Refresh),
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
