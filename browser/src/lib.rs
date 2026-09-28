//! A browser-local workspace. JSON crosses the worker boundary; all language logic stays in Rust.

use chrono::{DateTime, FixedOffset};
use common::{file_path, uri};
use eval::Workspace;
use lsp_types::*;
use model::identifier;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use services::Request;
use services::commands::{Action, Capabilities, PreparedAction};
use services::session::WorkspaceSession;
use services::{actions::TaskToggle, intelligence, presentation};
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
            session: WorkspaceSession::browser(Workspace {
                roots: vec!["/workspace".into()],
                documents: BTreeMap::new(),
                cache: BTreeMap::new(),
                lookups: Default::default(),
                modules: Default::default(),
            }),
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
        || !common::is_note(&path)
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || path.to_string_lossy().contains(['\0', '\\'])
    {
        return Err(format!(
            "Expected a .{} file within the browser's /workspace",
            common::EXTENSION
        ));
    }
    Ok(path)
}
fn serialized(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

impl BrowserWorkspace {
    fn edit(&self, changes: BTreeMap<PathBuf, Vec<TextEdit>>) -> Value {
        json!({"documentChanges": changes.into_iter().map(|(p,edits)| json!({
            "textDocument":{"uri":uri(&p),"version":self.session.version(&p)}, "edits":edits
        })).collect::<Vec<_>>()})
    }
    fn single_edit(&self, path: &Path, edits: Vec<TextEdit>) -> Value {
        self.edit([(path.to_path_buf(), edits)].into())
    }
    fn dispatch(
        &mut self,
        method: &str,
        params: Value,
        now: DateTime<FixedOffset>,
    ) -> Result<Value, String> {
        if method == "semanticLegend" {
            return Ok(
                json!({"tokenTypes": presentation::TOKEN_TYPES, "tokenModifiers": presentation::TOKEN_MODIFIERS}),
            );
        }
        if method == "setResourceData" {
            let target: String = field(&params, "url")?;
            let data: Value = field(&params, "data")?;
            let metadata = self
                .session
                .workspace
                .link_features()
                .decode_refresh(&target, &data, now.to_utc())
                .map_err(|e| e.to_string())?;
            self.session.workspace.cache.insert(target, metadata);
            return Ok(Value::Null);
        }
        if method == "setModules" {
            let sources: BTreeMap<String, String> = field(&params, "sources")?;
            let sources = sources
                .into_iter()
                .map(|(name, source)| {
                    if Path::new(&name).components().count() != 1
                        || !matches!(
                            Path::new(&name).components().next(),
                            Some(Component::Normal(_))
                        )
                        || !common::is_note(&name)
                        || name.contains(['\\', '\0'])
                    {
                        return Err(format!(
                            "Module names must be .{} filenames",
                            common::EXTENSION
                        ));
                    }
                    Ok((Path::new("/workspace/.xmd/modules").join(name), source))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let modules = eval::modules::ModuleRegistry::compile(sources)?;
            self.session.workspace.modules = std::sync::Arc::new(modules);
            return Ok(Value::Null);
        }
        if method == "setDocument" {
            let path = virtual_path(&field::<String>(&params, "uri")?)?;
            let version: i32 = field(&params, "version")?;
            let text: String = field(&params, "text")?;
            if text.len() > 1_000_000 {
                return Err("Notes are limited to 1 MB in the browser".into());
            }
            self.session.open(&path, version, text)?;
            return Ok(Value::Null);
        }
        if method == "removeDocument" {
            let path = virtual_path(&field::<String>(&params, "uri")?)?;
            self.session.workspace.documents.remove(&path);
            self.session.close(&path);
            return Ok(Value::Null);
        }
        if method == "execute" {
            if params["versions"] != self.session.versions_json() {
                return Err(
                    "Notes changed; request fresh controls before applying this action".into(),
                );
            }
            let command: Command = field(&params, "command")?;
            return self.execute(command, now);
        }
        if method == "query" {
            let compiled = services::query::Query::parse(&field::<String>(&params, "query")?)?;
            let only = params
                .get("uri")
                .filter(|v| !v.is_null())
                .map(|_| field::<String>(&params, "uri").and_then(|uri| virtual_path(&uri)))
                .transpose()?;
            let result =
                Request::new(&self.session.workspace, now).query(&compiled, only.as_deref())?;
            return Ok(
                json!({"schemaVersion":1,"now":now.to_rfc3339(),"rows":result.json(),"versions":self.session.versions_json()}),
            );
        }
        let path = virtual_path(&field::<String>(&params, "uri")?)?;
        let doc = self
            .session
            .workspace
            .documents
            .get(&path)
            .ok_or("Note is not open in this browser workspace")?;
        let ws = &self.session.workspace;
        let request = Request::new(ws, now);
        let position = || field::<Position>(&params, "position");
        let location = |symbol: &eval::Symbol| Location {
            uri: common::uri_from_url(&uri(&symbol.path)),
            range: ws
                .named(symbol)
                .span
                .range(&ws.documents[&symbol.path].text),
        };
        match method {
            "documentLinks" => serialized(request.document_links(&path)),
            "documentSymbols" => serialized(request.document_symbols(&path)),
            "formatting" => serialized(request.formatting(&path)?),
            "folding" => serialized(services::symbols::folding_ranges(doc)),
            "onTypeFormatting" => serialized(services::typing::on_type(
                doc,
                position()?,
                &field::<String>(&params, "ch")?,
            )),
            "analyze" | "render" => {
                let inlays = request.hints(
                    &path,
                    Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
                );
                let tokens: Vec<u32> = presentation::semantic_tokens(doc)
                    .into_iter()
                    .flat_map(|t| {
                        [
                            t.delta_line,
                            t.delta_start,
                            t.length,
                            t.token_type,
                            t.token_modifiers_bitset,
                        ]
                    })
                    .collect();
                let lenses = request.code_lenses(&path, Capabilities::BROWSER);
                let links = request.document_links(&path);
                let editing = params
                    .get("editing")
                    .and_then(Value::as_bool)
                    .unwrap_or(method == "analyze");
                let diagnostics = request.diagnostics(&path, editing);
                let html = services::rendering::fragment(doc, &inlays.hints, &diagnostics, &links)?;
                Ok(
                    json!({"schemaVersion":1,"engineVersion":env!("CARGO_PKG_VERSION"),"uri":field::<String>(&params,"uri")?,"source":doc.text,"now":now.to_rfc3339(),"editing":editing,"html":html,"lineClasses":services::rendering::line_classes(doc),"tokenModifiers":presentation::TOKEN_MODIFIERS,"version":self.session.version(&path),"versions":self.session.versions_json(),"hints":inlays.hints,"tokens":tokens,"tokenTypes":presentation::TOKEN_TYPES,
                    "diagnostics":diagnostics,"lenses":lenses,"links":links,"live":inlays.time_dependent,
                    "symbols":request.document_symbols(&path)}),
                )
            }
            "completion" => serialized(request.completions(&path, position()?, true)),
            "signature" => serialized(intelligence::signature(doc, &path, position()?)),
            "hover" => match request.hover(&path, position()?) {
                Some(hover) => serialized(hover),
                None => Ok(Value::Null),
            },
            "definition" | "references" | "highlights" | "prepareRename" | "rename" => {
                let Some((symbol, span)) = intelligence::symbol_at(ws, &path, position()?) else {
                    return Ok(Value::Null);
                };
                if method == "definition" {
                    return serialized(location(&symbol));
                }
                if method == "prepareRename" {
                    return Ok(
                        json!({"range":span.range(&doc.text),"placeholder":ws.named(&symbol).name}),
                    );
                }
                let locations: Vec<Location> = intelligence::occurrences(ws, &symbol)
                    .into_iter()
                    .map(|(p, span)| Location {
                        uri: common::uri_from_url(&uri(&p)),
                        range: span.range(&ws.documents[&p].text),
                    })
                    .collect();
                if method == "references" {
                    return serialized(locations);
                }
                if method == "highlights" {
                    let current = common::uri_from_url(&uri(&path));
                    return Ok(json!(
                        locations
                            .into_iter()
                            .enumerate()
                            .filter(|(_, l)| l.uri == current)
                            .map(|(i, l)| json!({"range":l.range,"kind":if i==0 {3} else {2}}))
                            .collect::<Vec<_>>()
                    ));
                }
                let name: String = field(&params, "newName")?;
                if !identifier(&name) {
                    return Err("Use a name with letters, digits, and underscores".into());
                }
                eval::tables::validate_rename(ws, &symbol, &name)?;
                let mut changes = BTreeMap::<PathBuf, Vec<TextEdit>>::new();
                for location in locations {
                    changes
                        .entry(file_path(&common::url_from_uri(&location.uri))?)
                        .or_default()
                        .push(TextEdit::new(location.range, name.clone()));
                }
                Ok(self.edit(changes))
            }
            "actions" => {
                let range: Range = field(&params, "range")?;
                let choices: Vec<Value> = request
                    .code_actions(&path, range, Capabilities::BROWSER, TaskToggle::Command)
                    .into_iter()
                    .map(|item| match item.command {
                        Some(command) => json!({"title":item.title,"command":command}),
                        None => json!({
                            "title": item.title,
                            "kind": item.kind,
                            "edit": self.single_edit(&path, item.edits),
                        }),
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
        let request = Request::new(&self.session.workspace, now);
        match action.prepare(&request, Capabilities::BROWSER)? {
            PreparedAction::Edit { path, edits } => {
                Ok(json!({"edit":self.single_edit(&path, edits)}))
            }
            PreparedAction::Open { url } => Ok(json!({"open":url})),
            _ => Err("This command is not available in the browser".into()),
        }
    }
}
