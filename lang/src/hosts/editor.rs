//! The language server host.
//!
//! Every request evaluates the workspace at "now". Tests need that clock to be
//! reproducible, so `WTF_NOW` freezes it: set it to an RFC3339 timestamp (for
//! example `2026-09-16T14:00:00-04:00`) and [`now`] returns that instant for
//! the life of the process instead of reading the system clock. An unset or
//! unparseable value is ignored and the real clock is used.
use crate::actions::TaskToggle;
use crate::commands::{Action, Capabilities, PreparedAction};
use crate::{
    document::identifier,
    model::session::WorkspaceSession,
    paths,
    session::RefreshReport,
    workspace::{SymbolKind, Workspace},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use tower_lsp::{
    Client, LanguageServer, LspService, Server,
    jsonrpc::{Error, Result},
    lsp_types::*,
};

pub use crate::presentation::semantic_tokens;
use crate::presentation::{TOKEN_MODIFIERS, TOKEN_TYPES};

/// The current time, or the instant pinned by the `WTF_NOW` environment
/// variable (read once, at startup).
pub fn now() -> chrono::DateTime<chrono::FixedOffset> {
    static FROZEN: std::sync::OnceLock<Option<chrono::DateTime<chrono::FixedOffset>>> =
        std::sync::OnceLock::new();
    (*FROZEN.get_or_init(|| {
        std::env::var("WTF_NOW")
            .ok()
            .and_then(|text| chrono::DateTime::parse_from_rfc3339(text.trim()).ok())
    }))
    .unwrap_or_else(|| chrono::Local::now().fixed_offset())
}

struct State {
    session: WorkspaceSession,
    hint_refresh: bool,
    watch: bool,
    lens_refresh: bool,
    snippets: bool,
    hierarchical_symbols: bool,
}
#[derive(Clone)]
pub struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
}
#[derive(serde::Deserialize)]
struct QueryParams {
    query: String,
    uri: Option<Url>,
    now: Option<chrono::DateTime<chrono::FixedOffset>>,
}
impl Backend {
    async fn query(&self, params: QueryParams) -> Result<serde_json::Value> {
        let compiled = crate::query::Query::parse(&params.query).map_err(Error::invalid_params)?;
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
            state
                .session
                .workspace
                .include_file(path)
                .map_err(Error::invalid_params)?;
        }
        compiled.load_imports(&mut state.session.workspace, only.as_deref());
        let result = crate::RequestContext::new(&state.session.workspace, now)
            .query(&compiled, only.as_deref())
            .map_err(Error::invalid_params)?;
        let versions = state.session.versions_json();
        Ok(
            serde_json::json!({"schemaVersion":1,"now":now.to_rfc3339(),"rows":result.rows,"versions":versions}),
        )
    }
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State {
                session: WorkspaceSession::editor(Workspace {
                    roots: Vec::new(),
                    documents: BTreeMap::new(),
                    cache: BTreeMap::new(),
                    lookups: BTreeMap::new(),
                    modules: Default::default(),
                }),
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
        let (report, hint_refresh, lens_refresh) = {
            let mut state = self.state.write().await;
            let report = state.session.refresh(now());
            (report, state.hint_refresh, state.lens_refresh)
        };
        self.publish(report, hint_refresh, lens_refresh).await;
    }
    /// A clock tick: only the notes that read the clock, and only what moved.
    async fn tick(&self) {
        let (report, hint_refresh, lens_refresh) = {
            let mut state = self.state.write().await;
            let Some(report) = state.session.refresh_live(now()) else {
                return;
            };
            (report, state.hint_refresh, state.lens_refresh)
        };
        self.publish(report, hint_refresh, lens_refresh).await;
    }
    async fn publish(&self, report: RefreshReport, hint_refresh: bool, lens_refresh: bool) {
        for (uri, version, diagnostics) in report.diagnostics {
            self.client
                .publish_diagnostics(uri, diagnostics, Some(version))
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
    async fn update(&self, uri: Url, version: i32, text: String) {
        if let Ok(path) = uri.to_file_path() {
            let mut state = self.state.write().await;
            if state.session.open(&path, version, text).is_err() {
                return;
            }
        }
        self.notify_changes().await;
    }
    /// Reload the notes on disk, keeping the open buffers on top of them.
    async fn rescan(&self) {
        let (roots, modules) = {
            let state = self.state.read().await;
            (
                state.session.workspace.roots.clone(),
                state.session.workspace.modules.clone(),
            )
        };
        let result = tokio::task::spawn_blocking(move || {
            let (mut workspace, mut errors) = Workspace::scan_notes(roots);
            workspace.modules = modules;
            if let Err(error) = workspace.reload_modules() {
                errors.push(error);
            }
            (workspace, errors)
        })
        .await;
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
}
fn file(uri: &Url) -> Result<PathBuf> {
    uri.to_file_path()
        .map_err(|_| Error::invalid_params("WTF needs a local file URI"))
}
use crate::intelligence::symbol_at;
fn edit_for(state: &State, path: &Path, edits: Vec<TextEdit>) -> WorkspaceEdit {
    versioned_edit(path, edits, state.session.version(path))
}
fn versioned_edit(path: &Path, edits: Vec<TextEdit>, version: Option<i32>) -> WorkspaceEdit {
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: paths::uri(path),
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
            state.session.workspace.roots = roots;
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
            let _=self.client.register_capability(vec![Registration{id:"wtf-notes".into(),method:"workspace/didChangeWatchedFiles".into(),register_options:Some(serde_json::json!({"watchers":[{"globPattern":"**/*.wtf"},{"globPattern":"**/.wtf/cache.json"},{"globPattern":"**/.wtf/lookups.json"},{"globPattern":"**/.wtf/modules.json"}]}))}]).await;
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
            self.state.write().await.session.close(&path);
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
        Ok(state
            .session
            .workspace
            .documents
            .contains_key(&path)
            .then(|| {
                crate::RequestContext::new(&state.session.workspace, now())
                    .hints(&path, params.range)
                    .hints
            }))
    }
    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state.session.document_for_highlighting(&path).map(|doc| {
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
        let request = crate::RequestContext::new(&state.session.workspace, now());
        Ok(request.hover(&path, at.position))
    }
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        Ok(
            symbol_at(&state.session.workspace, &path, at.position).map(|(s, _)| {
                GotoDefinitionResponse::Scalar(Location {
                    uri: paths::uri(&s.path),
                    range: state
                        .session
                        .workspace
                        .named(&s)
                        .span
                        .range(&state.session.workspace.documents[&s.path].text),
                })
            }),
        )
    }
    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let at = params.text_document_position;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.session.workspace;
        let Some((symbol, _)) = symbol_at(ws, &path, at.position) else {
            return Ok(None);
        };
        let result = crate::intelligence::occurrences(ws, &symbol)
            .into_iter()
            .skip(usize::from(!params.context.include_declaration))
            .map(|(p, span)| Location {
                uri: paths::uri(&p),
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
        let ws = &state.session.workspace;
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
                    uri: paths::uri(&p),
                    version: state.session.version(&p),
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
        let request = crate::RequestContext::new(&state.session.workspace, now());
        Ok(Some(CompletionResponse::Array(request.completions(
            &path,
            at.position,
            state.snippets,
        ))))
    }
    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state
            .session
            .workspace
            .documents
            .get(&path)
            .and_then(|doc| crate::intelligence::signature(doc, &path, at.position)))
    }
    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let request = crate::RequestContext::new(&state.session.workspace, now());
        Ok(Some(request.code_lenses(&path, Capabilities::NATIVE)))
    }
    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let at = params.text_document_position_params;
        let path = file(&at.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.session.workspace;
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
            symbol_at(&state.session.workspace, &path, params.position).map(|(s, span)| {
                PrepareRenameResponse::RangeWithPlaceholder {
                    range: span.range(&state.session.workspace.documents[&path].text),
                    placeholder: state.session.workspace.named(&s).name.clone(),
                }
            }),
        )
    }
    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        if !state.session.workspace.documents.contains_key(&path) {
            return Ok(None);
        }
        let request = crate::RequestContext::new(&state.session.workspace, now());
        Ok(Some(request.document_links(&path)))
    }
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        if !state.session.workspace.documents.contains_key(&path) {
            return Ok(None);
        }
        let request = crate::RequestContext::new(&state.session.workspace, now());
        let mut result: Vec<_> = request
            .code_actions(
                &path,
                params.range,
                Capabilities::NATIVE,
                TaskToggle::Action,
            )
            .into_iter()
            .map(|item| match item.command {
                Some(command) => CodeActionOrCommand::Command(command),
                None => CodeActionOrCommand::CodeAction(CodeAction {
                    title: item.title,
                    kind: item.kind,
                    edit: item
                        .disabled
                        .is_none()
                        .then(|| edit_for(&state, &path, item.edits)),
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
        let (prepared, version) = {
            let state = self.state.read().await;
            let request = crate::RequestContext::new(&state.session.workspace, now());
            let prepared = action
                .prepare(&request, Capabilities::NATIVE)
                .map_err(Error::invalid_params)?;
            let version = match &prepared {
                PreparedAction::Edit { path, .. } => state.session.version(path),
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
                let snapshot = self.state.read().await.session.workspace.clone();
                let metadata = snapshot
                    .link_features()
                    .fetch(&resource.target)
                    .await
                    .map_err(Error::invalid_params)?;
                let workspace = {
                    let mut state = self.state.write().await;
                    if !Arc::ptr_eq(&snapshot.modules, &state.session.workspace.modules) {
                        return Err(Error::invalid_params(
                            "Modules changed during refresh; refresh again",
                        ));
                    }
                    state
                        .session
                        .workspace
                        .cache
                        .insert(resource.target, metadata);
                    state.session.workspace.clone()
                };
                tokio::task::spawn_blocking(move || workspace.save_cache())
                    .await
                    .map_err(|e| Error::invalid_params(e.to_string()))?
                    .map_err(Error::invalid_params)?;
                self.notify_changes().await;
            }
            PreparedAction::Refresh { path } => {
                let mut workspace = self.state.read().await.session.workspace.clone();
                let mut errors =
                    crate::cli::refresh_in_memory(&mut workspace, path.as_deref()).await;
                let workspace = {
                    let mut state = self.state.write().await;
                    if !Arc::ptr_eq(&workspace.modules, &state.session.workspace.modules) {
                        return Err(Error::invalid_params(
                            "Modules changed during refresh; refresh again",
                        ));
                    }
                    state.session.workspace.cache = workspace.cache;
                    state.session.workspace.lookups = workspace.lookups;
                    state.session.workspace.clone()
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
                let workspace = self.state.read().await.session.workspace.clone();
                let content = crate::features::agenda::today_markdown(&crate::RequestContext::new(
                    &workspace,
                    now(),
                ))
                .map_err(Error::invalid_params)?;
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
                        uri: paths::uri(dir.join("today.md")),
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
        let ws = &state.session.workspace;
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
                    uri: paths::uri(&s.path),
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
        let ws = &state.session.workspace;
        if !ws.documents.contains_key(&path) {
            return Ok(None);
        }
        let symbols = crate::RequestContext::new(ws, now()).document_symbols(&path);
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
            .session
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
        let ws = &state.session.workspace;
        let request = crate::RequestContext::new(ws, now());
        Ok(crate::hierarchy::prepare(ws, &path, at.position)
            .map(|symbol| vec![request.hierarchy_item(&symbol)]))
    }
    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let state = self.state.read().await;
        let ws = &state.session.workspace;
        let Some(symbol) = crate::hierarchy::decode(ws, &params.item) else {
            return Ok(None);
        };
        let request = crate::RequestContext::new(ws, now());
        Ok(Some(
            crate::hierarchy::dependents(ws, &symbol)
                .into_iter()
                .map(|(from, spans)| CallHierarchyIncomingCall {
                    from_ranges: crate::hierarchy::ranges(ws, &from.path, &spans),
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
        let ws = &state.session.workspace;
        let Some(symbol) = crate::hierarchy::decode(ws, &params.item) else {
            return Ok(None);
        };
        let request = crate::RequestContext::new(ws, now());
        Ok(Some(
            crate::hierarchy::dependencies(ws, &symbol)
                .into_iter()
                .map(|(to, spans)| CallHierarchyOutgoingCall {
                    from_ranges: crate::hierarchy::ranges(ws, &symbol.path, &spans),
                    to: request.hierarchy_item(&to),
                })
                .collect(),
        ))
    }
    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        Ok(state
            .session
            .document_for_highlighting(&path)
            .map(crate::symbols::folding_ranges))
    }
    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let request = crate::RequestContext::new(&state.session.workspace, now());
        request
            .formatting(&path)
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
