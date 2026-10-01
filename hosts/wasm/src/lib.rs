//! A browser-local workspace. JSON crosses the worker boundary; all language logic stays in Rust.

use chrono::{DateTime, FixedOffset};
use lang::common::file_path;
use lang::document::identifier;
use lang::eval::Workspace;
use lang::eval::modules::CompileModules;
use lsp_types::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use services::commands::{Action, Capabilities, PreparedAction};
use services::{
    Query, RowActions, TOKEN_MODIFIERS, TOKEN_TYPES, WorkspaceSession, definition, folding_ranges,
    fragment, highlights, line_classes, references, rename, semantic_tokens, signature, symbol_at,
};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};
use url::Url;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BrowserWorkspace {
    session: WorkspaceSession,
}

impl Default for BrowserWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl BrowserWorkspace {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            session: WorkspaceSession::browser(Workspace::new(vec!["/workspace".into()])),
        }
    }

    /// Errors are ordinary JSON, so malformed input never needs to trap the worker.
    pub fn request(&mut self, method: &str, payload: &str, now: &str) -> String {
        let result = serde_json::from_str(payload)
            .map_err(|e| e.to_string())
            .and_then(|params| {
                DateTime::parse_from_rfc3339(now)
                    .map_err(|e| e.to_string())
                    .and_then(|time| self.dispatch(method, params, time))
            });
        match result {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error}),
        }
        .to_string()
    }
}

