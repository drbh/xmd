//! The language server: the LSP protocol over stdio, on top of what
//! every host shares (`services` for what editors show and do, `native` for
//! files, refresh and the clock). `xmd lsp` calls [`serve`]. Exposes its
//! interface from the root.
//!
//! Every request evaluates the workspace at "now", read from [`native::now`].
use lang::common::{uri, uri_from_url, url_from_uri};
use lang::document::identifier;
use lang::eval::{SymbolKind, Workspace};
use native::{WorkspaceFiles, now};
use services::commands::{Action, Capabilities, PreparedAction};
use services::{
    NoteFiles, Query, Request, RowActions, TOKEN_MODIFIERS, TOKEN_TYPES, WorkspaceSession,
    definition, flat_symbols, folding_ranges, hierarchy, highlights, references, rename,
    semantic_tokens, signature, symbol_at, today_markdown, typing,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{RwLock, RwLockReadGuard};
use tower_lsp_server::{
    Client, LanguageServer, LspService, Server,
    jsonrpc::{Error, Result},
    lsp_types::*,
};
use url::Url;

struct State {
    session: WorkspaceSession,
    hint_refresh: bool,
    watch: bool,
    lens_refresh: bool,
    snippets: bool,
    hierarchical_symbols: bool,
}
#[derive(Clone)]
pub(crate) struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
}
#[derive(serde::Deserialize)]
struct QueryParams {
    query: String,
    uri: Option<Url>,
    now: Option<chrono::DateTime<chrono::FixedOffset>>,
}
/// An empty workspace over `roots` that resolves `~/` links natively.
fn workspace(roots: Vec<PathBuf>) -> Workspace {
    let mut workspace = Workspace::new(roots);
    workspace.set_home(native::home());
    workspace
}
impl Backend {
    async fn query(&self, params: QueryParams) -> Result<serde_json::Value> {
        let compiled = Query::parse(&params.query).map_err(Error::invalid_params)?;
        self.rescan().await;
        let now = params.now.unwrap_or_else(now);
        let only = params
            .uri
            .map(|uri| {
                uri.to_file_path()
                    .map_err(|_| Error::invalid_params("Query uri must be a local file URI"))
            })
            .transpose()?;
        let mut state = self.state.write().await;
        if let Some(path) = &only {
            let workspace = state.session.workspace_mut();
            native::DiskFiles
                .include_file(workspace, path)
                .map_err(Error::invalid_params)?;
        }
        compiled.load_imports(
            state.session.workspace_mut(),
            only.as_deref(),
            &native::DiskFiles,
        );
        state
            .session
            .query(&compiled, only.as_deref(), now)
            .map_err(Error::invalid_params)
    }
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State {
                session: WorkspaceSession::editor(
                    workspace(Vec::new()),
                    Arc::new(native::DiskFiles),
                ),
                hint_refresh: false,
                watch: false,
                lens_refresh: false,
                snippets: false,
                hierarchical_symbols: false,
            })),
        }
    }
    /// Re-evaluate every open note and tell the client about all of it.
    async fn notify_changes(&self) {
        self.publish(true).await;
    }
    /// A clock tick: only the notes that read the clock, and only what moved.
    async fn tick(&self) {
        self.publish(false).await;
    }
    /// Re-evaluate every open note, or only the live ones, and tell the
    /// client what it has not been told.
    async fn publish(&self, all: bool) {
        let (report, hint_refresh, lens_refresh) = {
            let mut state = self.state.write().await;
            let report = if all {
                state.session.refresh(now())
            } else {
                let Some(report) = state.session.refresh_live(now()) else {
                    return;
                };
                report
            };
            (report, state.hint_refresh, state.lens_refresh)
        };
        for (uri, version, diagnostics) in report.diagnostics {
            self.client
                .publish_diagnostics(uri_from_url(&uri), diagnostics, Some(version))
                .await;
        }
        // Only evaluated features need workspace refreshes. Semantic tokens depend
        // on document text, and clients request updates when their buffers change.
        if hint_refresh {
            let _ = self.client.inlay_hint_refresh().await;
        }
        if lens_refresh && report.lenses_changed {
            let _ = self.client.code_lens_refresh().await;
        }
    }
    async fn update(&self, uri: Uri, version: i32, text: String) {
        if let Ok(path) = url_from_uri(&uri).to_file_path() {
            let mut state = self.state.write().await;
            if state.session.open(&path, version, text).is_err() {
                return;
            }
        }
        self.notify_changes().await;
    }
    /// Reload the notes on disk and tell the client what changed.
    async fn reload(&self) {
        self.rescan().await;
        self.notify_changes().await;
    }
    /// Reload the notes on disk, keeping the open buffers on top of them.
    async fn rescan(&self) {
        let (roots, modules) = {
            let state = self.state.read().await;
            (
                state.session.workspace().roots().to_vec(),
                state.session.workspace().modules().clone(),
            )
        };
        let result = tokio::task::spawn_blocking(move || Workspace::rescan(roots, modules)).await;
        match result {
            Ok((workspace, errors)) => {
                for error in errors {
                    self.client.log_message(MessageType::ERROR, error).await;
                }
                self.state.write().await.session.rescan(workspace);
            }
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, e.to_string())
                    .await
            }
        }
    }
    /// The note a request names, and the state to answer it from.
    async fn note(&self, uri: &Uri) -> Result<(PathBuf, RwLockReadGuard<'_, State>)> {
        let path = url_from_uri(uri)
            .to_file_path()
            .map_err(|_| Error::invalid_params("XMD needs a local file URI"))?;
        Ok((path, self.state.read().await))
    }
    /// Like `note`, but `None` when the workspace has no such document.
    async fn open_note(&self, uri: &Uri) -> Result<Option<(PathBuf, RwLockReadGuard<'_, State>)>> {
        let (path, state) = self.note(uri).await?;
        let known = state.session.workspace().documents().contains_key(&path);
        Ok(known.then_some((path, state)))
    }
    /// Fold what a refresh of `snapshot` fetched into the session with
    /// `adopt`, unless its modules changed meanwhile, then save the cache.
    async fn adopt(
        &self,
        snapshot: &Workspace,
        adopt: impl FnOnce(&mut Workspace),
    ) -> Result<std::result::Result<(), String>> {
        let (root, cache) = {
            let mut state = self.state.write().await;
            if !Arc::ptr_eq(snapshot.modules(), state.session.workspace().modules()) {
                return Err(Error::invalid_params(
                    "Modules changed during refresh; refresh again",
                ));
            }
            adopt(state.session.workspace_mut());
            let workspace = state.session.workspace();
            (workspace.root().to_path_buf(), workspace.cache().clone())
        };
        let saved = tokio::task::spawn_blocking(move || native::save_cache(&root, &cache));
        Ok(saved.await.unwrap_or_else(|e| Err(e.to_string())))
    }
}

