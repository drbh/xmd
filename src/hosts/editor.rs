use crate::commands::{Action, Capabilities, PreparedAction};
use crate::{
    actions,
    document::{Document, Span, identifier},
    engine::Value,
    workspace::{SymbolKind, Workspace},
};
use chrono::Local;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use tower_lsp::{
    Client, LanguageServer, LspService, Server,
    jsonrpc::{Error, Result},
    lsp_types::*,
};

use crate::presentation::{TOKEN_MODIFIERS, TOKEN_TYPES};
pub use crate::presentation::{hints, hints_at, problems, semantic_tokens};

struct State {
    workspace: Workspace,
    open: BTreeMap<PathBuf, i32>,
    hint_refresh: bool,
    token_refresh: bool,
    watch: bool,
    live: BTreeSet<PathBuf>,
    lens_refresh: bool,
    snippets: bool,
    hierarchical_symbols: bool,
    diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    lenses: BTreeMap<PathBuf, Vec<CodeLens>>,
}
#[derive(Clone)]
pub struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
}
#[derive(serde::Deserialize)]
struct QueryParams {
    query: String,
    now: Option<chrono::DateTime<chrono::FixedOffset>>,
}
impl Backend {
    async fn query(&self, params: QueryParams) -> Result<serde_json::Value> {
        let compiled = crate::query::Query::parse(&params.query).map_err(Error::invalid_params)?;
        self.rescan().await;
        let now = params.now.unwrap_or_else(|| Local::now().fixed_offset());
        let state = self.state.read().await;
        let result = crate::query::execute(
            &state.workspace,
            &compiled,
            &crate::query::QueryContext::new(now),
        )
        .map_err(Error::invalid_params)?;
        let versions = state
            .open
            .iter()
            .filter_map(|(path, version)| {
                crate::paths::file_url(path)
                    .ok()
                    .map(|uri| (uri.to_string(), serde_json::json!(version)))
            })
            .collect::<serde_json::Map<_, _>>();
        Ok(
            serde_json::json!({"schemaVersion":1,"now":now.to_rfc3339(),"rows":result.rows,"versions":versions}),
        )
    }
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State {
                workspace: Workspace {
                    roots: Vec::new(),
                    documents: BTreeMap::new(),
                    cache: BTreeMap::new(),
                    lookups: BTreeMap::new(),
                    plugins: Default::default(),
                },
                open: BTreeMap::new(),
                hint_refresh: false,
                token_refresh: false,
                watch: false,
                live: BTreeSet::new(),
                lens_refresh: false,
                snippets: false,
                hierarchical_symbols: false,
                diagnostics: BTreeMap::new(),
                lenses: BTreeMap::new(),
            })),
        }
    }
    async fn notify_changes(&self) {
        let (diagnostics, hint_refresh, token_refresh, lens_refresh) = {
            let mut state = self.state.write().await;
            let now = Local::now().fixed_offset();
            let paths: Vec<_> = state
                .open
                .keys()
                .filter(|p| state.workspace.documents.contains_key(*p))
                .cloned()
                .collect();
            let (live, updates) = {
                let request = crate::RequestContext::new(&state.workspace, now);
                let live = paths
                    .iter()
                    .filter(|p| crate::presentation::live_hints_in(&request, p))
                    .cloned()
                    .collect();
                let updates: Vec<_> = paths
                    .into_iter()
                    .map(|path| {
                        let ds = crate::diagnostics::collect_in(&request, &path, true);
                        let lenses = crate::interaction::lenses_in(&request, &path);
                        (path, ds, lenses)
                    })
                    .collect();
                (live, updates)
            };
            state.live = live;
            state.diagnostics.clear();
            state.lenses.clear();
            let mut diagnostics = Vec::new();
            for (path, ds, lenses) in updates {
                diagnostics.push((
                    Url::from_file_path(&path).unwrap(),
                    state.open[&path],
                    ds.clone(),
                ));
                state.diagnostics.insert(path.clone(), ds);
                state.lenses.insert(path, lenses);
            }
            (
                diagnostics,
                state.hint_refresh,
                state.token_refresh,
                state.lens_refresh,
            )
        };
        for (uri, version, diagnostics) in diagnostics {
            self.client
                .publish_diagnostics(uri, diagnostics, Some(version))
                .await;
        }
        if hint_refresh {
            let _ = self.client.inlay_hint_refresh().await;
        }
        if token_refresh {
            let _ = self.client.semantic_tokens_refresh().await;
        }
        if lens_refresh {
            let _ = self.client.code_lens_refresh().await;
        }
    }
    async fn tick(&self) {
        let (refresh, lens_refresh, diagnostics) = {
            let mut state = self.state.write().await;
            if state.live.is_empty() {
                return;
            }
            let now = Local::now().fixed_offset();
            let paths: Vec<_> = state
                .live
                .iter()
                .filter(|p| {
                    state.open.contains_key(*p) && state.workspace.documents.contains_key(*p)
                })
                .cloned()
                .collect();
            let (live, updates) = {
                let request = crate::RequestContext::new(&state.workspace, now);
                let updates: Vec<_> = paths
                    .iter()
                    .map(|path| {
                        let ds = crate::diagnostics::collect_in(&request, path, true);
                        let lenses = crate::interaction::lenses_in(&request, path);
                        (path.clone(), ds, lenses)
                    })
                    .collect();
                let live = paths
                    .into_iter()
                    .filter(|p| crate::presentation::live_hints_in(&request, p))
                    .collect();
                (live, updates)
            };
            let mut diagnostics = Vec::new();
            let mut lens_changed = false;
            for (path, ds, lenses) in updates {
                if state.diagnostics.get(&path) != Some(&ds) {
                    diagnostics.push((
                        Url::from_file_path(&path).unwrap(),
                        state.open[&path],
                        ds.clone(),
                    ));
                    state.diagnostics.insert(path.clone(), ds);
                }
                if state.lenses.get(&path) != Some(&lenses) {
                    state.lenses.insert(path, lenses);
                    lens_changed = true;
                }
            }
            state.live = live;
            (
                state.hint_refresh,
                state.lens_refresh && lens_changed,
                diagnostics,
            )
        };
        for (uri, version, ds) in diagnostics {
            self.client
                .publish_diagnostics(uri, ds, Some(version))
                .await;
        }
        if refresh {
            let _ = self.client.inlay_hint_refresh().await;
        }
        if lens_refresh {
            let _ = self.client.code_lens_refresh().await;
        }
    }
    async fn update(&self, uri: Url, version: i32, text: String) {
        if let Ok(path) = uri.to_file_path() {
            let mut state = self.state.write().await;
            if state.open.get(&path).is_some_and(|v| *v > version) {
                return;
            }
            if crate::plugins::is_plugin_path(&path) {
                // Plugin files reload from disk on save or watched-file events.
                return;
            }
            state
                .workspace
                .documents
                .insert(path.clone(), Document::parse(text));
            state.open.insert(path, version);
        }
        self.notify_changes().await;
    }
    async fn rescan(&self) {
        let (roots, plugins) = {
            let state = self.state.read().await;
            (
                state.workspace.roots.clone(),
                state.workspace.plugins.clone(),
            )
        };
        let result = tokio::task::spawn_blocking(move || {
            let mut workspace = Workspace::load_notes(roots)?;
            workspace.plugins = plugins;
            let error = workspace.reload_plugins().err();
            Ok::<_, String>((workspace, error))
        })
        .await;
        match result {
            Ok(Ok((mut workspace, error))) => {
                if let Some(error) = error {
                    self.client.log_message(MessageType::ERROR, error).await;
                }
                let mut state = self.state.write().await;
                for path in state.open.keys() {
                    if let Some(doc) = state.workspace.documents.get(path) {
                        workspace.documents.insert(path.clone(), doc.clone());
                    }
                }
                state.workspace = workspace;
            }
            Ok(Err(e)) => self.client.log_message(MessageType::ERROR, e).await,
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, e.to_string())
                    .await
            }
        }
    }
}
fn file(uri: &Url) -> Result<PathBuf> {
    uri.to_file_path()
        .map_err(|_| Error::invalid_params("WTF needs a local file URI"))
}
use crate::intelligence::symbol_at;
fn edit_for(state: &State, path: &Path, edits: Vec<TextEdit>) -> WorkspaceEdit {
    versioned_edit(path, edits, state.open.get(path).copied())
}
fn versioned_edit(path: &Path, edits: Vec<TextEdit>, version: Option<i32>) -> WorkspaceEdit {
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: Url::from_file_path(path).unwrap(),
                version,
            },
            edits: edits.into_iter().map(OneOf::Left).collect(),
        }])),
        ..Default::default()
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let mut roots: Vec<_> = params
            .workspace_folders
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f.uri.to_file_path().ok())
            .collect();
        if roots.is_empty()
            && let Some(root) = params.root_uri.and_then(|u| u.to_file_path().ok())
        {
            roots.push(root);
        }
        if roots.is_empty()
            && let Ok(root) = std::env::current_dir()
        {
            roots.push(root);
        }
        {
            let mut state = self.state.write().await;
            state.workspace.roots = roots;
            state.hierarchical_symbols = params
                .capabilities
                .text_document
                .as_ref()
                .and_then(|t| t.document_symbol.as_ref())
                .is_some_and(|s| s.hierarchical_document_symbol_support == Some(true));
            state.snippets = params
                .capabilities
                .text_document
                .as_ref()
                .and_then(|t| t.completion.as_ref())
                .and_then(|c| c.completion_item.as_ref())
                .is_some_and(|c| c.snippet_support == Some(true));
            if let Some(w) = params.capabilities.workspace {
                state.lens_refresh = w.code_lens.is_some_and(|c| c.refresh_support == Some(true));
                state.hint_refresh = w
                    .inlay_hint
                    .is_some_and(|c| c.refresh_support == Some(true));
                state.token_refresh = w
                    .semantic_tokens
                    .is_some_and(|c| c.refresh_support == Some(true));
                state.watch = w
                    .did_change_watched_files
                    .is_some_and(|c| c.dynamic_registration == Some(true));
            }
        }
        self.rescan().await;
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "WTF".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                experimental: Some(
                    serde_json::json!({"wtfQuery":{"method":"wtf/query","schemaVersion":1}}),
                ),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                inlay_hint_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                document_highlight_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: Some(vec![")".into()]),
                    ..Default::default()
                }),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["[".into(), "@".into(), ".".into()]),
                    ..Default::default()
                }),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: Action::COMMANDS.iter().map(|s| (*s).into()).collect(),
                    ..Default::default()
                }),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: crate::typing::TRIGGERS[0].into(),
                    more_trigger_character: Some(
                        crate::typing::TRIGGERS[1..]
                            .iter()
                            .map(|c| c.to_string())
                            .collect(),
                    ),
                }),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        legend: SemanticTokensLegend {
                            token_types: TOKEN_TYPES
                                .iter()
                                .map(|t| SemanticTokenType::new(t))
                                .collect(),
                            token_modifiers: TOKEN_MODIFIERS
                                .iter()
                                .map(|m| SemanticTokenModifier::new(m))
                                .collect(),
                        },
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                        ..Default::default()
                    }
                    .into(),
                ),
                ..Default::default()
            },
        })
    }
    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "WTF: tasks, timers, resources, dates, and workspace navigation ready",
            )
            .await;
        if self.state.read().await.watch {
            let _=self.client.register_capability(vec![Registration{id:"wtf-notes".into(),method:"workspace/didChangeWatchedFiles".into(),register_options:Some(serde_json::json!({"watchers":[{"globPattern":"**/*.wtf"},{"globPattern":"**/.wtf/cache.json"},{"globPattern":"**/.wtf/plugins/*.wtf"}]}))}]).await;
        }
        let backend = self.clone();
        tokio::spawn(async move {
            let mut today = Local::now().date_naive();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let next = Local::now().date_naive();
                if next != today {
                    today = next;
                    backend.notify_changes().await;
                } else {
                    backend.tick().await;
                }
            }
        });
    }
    async fn shutdown(&self) -> Result<()> {
        self.state.write().await.live.clear();
        Ok(())
    }
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update(
            params.text_document.uri,
            params.text_document.version,
            params.text_document.text,
        )
        .await;
    }
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last()
            && change.range.is_none()
        {
            self.update(
                params.text_document.uri,
                params.text_document.version,
                change.text,
            )
            .await;
        }
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if let Ok(path) = params.text_document.uri.to_file_path() {
            self.state.write().await.open.remove(&path);
            self.rescan().await;
            self.client
                .publish_diagnostics(params.text_document.uri, vec![], None)
                .await;
            self.notify_changes().await;
        }
    }
    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.rescan().await;
        self.notify_changes().await;
    }
    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        self.rescan().await;
        self.notify_changes().await;
    }
    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state.workspace.documents.contains_key(&path).then(|| {
            hints_at(
                &state.workspace,
                &path,
                Local::now().fixed_offset(),
                params.range,
            )
        }))
    }
    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state.workspace.documents.get(&path).map(|doc| {
            SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: semantic_tokens(doc),
            })
        }))
    }
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some(doc) = ws.documents.get(&path) else {
            return Ok(None);
        };
        let now = Local::now().fixed_offset();
        let request = crate::RequestContext::new(ws, now);
        if let Some(hover) = crate::features::module_features::hover(&request, &path, at.position) {
            return Ok(Some(hover));
        }
        if let Some(hover) = crate::intelligence::link_hover_in(&request, &path, at.position) {
            return Ok(Some(hover));
        }
        if let Some(hover) = crate::intelligence::cell_hover_in(&request, &path, at.position) {
            return Ok(Some(hover));
        }

        if symbol_at(ws, &path, at.position).is_none()
            && let Some(hover) =
                crate::intelligence::calculation_hover_in(&request, &path, at.position)
        {
            return Ok(Some(hover));
        }
        if let Some((symbol, span)) = symbol_at(ws, &path, at.position) {
            let mut value = crate::intelligence::hover_in(&request, &symbol);
            let mut range = span.range(&doc.text);
            if let Some(reference) = doc
                .references
                .iter()
                .find(|r| r.span == span && r.property.is_some())
            {
                let property = request
                    .engine()
                    .eval(&path, &reference.expression())
                    .map(|v| v.display())
                    .unwrap_or_else(|e| e);
                value = format!("{} = {}\n\n{}", reference.expression(), property, value);
                range = Span::new(span.line, span.start, reference.end()).range(&doc.text);
            }
            return Ok(Some(Hover {
                contents: HoverContents::Markup(crate::intelligence::markup(value)),
                range: Some(range),
            }));
        }
        if let Some((index, task)) = doc
            .tasks
            .iter()
            .enumerate()
            .find(|(_, t)| t.line == at.position.line as usize)
        {
            let mut engine = request.engine();
            let blocked = engine.blocked(&path, index);
            let mut value = format!(
                "**{}**\n\n{}",
                task.title,
                if engine.task_done(&path, index) {
                    "Complete"
                } else {
                    "Incomplete"
                }
            );
            match blocked {
                Ok(names) if !names.is_empty() => {
                    let names = names
                        .into_iter()
                        .map(|name| {
                            ws.resolve(&path, &name)
                                .map(|s| crate::intelligence::source_link(ws, &s))
                                .unwrap_or(name)
                        })
                        .collect::<Vec<_>>();
                    value.push_str(&format!("\n\nBlocked by: {}", names.join(", ")));
                }
                Err(e) => value.push_str(&format!("\n\n{e}")),
                _ => {}
            }
            if let Some(attr) = task.attributes.get("estimate")
                && let Ok(v) = engine.eval(&path, &attr.value)
            {
                value.push_str(&format!("\n\nEstimate: {}", v.display()));
            }
            if let Some(attr) = task.attributes.get("timer")
                && let Ok(v) = engine.eval(&path, &attr.value)
            {
                value.push_str(&format!("\n\nTimer: {}", v.display()));
                if let Value::Timer(timer) = &v
                    && let Some(limit) = timer.limit
                {
                    value.push_str(&format!(
                        "\n\n`{}`",
                        crate::charts::bar_fraction(timer.elapsed as f64 / limit as f64)
                    ));
                }
            }
            let children: Vec<_> = doc
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.parent == Some(index))
                .map(|(j, _)| j)
                .collect();
            if !children.is_empty() {
                let done = children
                    .iter()
                    .filter(|j| engine.task_done(&path, **j))
                    .count();
                value.push_str(&format!(
                    "\n\nSubtasks: `{}` {done}/{}",
                    crate::charts::bar(done, children.len()),
                    children.len()
                ));
            }
            value.push_str("\n\nUse code actions or the clickable labels to complete/reopen tasks or control timers.");
            return Ok(Some(Hover {
                contents: HoverContents::Markup(crate::intelligence::markup(value)),
                range: Some(Span::new(task.line, 0, doc.line(task.line).len()).range(&doc.text)),
            }));
        }
        Ok(None)
    }
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        Ok(
            symbol_at(&state.workspace, &path, at.position).map(|(s, _)| {
                GotoDefinitionResponse::Scalar(Location {
                    uri: Url::from_file_path(&s.path).unwrap(),
                    range: state
                        .workspace
                        .named(&s)
                        .span
                        .range(&state.workspace.documents[&s.path].text),
                })
            }),
        )
    }
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let at = params.text_document_position;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some((symbol, _)) = symbol_at(ws, &path, at.position) else {
            return Ok(None);
        };
        let result = crate::intelligence::occurrences(ws, &symbol)
            .into_iter()
            .skip(usize::from(!params.context.include_declaration))
            .map(|(p, span)| Location {
                uri: Url::from_file_path(&p).unwrap(),
                range: span.range(&ws.documents[&p].text),
            })
            .collect();
        Ok(Some(result))
    }
    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        if !identifier(&params.new_name) {
            return Err(Error::invalid_params(
                "Names use letters, digits, and underscores and cannot start with a digit",
            ));
        }
        let at = params.text_document_position;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some((symbol, _)) = symbol_at(ws, &path, at.position) else {
            return Ok(None);
        };
        crate::tables::validate_rename(ws, &symbol, &params.new_name)
            .map_err(Error::invalid_params)?;
        let mut changes: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
        for (p, span) in crate::intelligence::occurrences(ws, &symbol) {
            changes.entry(p.clone()).or_default().push(TextEdit::new(
                span.range(&ws.documents[&p].text),
                params.new_name.clone(),
            ));
        }
        let edits = changes
            .into_iter()
            .map(|(p, edits)| TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: Url::from_file_path(&p).unwrap(),
                    version: state.open.get(&p).copied(),
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            })
            .collect();
        Ok(Some(WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(edits)),
            ..Default::default()
        }))
    }
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let at = params.text_document_position;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        Ok(Some(CompletionResponse::Array(
            crate::intelligence::completions(
                &state.workspace,
                &path,
                at.position,
                Local::now().fixed_offset(),
                state.snippets,
            ),
        )))
    }
    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state
            .workspace
            .documents
            .get(&path)
            .and_then(|doc| crate::intelligence::signature(doc, at.position)))
    }
    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(Some(crate::interaction::lenses(
            &state.workspace,
            &path,
            Local::now().fixed_offset(),
        )))
    }
    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some((symbol, _)) = symbol_at(ws, &path, at.position) else {
            return Ok(None);
        };
        let doc = &ws.documents[&path];
        let result = crate::intelligence::occurrences(ws, &symbol)
            .into_iter()
            .enumerate()
            .filter(|(_, (p, _))| *p == path)
            .map(|(i, (_, span))| DocumentHighlight {
                range: span.range(&doc.text),
                kind: Some(if i == 0 {
                    DocumentHighlightKind::WRITE
                } else {
                    DocumentHighlightKind::READ
                }),
            })
            .collect();
        Ok(Some(result))
    }
    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(
            symbol_at(&state.workspace, &path, params.position).map(|(s, span)| {
                PrepareRenameResponse::RangeWithPlaceholder {
                    range: span.range(&state.workspace.documents[&path].text),
                    placeholder: state.workspace.named(&s).name.clone(),
                }
            }),
        )
    }
    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        if !ws.documents.contains_key(&path) {
            return Ok(None);
        }
        Ok(Some(crate::presentation::document_links(
            ws,
            &path,
            Local::now().fixed_offset(),
        )))
    }
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some(doc) = ws.documents.get(&path) else {
            return Ok(None);
        };
        let now = Local::now().fixed_offset();
        let request = crate::RequestContext::new(ws, now);
        let row = params.range.start.line as usize;
        let mut result: Vec<_> = crate::interaction::row_commands_in(&request, &path, row, false)
            .into_iter()
            .map(CodeActionOrCommand::Command)
            .collect();
        if let Some((i, task)) = doc.tasks.iter().enumerate().find(|(_, t)| t.line == row) {
            let title = if task.attributes.contains_key("every") {
                "Complete occurrence and schedule next"
            } else if request.engine().task_done(&path, i) {
                "Reopen task"
            } else {
                "Complete task"
            };
            let mut action = CodeAction {
                title: title.into(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                ..Default::default()
            };
            match actions::toggle_task_in(&request, &path, i) {
                Ok(edits) => action.edit = Some(edit_for(&state, &path, edits)),
                Err(reason) => action.disabled = Some(CodeActionDisabled { reason }),
            }
            result.push(CodeActionOrCommand::CodeAction(action));
        }
        for action in crate::refactor::actions_for_in(&request, &path, params.range) {
            result.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: action.title,
                kind: Some(action.kind),
                edit: Some(edit_for(&state, &path, action.edits)),
                ..Default::default()
            }));
        }
        let edits = actions::freeze_dates_in(&request, &path)
            .into_iter()
            .filter(|e| {
                e.range.start.line >= params.range.start.line
                    && e.range.start.line <= params.range.end.line
            })
            .collect::<Vec<_>>();
        if !edits.is_empty() {
            result.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Freeze relative date".into(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit_for(&state, &path, edits)),
                ..Default::default()
            }));
        }
        if row == 0 && doc.line(0).trim_start().starts_with('#') {
            result.push(CodeActionOrCommand::Command(
                Action::ShowToday.command("Show today's agenda"),
            ));
        }
        if let Some(only) = params.context.only {
            result.retain(|a| match a {
                CodeActionOrCommand::CodeAction(a) => a.kind.as_ref().is_some_and(|kind| {
                    only.iter().any(|k| {
                        kind.as_str() == k.as_str()
                            || kind.as_str().starts_with(&format!("{}.", k.as_str()))
                    })
                }),
                _ => false,
            });
        }
        Ok(Some(result))
    }
    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        let action =
            Action::decode(&params.command, &params.arguments).map_err(Error::invalid_params)?;
        // Reload closed notes while preserving open buffers before validation.
        self.rescan().await;
        let (prepared, version) = {
            let state = self.state.read().await;
            let request = crate::RequestContext::new(&state.workspace, Local::now().fixed_offset());
            let prepared = action
                .prepare(&request, Capabilities::NATIVE)
                .map_err(Error::invalid_params)?;
            let version = match &prepared {
                PreparedAction::Edit { path, .. } => state.open.get(path).copied(),
                _ => None,
            };
            (prepared, version)
        };
        match prepared {
            PreparedAction::Edit { path, edits } => {
                let edit = versioned_edit(&path, edits, version);
                let applied = self.client.apply_edit(edit).await?;
                if !applied.applied {
                    return Err(Error::invalid_params(
                        applied
                            .failure_reason
                            .unwrap_or_else(|| "Editor declined the edit".into()),
                    ));
                }
                // didChange/didSave reports the applied edit and updates refreshes.
            }
            PreparedAction::Open { url } => {
                let opened = self
                    .client
                    .show_document(ShowDocumentParams {
                        external: Some(url.scheme() != "file"),
                        uri: url,
                        take_focus: Some(true),
                        selection: None,
                    })
                    .await?;
                if !opened {
                    return Err(Error::invalid_params("Editor could not open this resource"));
                }
            }
            PreparedAction::RefreshResource { resource } => {
                let snapshot = self.state.read().await.workspace.clone();
                let metadata = snapshot
                    .link_features()
                    .fetch(&resource.target)
                    .await
                    .map_err(Error::invalid_params)?;
                let workspace = {
                    let mut state = self.state.write().await;
                    if !Arc::ptr_eq(&snapshot.plugins, &state.workspace.plugins) {
                        return Err(Error::invalid_params(
                            "Plugins changed during refresh; refresh again",
                        ));
                    }
                    state.workspace.cache.insert(resource.target, metadata);
                    state.workspace.clone()
                };
                tokio::task::spawn_blocking(move || workspace.save_cache())
                    .await
                    .map_err(|e| Error::invalid_params(e.to_string()))?
                    .map_err(Error::invalid_params)?;
                self.notify_changes().await;
            }
            PreparedAction::Refresh => {
                let mut workspace = self.state.read().await.workspace.clone();
                let mut errors = crate::cli::refresh_in_memory(&mut workspace).await;
                let workspace = {
                    let mut state = self.state.write().await;
                    if !Arc::ptr_eq(&workspace.plugins, &state.workspace.plugins) {
                        return Err(Error::invalid_params(
                            "Plugins changed during refresh; refresh again",
                        ));
                    }
                    state.workspace.cache = workspace.cache;
                    state.workspace.lookups = workspace.lookups;
                    state.workspace.clone()
                };
                match tokio::task::spawn_blocking(move || workspace.save_cache()).await {
                    Ok(Ok(())) => (),
                    Ok(Err(e)) => errors.push(e),
                    Err(e) => errors.push(e.to_string()),
                }
                self.notify_changes().await;
                self.client
                    .show_message(
                        if errors.is_empty() {
                            MessageType::INFO
                        } else {
                            MessageType::WARNING
                        },
                        if errors.is_empty() {
                            "Resource status and lookups refreshed".into()
                        } else {
                            errors.join("\n")
                        },
                    )
                    .await;
            }
            PreparedAction::ShowToday => {
                self.rescan().await;
                let now = Local::now().fixed_offset();
                let today = now.date_naive();
                let workspace = self.state.read().await.workspace.clone();
                let compiled =
                    crate::query::Query::parse("@today").map_err(Error::invalid_params)?;
                let result = crate::query::execute(
                    &workspace,
                    &compiled,
                    &crate::query::QueryContext::new(now),
                )
                .map_err(Error::invalid_params)?;
                let lines: Vec<_> = result
                    .rows
                    .iter()
                    .map(|row| {
                        let e = row.json();
                        let path = Path::new(e["source"]["path"].as_str().unwrap_or(""));
                        let blocked = e["blocked_by"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>();
                        format!(
                            "- [{}:{}](<{}>) — {}{}",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            e["source"]["line"],
                            e["source"]["uri"].as_str().unwrap_or(""),
                            e["title"].as_str().unwrap_or(""),
                            if blocked.is_empty() {
                                String::new()
                            } else {
                                format!(" (blocked by {})", blocked.join(", "))
                            }
                        )
                    })
                    .collect();
                let content = format!(
                    "# Today — {today}\n\nGenerated view. Follow a link to edit the original note.\n\n{}\n",
                    if lines.is_empty() {
                        "Nothing scheduled.".into()
                    } else {
                        lines.join("\n")
                    }
                );
                let dir = workspace.root().join(".wtf");
                tokio::task::spawn_blocking({
                    let dir = dir.clone();
                    move || {
                        std::fs::create_dir_all(&dir)?;
                        std::fs::write(dir.join("today.md"), content)
                    }
                })
                .await
                .map_err(|e| Error::invalid_params(e.to_string()))?
                .map_err(|e| Error::invalid_params(e.to_string()))?;
                let _ = self
                    .client
                    .show_document(ShowDocumentParams {
                        uri: Url::from_file_path(dir.join("today.md")).unwrap(),
                        external: Some(false),
                        take_focus: Some(true),
                        selection: None,
                    })
                    .await;
            }
        }
        Ok(None)
    }
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let state = self.state.read().await;
        let ws = &state.workspace;
        #[allow(deprecated)]
        let result = ws
            .symbols()
            .into_iter()
            .filter(|s| {
                ws.named(s)
                    .name
                    .to_lowercase()
                    .contains(&params.query.to_lowercase())
            })
            .map(|s| SymbolInformation {
                name: ws.named(&s).name.clone(),
                kind: match s.kind {
                    SymbolKind::Task(_) => tower_lsp::lsp_types::SymbolKind::BOOLEAN,
                    SymbolKind::Section(_) => tower_lsp::lsp_types::SymbolKind::ARRAY,
                    _ => tower_lsp::lsp_types::SymbolKind::VARIABLE,
                },
                tags: None,
                deprecated: None,
                location: Location {
                    uri: Url::from_file_path(&s.path).unwrap(),
                    range: ws.named(&s).span.range(&ws.documents[&s.path].text),
                },
                container_name: Some(s.path.display().to_string()),
            })
            .collect();
        Ok(Some(result))
    }
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        if !ws.documents.contains_key(&path) {
            return Ok(None);
        }
        let symbols = crate::symbols::document_symbols(ws, &path, Local::now().fixed_offset());
        Ok(Some(if state.hierarchical_symbols {
            DocumentSymbolResponse::Nested(symbols)
        } else {
            DocumentSymbolResponse::Flat(crate::symbols::flat_symbols(
                symbols,
                &params.text_document.uri,
            ))
        }))
    }
    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let at = params.text_document_position;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state
            .workspace
            .documents
            .get(&path)
            .map(|doc| crate::typing::on_type(doc, at.position, &params.ch)))
    }
    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        Ok(
            crate::hierarchy::prepare(ws, &path, at.position).map(|symbol| {
                vec![crate::hierarchy::item(
                    ws,
                    &symbol,
                    Local::now().fixed_offset(),
                )]
            }),
        )
    }
    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some(symbol) = crate::hierarchy::decode(ws, &params.item) else {
            return Ok(None);
        };
        let now = Local::now().fixed_offset();
        Ok(Some(
            crate::hierarchy::dependents(ws, &symbol)
                .into_iter()
                .map(|(from, spans)| CallHierarchyIncomingCall {
                    from_ranges: crate::hierarchy::ranges(ws, &from.path, &spans),
                    from: crate::hierarchy::item(ws, &from, now),
                })
                .collect(),
        ))
    }
    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some(symbol) = crate::hierarchy::decode(ws, &params.item) else {
            return Ok(None);
        };
        let now = Local::now().fixed_offset();
        Ok(Some(
            crate::hierarchy::dependencies(ws, &symbol)
                .into_iter()
                .map(|(to, spans)| CallHierarchyOutgoingCall {
                    from_ranges: crate::hierarchy::ranges(ws, &symbol.path, &spans),
                    to: crate::hierarchy::item(ws, &to, now),
                })
                .collect(),
        ))
    }
    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state
            .workspace
            .documents
            .get(&path)
            .map(crate::symbols::folding_ranges))
    }
    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let request = crate::RequestContext::new(&state.workspace, Local::now().fixed_offset());
        crate::features::module_features::formatting(&request, &path)
            .map(Some)
            .map_err(Error::invalid_params)
    }
}
pub async fn serve() {
    let (service, socket) = LspService::build(Backend::new)
        .custom_method("wtf/query", Backend::query)
        .finish();
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