fn field<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(
        params
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Missing {key}"))?,
    )
    .map_err(|e| format!("Invalid {key}: {e}"))
}
fn virtual_path(uri: &str) -> Result<PathBuf, String> {
    let url = Url::parse(uri).map_err(|e| e.to_string())?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("Document URIs cannot contain queries or fragments".into());
    }
    let path = file_path(&url)?;
    if !path.starts_with("/workspace")
        || !lang::common::is_note(&path)
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || path.to_string_lossy().contains(['\0', '\\'])
    {
        return Err(format!(
            "Expected a .{} file within the browser's /workspace",
            lang::common::EXTENSION
        ));
    }
    Ok(path)
}
/// A module source's place in the virtual workspace, from its bare file name.
fn module_path(name: String) -> Result<PathBuf, String> {
    let mut parts = Path::new(&name).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) || !lang::common::is_note(&name)
        || name.contains(['\\', '\0'])
    {
        return Err(format!(
            "Module names must be .{} filenames",
            lang::common::EXTENSION
        ));
    }
    Ok(Path::new("/workspace/.xmd/modules").join(name))
}
fn serialized(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

impl BrowserWorkspace {
    fn dispatch(
        &mut self,
        method: &str,
        params: Value,
        now: DateTime<FixedOffset>,
    ) -> Result<Value, String> {
        match method {
            "semanticLegend" => {
                return Ok(json!({"tokenTypes": TOKEN_TYPES, "tokenModifiers": TOKEN_MODIFIERS}));
            }
            "setResourceData" => {
                let target: String = field(&params, "url")?;
                let metadata = self.session.workspace().link_features().decode_refresh(
                    &target,
                    &field(&params, "data")?,
                    now.to_utc(),
                )?;
                self.session
                    .workspace_mut()
                    .store_link_status(target, metadata);
                return Ok(Value::Null);
            }
            "setModules" => {
                let sources: BTreeMap<String, String> = field(&params, "sources")?;
                let sources = sources
                    .into_iter()
                    .map(|(name, source)| Ok((module_path(name)?, source)))
                    .collect::<Result<BTreeMap<_, _>, String>>()?;
                let modules = lang::eval::modules::ModuleRegistry::compile(sources)?;
                self.session
                    .workspace_mut()
                    .replace_modules(std::sync::Arc::new(modules));
                return Ok(Value::Null);
            }
            "setDocument" => {
                let path = virtual_path(&field::<String>(&params, "uri")?)?;
                let version: i32 = field(&params, "version")?;
                let text: String = field(&params, "text")?;
                if text.len() > 1_000_000 {
                    return Err("Notes are limited to 1 MB in the browser".into());
                }
                self.session.open(&path, version, text)?;
                return Ok(Value::Null);
            }
            "removeDocument" => {
                let path = virtual_path(&field::<String>(&params, "uri")?)?;
                self.session.workspace_mut().remove_document(&path);
                self.session.close(&path);
                return Ok(Value::Null);
            }
            "execute" => {
                if params["versions"] != self.session.versions_json() {
                    return Err(
                        "Notes changed; request fresh controls before applying this action".into(),
                    );
                }
                return self.execute(field(&params, "command")?, now);
            }
            "query" => {
                let compiled = Query::parse(&field::<String>(&params, "query")?)?;
                let only = params
                    .get("uri")
                    .filter(|v| !v.is_null())
                    .map(|_| field::<String>(&params, "uri").and_then(|uri| virtual_path(&uri)))
                    .transpose()?;
                return self.session.query(&compiled, only.as_deref(), now);
            }
            _ => {}
        }
        let uri: String = field(&params, "uri")?;
        let path = virtual_path(&uri)?;
        let doc = self
            .session
            .workspace()
            .documents()
            .get(&path)
            .ok_or("Note is not open in this browser workspace")?;
        let ws = self.session.workspace();
        let request = self.session.request(now);
        let position = || field::<Position>(&params, "position");
        match method {
            "documentLinks" => serialized(request.document_links(&path)),
            "documentSymbols" => serialized(request.document_symbols(&path)),
            "formatting" => serialized(request.formatting(&path)?),
            "folding" => serialized(folding_ranges(doc)),
            "onTypeFormatting" => {
                serialized(request.on_type(&path, position()?, &field::<String>(&params, "ch")?))
            }
            "analyze" | "render" => {
                let inlays = request.hints(
                    &path,
                    Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
                );
                let library = request.workspace().prelude_names(&path);
                // Flat, five numbers a token, as the protocol encodes them.
                let data = semantic_tokens(doc, &library);
                let tokens = serialized(SemanticTokensPartialResult { data })?["data"].take();
                let lenses = request.code_lenses(&path, Capabilities::BROWSER);
                let links = request.document_links(&path);
                let editing = params
                    .get("editing")
                    .and_then(Value::as_bool)
                    .unwrap_or(method == "analyze");
                let diagnostics = request.diagnostics(&path, editing);
                let html = fragment(doc, &library, &inlays.hints, &diagnostics, &links)?;
                Ok(
                    json!({"schemaVersion":1,"engineVersion":env!("CARGO_PKG_VERSION"),"uri":uri,"source":doc.text(),"now":now.to_rfc3339(),"editing":editing,"html":html,"lineClasses":line_classes(doc),"tokenModifiers":TOKEN_MODIFIERS,"version":self.session.version(&path),"versions":self.session.versions_json(),"hints":inlays.hints,"tokens":tokens,"tokenTypes":TOKEN_TYPES,
                    "diagnostics":diagnostics,"lenses":lenses,"links":links,"live":inlays.time_dependent,
                    "symbols":request.document_symbols(&path)}),
                )
            }
            "completion" => serialized(request.completions(&path, position()?, true)),
            "signature" => serialized(signature(request.workspace(), &path, position()?)),
            "hover" => serialized(request.hover(&path, position()?)),
            "definition" | "references" | "highlights" | "prepareRename" | "rename" => {
                let Some((symbol, span)) = symbol_at(ws, &path, position()?) else {
                    return Ok(Value::Null);
                };
                match method {
                    "definition" => serialized(definition(ws, &symbol)),
                    "references" => serialized(references(ws, &symbol)),
                    "highlights" => serialized(highlights(ws, &path, &symbol)),
                    "prepareRename" => {
                        Ok(json!({"range":span.range(doc),"placeholder":ws.named(&symbol).name}))
                    }
                    _ => {
                        let name: String = field(&params, "newName")?;
                        if !identifier(&name) {
                            return Err("Use a name with letters, digits, and underscores".into());
                        }
                        serialized(self.session.edit(rename(ws, &symbol, &name)?))
                    }
                }
            }
            "actions" => {
                let range: Range = field(&params, "range")?;
                let choices: Vec<Value> = request
                    .code_actions(&path, range, Capabilities::BROWSER, RowActions::Command)
                    .into_iter()
                    .map(|item| match item.command {
                        Some(command) => json!({"title":item.title,"command":command}),
                        // A blocked action is listed with why, and no edit.
                        None => match item.disabled {
                            Some(reason) => json!({
                                "title": item.title,
                                "kind": item.kind,
                                "disabled": {"reason": reason},
                            }),
                            None => json!({
                                "title": item.title,
                                "kind": item.kind,
                                "edit": self.session.edit([(path.clone(), item.edits)]),
                            }),
                        },
                    })
                    .collect();
                Ok(json!({"actions":choices,"versions":self.session.versions_json()}))
            }
            _ => Err(format!("Unknown browser request: {method}")),
        }
    }

    fn execute(&self, command: Command, now: DateTime<FixedOffset>) -> Result<Value, String> {
        let action = Action::decode(
            &command.command,
            command.arguments.as_deref().unwrap_or(&[]),
        )?;
        // The virtual workspace boundary belongs to the browser host.
        if let Some(document) = action.document() {
            virtual_path(document.as_str())?;
        }
        let request = self.session.request(now);
        match request.prepare(&action, Capabilities::BROWSER)? {
            PreparedAction::Edit { path, edits } => {
                Ok(json!({"edit":self.session.edit([(path, edits)])}))
            }
            PreparedAction::Open { url } => Ok(json!({"open":url})),
            _ => Err("This command is not available in the browser".into()),
        }
    }
}
