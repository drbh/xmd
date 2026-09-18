//! A browser-local workspace. JSON crosses the worker boundary; all language logic stays in Rust.
use crate::{
    actions,
    commands::{Action, Capabilities, PreparedAction},
    diagnostics,
    document::{Document, Span, identifier},
    intelligence, interaction, paths, presentation, refactor,
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset};
use lsp_types::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct BrowserWorkspace {
    workspace: Workspace,
    versions: BTreeMap<PathBuf, i32>,
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
            workspace: Workspace {
                roots: vec!["/workspace".into()],
                documents: BTreeMap::new(),
                cache: BTreeMap::new(),
                lookups: Default::default(),
            },
            versions: BTreeMap::new(),
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
    let path = paths::file_path(&url)?;
    if !path.starts_with("/workspace")
        || path.extension().is_none_or(|s| s != "wtf")
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || path.to_string_lossy().contains(['\0', '\\'])
    {
        return Err("Expected a .wtf file within the browser's /workspace".into());
    }
    Ok(path)
}
fn serialized(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

impl BrowserWorkspace {
    fn versions_json(&self) -> Value {
        self.versions
            .iter()
            .map(|(p, v)| (paths::file_url(p).unwrap().to_string(), json!(v)))
            .collect()
    }
    fn edit(&self, changes: BTreeMap<PathBuf, Vec<TextEdit>>) -> Value {
        json!({"documentChanges": changes.into_iter().map(|(p,edits)| json!({
            "textDocument":{"uri":paths::file_url(&p).unwrap(),"version":self.versions[&p]}, "edits":edits
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
        if method == "setDocument" {
            let path = virtual_path(&field::<String>(&params, "uri")?)?;
            let version: i32 = field(&params, "version")?;
            let text: String = field(&params, "text")?;
            if text.len() > 1_000_000 {
                return Err("Notes are limited to 1 MB in the browser".into());
            }
            if self.versions.get(&path).is_some_and(|old| version <= *old) {
                return Err("Stale document version".into());
            }
            self.workspace
                .documents
                .insert(path.clone(), Document::parse(text));
            self.versions.insert(path, version);
            return Ok(Value::Null);
        }
        if method == "removeDocument" {
            let path = virtual_path(&field::<String>(&params, "uri")?)?;
            self.workspace.documents.remove(&path);
            self.versions.remove(&path);
            return Ok(Value::Null);
        }
        if method == "execute" {
            if params["versions"] != self.versions_json() {
                return Err(
                    "Notes changed; request fresh controls before applying this action".into(),
                );
            }
            let command: Command = field(&params, "command")?;
            return self.execute(command, now);
        }
        if method == "query" {
            let compiled = crate::query::Query::parse(&field::<String>(&params, "query")?)?;
            let result = crate::query::execute(
                &self.workspace,
                &compiled,
                &crate::query::QueryContext::new(now),
            )?;
            return Ok(
                json!({"schemaVersion":1,"now":now.to_rfc3339(),"rows":result.rows,"versions":self.versions_json()}),
            );
        }
        let path = virtual_path(&field::<String>(&params, "uri")?)?;
        let doc = self
            .workspace
            .documents
            .get(&path)
            .ok_or("Note is not open in this browser workspace")?;
        let ws = &self.workspace;
        let request = crate::RequestContext::new(ws, now);
        let position = || field::<Position>(&params, "position");
        let location = |symbol: &crate::workspace::Symbol| Location {
            uri: paths::file_url(&symbol.path).unwrap(),
            range: ws
                .named(symbol)
                .span
                .range(&ws.documents[&symbol.path].text),
        };
        match method {
            "documentLinks" => serialized(presentation::document_links_in(&request, &path)),
            "documentSymbols" => serialized(crate::symbols::document_symbols_in(&request, &path)),
            "formatting" => serialized(crate::tables::formatting(doc)),
            "folding" => serialized(crate::symbols::folding_ranges(doc)),
            "onTypeFormatting" => serialized(crate::typing::on_type(
                doc,
                position()?,
                &field::<String>(&params, "ch")?,
            )),
            "analyze" => {
                let inlays = presentation::hints_in(
                    &request,
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
                let lenses = interaction::lenses_for(&request, &path, Capabilities::BROWSER);
                let links = presentation::document_links_in(&request, &path);
                Ok(
                    json!({"version":self.versions[&path],"versions":self.versions_json(),"hints":inlays.hints,"tokens":tokens,"tokenTypes":presentation::TOKEN_TYPES,
                    "diagnostics":diagnostics::collect_in(&request, &path, true),"lenses":lenses,"links":links,"live":inlays.time_dependent,
                    "symbols":crate::symbols::document_symbols_in(&request,&path)}),
                )
            }
            "completion" => serialized(intelligence::completions_in(
                &request,
                &path,
                position()?,
                true,
            )),
            "signature" => serialized(intelligence::signature(doc, position()?)),
            "hover" => {
                if let Some(hover) = intelligence::link_hover_in(&request, &path, position()?) {
                    return serialized(hover);
                }
                if let Some(hover) = intelligence::cell_hover_in(&request, &path, position()?) {
                    return serialized(hover);
                }
                if let Some(hover) =
                    intelligence::stop_hover(ws, &path, position()?, now.date_naive())
                {
                    return serialized(hover);
                }
                if intelligence::symbol_at(ws, &path, position()?).is_none()
                    && let Some(hover) =
                        intelligence::calculation_hover_in(&request, &path, position()?)
                {
                    return serialized(hover);
                }
                if let Some((symbol, span)) = intelligence::symbol_at(ws, &path, position()?) {
                    let mut value = intelligence::hover_in(&request, &symbol);
                    let mut range = span.range(&doc.text);
                    if let Some(reference) = doc
                        .references
                        .iter()
                        .find(|r| r.span == span && r.property.is_some())
                    {
                        let preview = request
                            .engine()
                            .eval(&path, &reference.expression())
                            .map(|v| v.display())
                            .unwrap_or_else(|e| e);
                        value = format!("{} = {preview}\n\n{value}", reference.expression());
                        range = Span::new(span.line, span.start, reference.end()).range(&doc.text);
                    }
                    return Ok(json!({"contents":{"kind":"markdown","value":value},"range":range}));
                }
                let row = position()?.line as usize;
                if let Some((i, task)) = doc.tasks.iter().enumerate().find(|(_, t)| t.line == row) {
                    let mut engine = request.engine();
                    let status = if engine.task_done(&path, i) {
                        "Complete"
                    } else {
                        "Incomplete"
                    };
                    let blockers = engine
                        .blocked(&path, i)
                        .map(|names| names.join(", "))
                        .unwrap_or_else(|e| e);
                    return Ok(
                        json!({"contents":{"kind":"markdown","value":format!("**{}**\n\n{status}\n\n{blockers}",task.title)},"range":Span::new(row,0,doc.line(row).len()).range(&doc.text)}),
                    );
                }
                Ok(Value::Null)
            }
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
                        uri: paths::file_url(&p).unwrap(),
                        range: span.range(&ws.documents[&p].text),
                    })
                    .collect();
                if method == "references" {
                    return serialized(locations);
                }
                if method == "highlights" {
                    return Ok(json!(
                        locations
                            .into_iter()
                            .enumerate()
                            .filter(|(_, l)| l.uri == paths::file_url(&path).unwrap())
                            .map(|(i, l)| json!({"range":l.range,"kind":if i==0 {3} else {2}}))
                            .collect::<Vec<_>>()
                    ));
                }
                let name: String = field(&params, "newName")?;
                if !identifier(&name) {
                    return Err("Use a name with letters, digits, and underscores".into());
                }
                crate::tables::validate_rename(ws, &symbol, &name)?;
                let mut changes = BTreeMap::<PathBuf, Vec<TextEdit>>::new();
                for location in locations {
                    changes
                        .entry(paths::file_path(&location.uri)?)
                        .or_default()
                        .push(TextEdit::new(location.range, name.clone()));
                }
                Ok(self.edit(changes))
            }
            "actions" => {
                let range: Range = field(&params, "range")?;
                let mut choices: Vec<Value> = refactor::actions_for_in(&request,&path,range).into_iter().map(|a|json!({"title":a.title,"kind":a.kind,"edit":self.single_edit(&path,a.edits)})).collect();
                for command in interaction::row_commands_for(
                    &request,
                    &path,
                    range.start.line as usize,
                    true,
                    Capabilities::BROWSER,
                ) {
                    choices.push(json!({"title":command.title,"command":command}));
                }
                let dates: Vec<_> = actions::freeze_dates_in(&request, &path)
                    .into_iter()
                    .filter(|e| {
                        e.range.start.line >= range.start.line && e.range.end.line <= range.end.line
                    })
                    .collect();
                if !dates.is_empty() {
                    choices.push(json!({"title":"Freeze relative date","kind":"refactor.rewrite","edit":self.single_edit(&path,dates)}));
                }
                Ok(json!({"actions":choices,"versions":self.versions_json()}))
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
        let request = crate::RequestContext::new(&self.workspace, now);
        match action.prepare(&request, Capabilities::BROWSER)? {
            PreparedAction::Edit { path, edits } => {
                Ok(json!({"edit":self.single_edit(&path, edits)}))
            }
            PreparedAction::Open { url } => Ok(json!({"open":url})),
            _ => Err("This command is not available in the browser".into()),
        }
    }
}