impl LanguageServer for Backend {
    #[allow(deprecated)]
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let mut roots: Vec<_> = params
            .workspace_folders
            .unwrap_or_default()
            .iter()
            .filter_map(|f| url_from_uri(&f.uri).to_file_path().ok())
            .collect();
        if roots.is_empty() {
            let root = params
                .root_uri
                .and_then(|u| url_from_uri(&u).to_file_path().ok());
            roots.extend(root.or_else(|| std::env::current_dir().ok()));
        }
        // An editor that opens a single note (Zed does) names that file as
        // the workspace folder; the workspace is the folder it sits in.
        for root in &mut roots {
            if root.is_file()
                && let Some(parent) = root.parent()
            {
                *root = parent.to_path_buf();
            }
        }
        roots.dedup();
        {
            let mut state = self.state.write().await;
            // Nothing is open before initialization, so the workspace starts
            // over on the client's roots.
            *state.session.workspace_mut() = workspace(roots);
            let text_document = params.capabilities.text_document.as_ref();
            state.hierarchical_symbols = text_document
                .and_then(|t| t.document_symbol.as_ref())
                .is_some_and(|s| s.hierarchical_document_symbol_support == Some(true));
            state.snippets = text_document
                .and_then(|t| t.completion.as_ref())
                .and_then(|c| c.completion_item.as_ref())
                .is_some_and(|c| c.snippet_support == Some(true));
            if let Some(w) = params.capabilities.workspace {
                state.lens_refresh = w.code_lens.is_some_and(|c| c.refresh_support == Some(true));
                state.hint_refresh = w
                    .inlay_hint
                    .is_some_and(|c| c.refresh_support == Some(true));
                state.watch = w
                    .did_change_watched_files
                    .is_some_and(|c| c.dynamic_registration == Some(true));
            }
        }
        self.rescan().await;
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "XMD".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            // What the server offers, as the protocol spells it.
            capabilities: serde_json::from_value(serde_json::json!({
                "experimental": {"xmdQuery": {"method": "xmd/query", "schemaVersion": 1}},
                "textDocumentSync": TextDocumentSyncKind::FULL,
                "inlayHintProvider": true,
                "hoverProvider": true,
                "definitionProvider": true,
                "referencesProvider": true,
                "renameProvider": {"prepareProvider": true},
                "documentHighlightProvider": true,
                "codeLensProvider": {"resolveProvider": false},
                "signatureHelpProvider": {
                    "triggerCharacters": ["(", ","],
                    "retriggerCharacters": [")"],
                },
                "completionProvider": {"triggerCharacters": ["[", "@", "."]},
                "documentLinkProvider": {"resolveProvider": false},
                "codeActionProvider": true,
                "executeCommandProvider": {"commands": Action::commands().collect::<Vec<_>>()},
                "workspaceSymbolProvider": true,
                "documentSymbolProvider": true,
                "documentFormattingProvider": true,
                "documentOnTypeFormattingProvider": {
                    "firstTriggerCharacter": typing::TRIGGERS[0],
                    "moreTriggerCharacter": typing::TRIGGERS[1..],
                },
                "callHierarchyProvider": true,
                "foldingRangeProvider": true,
                "semanticTokensProvider": {
                    "legend": {"tokenTypes": TOKEN_TYPES, "tokenModifiers": TOKEN_MODIFIERS},
                    "full": true,
                },
            }))
            .expect("the server's capabilities are well formed"),
        })
    }
    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "XMD: tasks, timers, resources, dates, and workspace navigation ready",
            )
            .await;
        if self.state.read().await.watch {
            // Notes and libraries, and the workspace's own settings.
            let watchers = [lang::common::EXTENSION, lang::common::LIBRARY_EXTENSION]
                .map(|extension| format!("**/*.{extension}"))
                .into_iter()
                .chain(
                    ["cache.json", "lookups.json", "modules.json"]
                        .map(|file| format!("**/.xmd/{file}")),
                )
                .map(|glob| serde_json::json!({ "globPattern": glob }))
                .collect::<Vec<_>>();
            let _ = self
                .client
                .register_capability(vec![Registration {
                    id: "xmd-notes".into(),
                    method: "workspace/didChangeWatchedFiles".into(),
                    register_options: Some(serde_json::json!({ "watchers": watchers })),
                }])
                .await;
        }
        let backend = self.clone();
        tokio::spawn(async move {
            let mut today = now().date_naive();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let next = now().date_naive();
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
        self.state.write().await.session.clear_live();
        Ok(())
    }
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        self.update(document.uri, document.version, document.text)
            .await;
    }
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last()
            && change.range.is_none()
        {
            let document = params.text_document;
            self.update(document.uri, document.version, change.text)
                .await;
        }
    }
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if let Ok(path) = url_from_uri(&params.text_document.uri).to_file_path() {
            self.state.write().await.session.close(&path);
            self.rescan().await;
            self.client
                .publish_diagnostics(params.text_document.uri, vec![], None)
                .await;
            self.notify_changes().await;
        }
    }
    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.reload().await;
    }
    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        self.reload().await;
    }
    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let Some((path, state)) = self.open_note(&params.text_document.uri).await? else {
            return Ok(None);
        };
        let request = state.session.request(now());
        Ok(Some(request.hints(&path, params.range).hints))
    }
    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let (path, state) = self.note(&params.text_document.uri).await?;
        let library = state.session.workspace().prelude_names(&path);
        Ok(state.session.document_for_highlighting(&path).map(|doc| {
            SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: semantic_tokens(doc, &library),
            })
        }))
    }
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let at = params.text_document_position_params;
        let (path, state) = self.note(&at.text_document.uri).await?;
        Ok(state.session.request(now()).hover(&path, at.position))
    }
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let at = params.text_document_position_params;
        let (path, state) = self.note(&at.text_document.uri).await?;
        let ws = state.session.workspace();
        Ok(symbol_at(ws, &path, at.position)
            .map(|(symbol, _)| GotoDefinitionResponse::Scalar(definition(ws, &symbol))))
    }
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let at = params.text_document_position;
        let (path, state) = self.note(&at.text_document.uri).await?;
        let ws = state.session.workspace();
        let skip = usize::from(!params.context.include_declaration);
        Ok(symbol_at(ws, &path, at.position)
            .map(|(symbol, _)| references(ws, &symbol).into_iter().skip(skip).collect()))
    }
    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        if !identifier(&params.new_name) {
            return Err(Error::invalid_params(
                "Names use letters, digits, and underscores and cannot start with a digit",
            ));
        }
        let at = params.text_document_position;
        let (path, state) = self.note(&at.text_document.uri).await?;
        let ws = state.session.workspace();
        let Some((symbol, _)) = symbol_at(ws, &path, at.position) else {
            return Ok(None);
        };
        let changes = rename(ws, &symbol, &params.new_name).map_err(Error::invalid_params)?;
        Ok(Some(state.session.edit(changes)))
    }
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let at = params.text_document_position;
        let (path, state) = self.note(&at.text_document.uri).await?;
        let request = state.session.request(now());
        Ok(Some(CompletionResponse::Array(request.completions(
            &path,
            at.position,
            state.snippets,
        ))))
    }
    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let at = params.text_document_position_params;
        let (path, state) = self.note(&at.text_document.uri).await?;
        Ok(signature(state.session.workspace(), &path, at.position))
    }
    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let (path, state) = self.note(&params.text_document.uri).await?;
        let request = state.session.request(now());
        Ok(Some(request.code_lenses(&path, Capabilities::NATIVE)))
    }
    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let at = params.text_document_position_params;
        let (path, state) = self.note(&at.text_document.uri).await?;
        let ws = state.session.workspace();
        Ok(symbol_at(ws, &path, at.position).map(|(symbol, _)| highlights(ws, &path, &symbol)))
    }
    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let (path, state) = self.note(&params.text_document.uri).await?;
        let ws = state.session.workspace();
        Ok(symbol_at(ws, &path, params.position).map(|(symbol, span)| {
            PrepareRenameResponse::RangeWithPlaceholder {
                range: span.range(&ws.documents()[&path]),
                placeholder: ws.named(&symbol).name.clone(),
            }
        }))
    }
    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let note = self.open_note(&params.text_document.uri).await?;
        Ok(note.map(|(path, state)| state.session.request(now()).document_links(&path)))
    }
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let Some((path, state)) = self.open_note(&params.text_document.uri).await? else {
            return Ok(None);
        };
        let request = state.session.request(now());
        let mut result: Vec<_> = request
            .code_actions(&path, params.range, Capabilities::NATIVE, RowActions::Edit)
            .into_iter()
            .map(|item| match item.command {
                Some(command) => CodeActionOrCommand::Command(command),
                None => CodeActionOrCommand::CodeAction(CodeAction {
                    title: item.title,
                    kind: item.kind,
                    edit: item
                        .disabled
                        .is_none()
                        .then(|| state.session.edit([(path.clone(), item.edits)])),
                    disabled: item.disabled.map(|reason| CodeActionDisabled { reason }),
                    ..Default::default()
                }),
            })
            .collect();
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
        let (prepared, edit) = {
            let state = self.state.read().await;
            let request = state.session.request(now());
            let prepared = request
                .prepare(&action, Capabilities::NATIVE)
                .map_err(Error::invalid_params)?;
            let edit = match &prepared {
                PreparedAction::Edit { path, edits } => {
                    state.session.edit([(path.clone(), edits.clone())])
                }
                _ => WorkspaceEdit::default(),
            };
            (prepared, edit)
        };
        match prepared {
            PreparedAction::Edit { .. } => {
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
                        uri: uri_from_url(&url),
                        take_focus: Some(true),
                        selection: None,
                    })
                    .await?;
                if !opened {
                    return Err(Error::invalid_params("Editor could not open this resource"));
                }
            }
            PreparedAction::RefreshResource { resource } => {
                let snapshot = self.state.read().await.session.workspace().clone();
                let metadata = native::fetch_link(snapshot.link_features(), &resource.target)
                    .await
                    .map_err(Error::invalid_params)?;
                self.adopt(&snapshot, |workspace| {
                    workspace.store_link_status(resource.target, metadata)
                })
                .await?
                .map_err(Error::invalid_params)?;
                self.notify_changes().await;
            }
            PreparedAction::Refresh { path } => {
                let mut workspace = self.state.read().await.session.workspace().clone();
                let mut errors =
                    native::refresh_workspace(&mut workspace, now(), path.as_deref()).await;
                let saved = self
                    .adopt(&workspace, |own| own.adopt_caches(&workspace))
                    .await?;
                errors.extend(saved.err());
                self.notify_changes().await;
                let (kind, message) = if errors.is_empty() {
                    (
                        MessageType::INFO,
                        "Resource status and lookups refreshed".into(),
                    )
                } else {
                    (MessageType::WARNING, errors.join("\n"))
                };
                self.client.show_message(kind, message).await;
            }
            PreparedAction::ShowToday => {
                self.rescan().await;
                let workspace = self.state.read().await.session.workspace().clone();
                let content = today_markdown(&Request::new(&workspace, now()))
                    .map_err(Error::invalid_params)?;
                let dir = workspace.root().join(".xmd");
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
                        uri: uri_from_url(&uri(dir.join("today.md"))),
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
    ) -> Result<Option<OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>>> {
        let state = self.state.read().await;
        let ws = state.session.workspace();
        let query = params.query.to_lowercase();
        #[allow(deprecated)]
        let result = ws
            .symbols()
            .into_iter()
            .filter(|s| ws.named(s).name.to_lowercase().contains(&query))
            .map(|s| SymbolInformation {
                name: ws.named(&s).name.clone(),
                kind: match s.kind {
                    SymbolKind::Task(_) => tower_lsp_server::lsp_types::SymbolKind::BOOLEAN,
                    SymbolKind::Section(_) => tower_lsp_server::lsp_types::SymbolKind::ARRAY,
                    _ => tower_lsp_server::lsp_types::SymbolKind::VARIABLE,
                },
                tags: None,
                deprecated: None,
                location: definition(ws, &s),
                container_name: Some(s.path.display().to_string()),
            })
            .collect();
        Ok(Some(OneOf::Left(result)))
    }
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some((path, state)) = self.open_note(&params.text_document.uri).await? else {
            return Ok(None);
        };
        let symbols = state.session.request(now()).document_symbols(&path);
        Ok(Some(if state.hierarchical_symbols {
            DocumentSymbolResponse::Nested(symbols)
        } else {
            DocumentSymbolResponse::Flat(flat_symbols(
                symbols,
                &url_from_uri(&params.text_document.uri),
            ))
        }))
    }
    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let at = params.text_document_position;
        let Some((path, state)) = self.open_note(&at.text_document.uri).await? else {
            return Ok(None);
        };
        let request = state.session.request(now());
        Ok(Some(request.on_type(&path, at.position, &params.ch)))
    }
    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let at = params.text_document_position_params;
        let (path, state) = self.note(&at.text_document.uri).await?;
        let request = state.session.request(now());
        Ok(
            hierarchy::prepare(state.session.workspace(), &path, at.position)
                .map(|symbol| vec![request.hierarchy_item(&symbol)]),
        )
    }
    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let state = self.state.read().await;
        let ws = state.session.workspace();
        let Some(symbol) = hierarchy::decode(ws, &params.item) else {
            return Ok(None);
        };
        let request = state.session.request(now());
        Ok(Some(
            hierarchy::dependents(ws, &symbol)
                .into_iter()
                .map(|(from, spans)| CallHierarchyIncomingCall {
                    from_ranges: hierarchy::ranges(ws, &from.path, &spans),
                    from: request.hierarchy_item(&from),
                })
                .collect(),
        ))
    }
    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let state = self.state.read().await;
        let ws = state.session.workspace();
        let Some(symbol) = hierarchy::decode(ws, &params.item) else {
            return Ok(None);
        };
        let request = state.session.request(now());
        Ok(Some(
            hierarchy::dependencies(ws, &symbol)
                .into_iter()
                .map(|(to, spans)| CallHierarchyOutgoingCall {
                    from_ranges: hierarchy::ranges(ws, &symbol.path, &spans),
                    to: request.hierarchy_item(&to),
                })
                .collect(),
        ))
    }
    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let (path, state) = self.note(&params.text_document.uri).await?;
        Ok(state
            .session
            .document_for_highlighting(&path)
            .map(folding_ranges))
    }
    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let (path, state) = self.note(&params.text_document.uri).await?;
        let request = state.session.request(now());
        request
            .formatting(&path)
            .map(Some)
            .map_err(Error::invalid_params)
    }
}
/// Serve the language server over stdin and stdout until the client exits.
pub async fn serve() {
    let (service, socket) = LspService::build(Backend::new)
        .custom_method("xmd/query", Backend::query)
        .finish();
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
