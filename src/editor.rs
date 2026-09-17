use crate::{
    actions,
    document::{Document, Problem, Span, byte_at, identifier},
    engine::{Engine, Value, next_occurrence},
    resources::Resource,
    timers,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset, Local, NaiveDate};
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

const TOKEN_TYPES: &[&str] = &[
    "comment", "keyword", "number", "variable", "operator", "string", "heading",
];
pub fn problems(workspace: &Workspace, path: &Path, today: NaiveDate) -> Vec<Problem> {
    let doc = &workspace.documents[path];
    let mut issues = doc.problems.clone();
    let mut engine = Engine::new(workspace, today);
    for symbol in workspace.symbols().into_iter().filter(|s| s.path == path) {
        let named = workspace.named(&symbol);
        if let Err(message) = workspace.resolve(path, &named.name) {
            issues.push(Problem {
                span: named.span,
                message,
            });
        }
        if let Err(message) = engine.symbol(&symbol) {
            issues.push(Problem {
                span: named.span,
                message,
            });
        }
        if let Ok(Value::Resource(resource)) = engine.symbol(&symbol)
            && let Err(message) = resource.url(path)
        {
            issues.push(Problem {
                span: named.span,
                message,
            });
        }
    }
    for reference in &doc.references {
        if let Err(message) = workspace.resolve(path, &reference.name) {
            issues.push(Problem {
                span: reference.span,
                message,
            });
        }
        if reference.property.is_some()
            && let Err(message) = engine.eval(path, &reference.expression())
        {
            issues.push(Problem {
                span: reference.span,
                message,
            });
        }
    }
    for (i, task) in doc.tasks.iter().enumerate() {
        if let Err(message) = engine.blocked(path, i) {
            issues.push(Problem {
                span: task.checkbox,
                message,
            });
        }
        for key in ["due", "scheduled", "at", "repeat_from"] {
            if let Some(attr) = task.attributes.get(key)
                && let Err(message) = engine.when(path, &attr.value)
            {
                issues.push(Problem {
                    span: attr.value_span,
                    message,
                });
            }
        }
        if let Some(attr) = task.attributes.get("estimate")
            && !matches!(engine.eval(path,&attr.value),Ok(Value::Duration(m)) if m>=0)
        {
            issues.push(Problem {
                span: attr.value_span,
                message: "@estimate requires a nonnegative duration, e.g. 20m or 2h".into(),
            });
        }
        if let Some(attr) = task.attributes.get("timer") {
            match engine.eval(path, &attr.value) {
                Ok(Value::Timer(timer)) if timer.origin.is_some() && identifier(&attr.value) => {}
                _ => issues.push(Problem {
                    span: attr.value_span,
                    message: "@timer requires a named stopwatch or countdown, e.g. @timer(focus)"
                        .into(),
                }),
            }
        }
        if let Some(attr) = task.attributes.get("every") {
            if let Err(message) = next_occurrence(&attr.value, today, today) {
                issues.push(Problem {
                    span: attr.value_span,
                    message,
                });
            }
            if doc.tasks.iter().any(|t| t.parent == Some(i)) {
                issues.push(Problem {
                    span: attr.span,
                    message: "Put recurrence on individual tasks, not parent checklists".into(),
                });
            }
        }
    }
    for event in &doc.events {
        let attr = &event.attributes["at"];
        if let Err(message) = engine.when(path, &attr.value) {
            issues.push(Problem {
                span: attr.value_span,
                message,
            });
        }
    }
    issues.sort_by_key(|p| (p.span.line, p.span.start, p.message.clone()));
    issues.dedup_by(|a, b| a.span == b.span && a.message == b.message);
    issues
}
pub fn hints(workspace: &Workspace, path: &Path, today: NaiveDate, range: Range) -> Vec<InlayHint> {
    let mut engine = Engine::new(workspace, today);
    collect_hints(&mut engine, path, range)
}
pub fn hints_at(
    workspace: &Workspace,
    path: &Path,
    now: DateTime<FixedOffset>,
    range: Range,
) -> Vec<InlayHint> {
    collect_hints(&mut Engine::at(workspace, now), path, range)
}
fn live_hints(workspace: &Workspace, path: &Path, now: DateTime<FixedOffset>) -> bool {
    let mut engine = Engine::at(workspace, now);
    collect_hints(
        &mut engine,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
    );
    engine.time_dependent
}
fn collect_hints(engine: &mut Engine<'_>, path: &Path, range: Range) -> Vec<InlayHint> {
    let workspace = engine.workspace;
    let today = engine.today;
    let doc = &workspace.documents[path];
    let mut hints = Vec::new();
    let mut push = |position: Position, label: String, tooltip: String| {
        if position >= range.start && position <= range.end {
            hints.push(InlayHint {
                position,
                label: InlayHintLabel::String(label),
                kind: None,
                text_edits: None,
                tooltip: Some(InlayHintTooltip::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: tooltip,
                })),
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
    };
    for (i, def) in doc.definitions.iter().enumerate() {
        match engine.symbol(&Symbol {
            path: path.to_path_buf(),
            kind: SymbolKind::Definition(i),
        }) {
            Ok(Value::Resource(resource)) => push(
                def.end.range(&doc.text).start,
                resource.label(&workspace.cache),
                resource.hover(path, &workspace.cache),
            ),
            Ok(value) if def.expression => push(
                def.end.range(&doc.text).start,
                format!("= {}", value.display()),
                format!("{}\n\n{} = {}", def.named.name, def.source, value.display()),
            ),
            _ => {}
        }
    }
    for section in &doc.sections {
        let tasks: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                t.line > section.line
                    && t.line < section.end_line
                    && !doc.tasks.iter().any(|t| t.parent == Some(*i))
            })
            .collect();
        if tasks.is_empty() {
            continue;
        }
        let done = tasks
            .iter()
            .filter(|(i, _)| engine.task_done(path, *i))
            .count();
        let mut effort = 0i64;
        let mut estimates = 0;
        for (i, t) in &tasks {
            if !engine.task_done(path, *i)
                && let Some(attr) = t.attributes.get("estimate")
                && let Ok(Value::Duration(m)) = engine.eval(path, &attr.value)
            {
                effort = effort.saturating_add(m);
                estimates += 1;
            }
        }
        let label = format!(
            "{done}/{} complete{}",
            tasks.len(),
            if estimates > 0 {
                format!(" · {} estimated left", Value::Duration(effort).display())
            } else {
                String::new()
            }
        );
        push(doc.line_end(section.line), label.clone(), label);
    }
    for (i, task) in doc.tasks.iter().enumerate() {
        let mut labels = Vec::new();
        if let Some(attr) = task.attributes.get("timer")
            && let Ok(Value::Timer(timer)) = engine.eval(path, &attr.value)
        {
            labels.push(timer.display());
        }
        if !engine.task_done(path, i) {
            match engine.blocked(path, i) {
                Ok(blocked) if !blocked.is_empty() => {
                    labels.push(format!("blocked by {}", blocked.join(", ")))
                }
                Err(e) => labels.push(e),
                _ => {}
            }
            for key in ["due", "scheduled", "at"] {
                if let Some(attr) = task.attributes.get(key) {
                    match engine.when(path, &attr.value) {
                        Ok(value) => {
                            let date = value.date().unwrap();
                            let delta = (date - today).num_days();
                            let relative = if delta < 0 && key == "due" {
                                format!("{}d overdue", -delta)
                            } else if delta == 0 {
                                "today".into()
                            } else if delta == 1 {
                                "tomorrow".into()
                            } else {
                                date.to_string()
                            };
                            labels.push(format!("{key} {relative}"));
                        }
                        Err(e) => labels.push(e),
                    }
                }
            }
            if let Some(attr) = task.attributes.get("every") {
                labels.push(format!("repeats every {}", attr.value));
            }
        }
        let children: Vec<_> = doc
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent == Some(i))
            .collect();
        if !children.is_empty() {
            labels.push(format!(
                "{}/{} subtasks",
                children
                    .iter()
                    .filter(|(j, _)| engine.task_done(path, *j))
                    .count(),
                children.len()
            ));
        }
        if !labels.is_empty() {
            push(doc.line_end(task.line),labels.join(" · "),"Use code actions to complete/reopen tasks or start/pause/reset their timer. Completing a task does not stop its timer.".into());
        }
    }
    for reference in doc.references.iter().filter(|r| r.bracket) {
        if let Ok(value) = engine.eval(path, &reference.expression()) {
            let target = workspace
                .resolve(path, &reference.name)
                .map(|s| s.path)
                .unwrap_or_else(|_| path.into());
            let end = reference.end()
                + doc.line(reference.span.line)[reference.end()..]
                    .find(']')
                    .unwrap_or(0)
                + 1;
            let after = Span::new(reference.span.line, end, end)
                .range(&doc.text)
                .start;
            match value {
                Value::Resource(resource) => push(
                    after,
                    resource.label(&workspace.cache),
                    resource.hover(&target, &workspace.cache),
                ),
                Value::Timer(timer) => push(
                    after,
                    timer.display(),
                    "Use Start, Pause, Resume, or Reset timer in code actions.".into(),
                ),
                value if reference.property.is_some() => {
                    push(after, value.display(), reference.expression())
                }
                _ => {}
            }
        }
    }
    hints.sort_by_key(|h| h.position);
    hints
}
pub fn semantic_tokens(doc: &Document) -> Vec<SemanticToken> {
    let mut result = Vec::new();
    let mut previous = Position::new(0, 0);
    let mut previous_end = Position::new(0, 0);
    for highlight in &doc.highlights {
        let range = highlight.span.range(&doc.text);
        if range.start < previous_end || range.start == range.end {
            continue;
        }
        let delta_line = range.start.line - previous.line;
        result.push(SemanticToken {
            delta_line,
            delta_start: if delta_line == 0 {
                range.start.character - previous.character
            } else {
                range.start.character
            },
            length: range.end.character - range.start.character,
            token_type: TOKEN_TYPES
                .iter()
                .position(|t| *t == highlight.kind)
                .unwrap_or(3) as u32,
            token_modifiers_bitset: 0,
        });
        previous = range.start;
        previous_end = range.end;
    }
    result
}

struct State {
    workspace: Workspace,
    open: BTreeMap<PathBuf, i32>,
    hint_refresh: bool,
    token_refresh: bool,
    watch: bool,
    live: BTreeSet<PathBuf>,
}
#[derive(Clone)]
pub struct Backend {
    client: Client,
    state: Arc<RwLock<State>>,
}
impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(RwLock::new(State {
                workspace: Workspace {
                    roots: Vec::new(),
                    documents: BTreeMap::new(),
                    cache: BTreeMap::new(),
                },
                open: BTreeMap::new(),
                hint_refresh: false,
                token_refresh: false,
                watch: false,
                live: BTreeSet::new(),
            })),
        }
    }
    async fn notify_changes(&self) {
        let (diagnostics, hint_refresh, token_refresh) = {
            let mut state = self.state.write().await;
            let now = Local::now().fixed_offset();
            state.live = state
                .open
                .keys()
                .filter(|path| {
                    state.workspace.documents.contains_key(*path)
                        && live_hints(&state.workspace, path, now)
                })
                .cloned()
                .collect();
            let today = Local::now().date_naive();
            let diagnostics = state
                .open
                .iter()
                .filter_map(|(path, version)| {
                    state.workspace.documents.get(path).map(|doc| {
                        (
                            Url::from_file_path(path).unwrap(),
                            *version,
                            problems(&state.workspace, path, today)
                                .into_iter()
                                .map(|p| Diagnostic {
                                    range: p.span.range(&doc.text),
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    source: Some("jot".into()),
                                    message: p.message,
                                    ..Default::default()
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                })
                .collect::<Vec<_>>();
            (diagnostics, state.hint_refresh, state.token_refresh)
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
    }
    async fn tick(&self) {
        let refresh = {
            let mut state = self.state.write().await;
            if !state.hint_refresh || state.live.is_empty() {
                return;
            }
            let refresh = state.hint_refresh && !state.live.is_empty();
            let now = Local::now().fixed_offset();
            state.live = state
                .live
                .iter()
                .filter(|path| {
                    state.open.contains_key(*path)
                        && state.workspace.documents.contains_key(*path)
                        && live_hints(&state.workspace, path, now)
                })
                .cloned()
                .collect();
            // Refresh once more at expiry before removing a countdown from the live set.
            refresh
        };
        if refresh {
            let _ = self.client.inlay_hint_refresh().await;
        }
    }
    async fn update(&self, uri: Url, version: i32, text: String) {
        if let Ok(path) = uri.to_file_path() {
            let mut state = self.state.write().await;
            if state.open.get(&path).is_some_and(|v| *v > version) {
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
        let roots = self.state.read().await.workspace.roots.clone();
        let result = tokio::task::spawn_blocking(move || Workspace::load(roots)).await;
        match result {
            Ok(Ok(mut workspace)) => {
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
        .map_err(|_| Error::invalid_params("Jot needs a local file URI"))
}
fn symbol_at(workspace: &Workspace, path: &Path, position: Position) -> Option<(Symbol, Span)> {
    let doc = workspace.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let inside =
        |span: Span| span.line == position.line as usize && byte >= span.start && byte <= span.end;
    for symbol in workspace.symbols().into_iter().filter(|s| s.path == path) {
        let span = workspace.named(&symbol).span;
        if inside(span) {
            return Some((symbol, span));
        }
    }
    doc.references
        .iter()
        .find(|r| inside(Span::new(r.span.line, r.span.start, r.end())))
        .and_then(|r| workspace.resolve(path, &r.name).ok().map(|s| (s, r.span)))
}
fn edit_for(state: &State, path: &Path, edits: Vec<TextEdit>) -> WorkspaceEdit {
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: Url::from_file_path(path).unwrap(),
                version: state.open.get(path).copied(),
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
            if let Some(w) = params.capabilities.workspace {
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
                name: "Jot".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                inlay_hint_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
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
                    commands: vec!["jot.refresh".into(), "jot.today".into(), "jot.timer".into()],
                    ..Default::default()
                }),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        legend: SemanticTokensLegend {
                            token_types: TOKEN_TYPES
                                .iter()
                                .map(|t| SemanticTokenType::new(t))
                                .collect(),
                            token_modifiers: vec![],
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
                "Jot: tasks, timers, resources, dates, and workspace navigation ready",
            )
            .await;
        if self.state.read().await.watch {
            let _=self.client.register_capability(vec![Registration{id:"jot-notes".into(),method:"workspace/didChangeWatchedFiles".into(),register_options:Some(serde_json::json!({"watchers":[{"globPattern":"**/*.jot"},{"globPattern":"**/.jot/cache.json"}]}))}]).await;
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
        let workspace = &state.workspace;
        let Some(doc) = workspace.documents.get(&path) else {
            return Ok(None);
        };
        let Some((symbol, span)) = symbol_at(workspace, &path, at.position) else {
            if let Some(task) = doc
                .tasks
                .iter()
                .find(|t| t.line == at.position.line as usize)
            {
                return Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "{}\n\nUse **Complete task** / **Reopen task** in code actions. Dates: @due, @scheduled, @at. Effort: @estimate(20m). Dependencies: @after(name).",
                        task.title
                    ))),
                    range: None,
                }));
            }
            return Ok(None);
        };
        let mut engine = Engine::new(workspace, Local::now().date_naive());
        let named = workspace.named(&symbol);
        let value = match engine.symbol(&symbol) {
            Ok(Value::Resource(r)) => r.hover(&symbol.path, &workspace.cache),
            Ok(Value::Timer(timer)) => format!(
                "{}\n\nUse **Start / Pause / Resume / Reset timer** in code actions. Timer state is saved in the note; ticking never edits it.",
                timer.display()
            ),
            Ok(v) => v.display(),
            Err(e) => e,
        };
        let property = doc
            .references
            .iter()
            .find(|r| r.span == span && r.property.is_some());
        let value = if let Some(reference) = property {
            match engine.eval(&path, &reference.expression()) {
                Ok(v) => format!("{} = {}", reference.expression(), v.display()),
                Err(e) => e,
            }
        } else {
            value
        };
        let expression = match symbol.kind {
            SymbolKind::Definition(i) => format!(
                "\n\nSource: `{}`",
                workspace.documents[&symbol.path].definitions[i].source
            ),
            _ => String::new(),
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!(
                    "**{}**\n\n{value}{expression}\n\nDefined in {}:{}",
                    named.name,
                    symbol.path.display(),
                    named.span.line + 1
                ),
            }),
            range: Some(
                property
                    .map(|r| Span::new(r.span.line, r.span.start, r.end()))
                    .unwrap_or(span)
                    .range(&doc.text),
            ),
        }))
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
        let name = &ws.named(&symbol).name;
        let mut result = Vec::new();
        if params.context.include_declaration {
            result.push(Location {
                uri: Url::from_file_path(&symbol.path).unwrap(),
                range: ws
                    .named(&symbol)
                    .span
                    .range(&ws.documents[&symbol.path].text),
            });
        }
        for (p, doc) in &ws.documents {
            for r in &doc.references {
                if r.name == *name && ws.resolve(p, name).ok().as_ref() == Some(&symbol) {
                    result.push(Location {
                        uri: Url::from_file_path(p).unwrap(),
                        range: r.span.range(&doc.text),
                    });
                }
            }
        }
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
        if ws
            .symbols()
            .iter()
            .any(|s| *s != symbol && ws.named(s).name == params.new_name)
        {
            return Err(Error::invalid_params(
                "That name already exists in this workspace",
            ));
        }
        let name = &ws.named(&symbol).name;
        let mut changes: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
        changes
            .entry(symbol.path.clone())
            .or_default()
            .push(TextEdit::new(
                ws.named(&symbol)
                    .span
                    .range(&ws.documents[&symbol.path].text),
                params.new_name.clone(),
            ));
        for (p, doc) in &ws.documents {
            for r in &doc.references {
                if r.name == *name && ws.resolve(p, name).ok().as_ref() == Some(&symbol) {
                    changes.entry(p.clone()).or_default().push(TextEdit::new(
                        r.span.range(&doc.text),
                        params.new_name.clone(),
                    ));
                }
            }
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
        let path = file(&params.text_document_position.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let position = params.text_document_position.position;
        if let Some(doc) = ws.documents.get(&path)
            && let Some(byte) = byte_at(doc.line(position.line as usize), position.character)
        {
            let prefix = &doc.line(position.line as usize)[..byte];
            if let Some(dot) = prefix.rfind('.')
                && prefix[dot + 1..]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                let name = prefix[..dot]
                    .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if let Ok(Value::Timer(timer)) =
                    Engine::at(ws, Local::now().fixed_offset()).named(&path, name)
                {
                    let names = if timer.limit.is_some() {
                        vec![
                            "elapsed",
                            "remaining",
                            "duration",
                            "running",
                            "done",
                            "state",
                        ]
                    } else {
                        vec!["elapsed", "running", "done", "state"]
                    };
                    return Ok(Some(CompletionResponse::Array(
                        names
                            .into_iter()
                            .map(|name| CompletionItem {
                                label: name.into(),
                                kind: Some(CompletionItemKind::PROPERTY),
                                ..Default::default()
                            })
                            .collect(),
                    )));
                }
            }
        }
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for symbol in ws.symbols() {
            let name = &ws.named(&symbol).name;
            if seen.insert(name.clone()) && ws.resolve(&path, name).is_ok() {
                result.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some(symbol.path.display().to_string()),
                    ..Default::default()
                });
            }
        }
        for name in [
            "today()",
            "completed()",
            "total()",
            "remaining()",
            "effort()",
            "date()",
            "now()",
            "countdown(25m)",
            "stopwatch()",
        ] {
            result.push(CompletionItem {
                label: name.into(),
                kind: Some(CompletionItemKind::FUNCTION),
                ..Default::default()
            });
        }
        Ok(Some(CompletionResponse::Array(result)))
    }
    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some(doc) = ws.documents.get(&path) else {
            return Ok(None);
        };
        let mut links = Vec::new();
        let mut add = |span: Span, resource: Resource, base: &Path| {
            if let Ok(url) = resource.url(base) {
                links.push(DocumentLink {
                    range: span.range(&doc.text),
                    target: Some(url),
                    tooltip: Some("Open resource".into()),
                    data: None,
                });
            }
        };
        for link in &doc.links {
            add(
                link.span,
                Resource {
                    target: link.target.clone(),
                    origin: None,
                },
                &path,
            );
        }
        let mut engine = Engine::new(ws, Local::now().date_naive());
        for (i, def) in doc.definitions.iter().enumerate() {
            if let Ok(Value::Resource(r)) = engine.symbol(&Symbol {
                path: path.clone(),
                kind: SymbolKind::Definition(i),
            }) {
                add(def.value_span, r, &path);
            }
        }
        for r in &doc.references {
            if let Ok(s) = ws.resolve(&path, &r.name)
                && let Ok(Value::Resource(resource)) = engine.symbol(&s)
            {
                add(r.span, resource, &s.path);
            }
        }
        Ok(Some(links))
    }
    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let path = file(&params.text_document.uri)?;
        let state = self.state.read().await;
        let ws = &state.workspace;
        let Some(doc) = ws.documents.get(&path) else {
            return Ok(None);
        };
        let today = Local::now().date_naive();
        let mut result = Vec::new();
        let mut engine = Engine::at(ws, Local::now().fixed_offset());
        let row = params.range.start.line as usize;
        let names = doc
            .definitions
            .iter()
            .filter(|d| d.named.span.line == row)
            .map(|d| d.named.name.as_str())
            .chain(
                doc.references
                    .iter()
                    .filter(|r| r.span.line == row)
                    .map(|r| r.name.as_str()),
            );
        let mut seen_timers = BTreeSet::new();
        for name in names {
            if let Ok(Value::Timer(timer)) = engine.named(&path, name)
                && let Some(origin) = &timer.origin
                && seen_timers.insert(origin.clone())
            {
                for action in timer.actions() {
                    let name = &ws.named(origin).name;
                    let title = format!(
                        "{}{} timer '{name}'",
                        action[..1].to_uppercase(),
                        &action[1..]
                    );
                    result.push(CodeActionOrCommand::Command(Command {
                        title,
                        command: "jot.timer".into(),
                        arguments: Some(vec![
                            serde_json::json!(Url::from_file_path(&origin.path).unwrap()),
                            serde_json::json!(name),
                            serde_json::json!(action),
                        ]),
                    }));
                }
            }
        }
        if let Some((i, task)) = doc
            .tasks
            .iter()
            .enumerate()
            .find(|(_, t)| t.line == params.range.start.line as usize)
        {
            let title = if task.attributes.contains_key("every") {
                "Complete occurrence and schedule next"
            } else if Engine::new(ws, today).task_done(&path, i) {
                "Reopen task"
            } else {
                "Complete task"
            };
            match actions::toggle_task(ws, &path, i, today) {
                Ok(edits) => result.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: title.into(),
                    kind: Some(CodeActionKind::REFACTOR_REWRITE),
                    edit: Some(edit_for(&state, &path, edits)),
                    ..Default::default()
                })),
                Err(reason) => result.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: title.into(),
                    disabled: Some(CodeActionDisabled { reason }),
                    ..Default::default()
                })),
            }
        }
        let edits = actions::freeze_dates(ws, &path, today);
        if !edits.is_empty() {
            result.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Resolve relative dates to calendar dates".into(),
                kind: Some(CodeActionKind::REFACTOR_REWRITE),
                edit: Some(edit_for(&state, &path, edits)),
                ..Default::default()
            }));
        }
        result.push(CodeActionOrCommand::Command(Command {
            title: "Refresh GitHub resources".into(),
            command: "jot.refresh".into(),
            arguments: None,
        }));
        result.push(CodeActionOrCommand::Command(Command {
            title: "Show today's agenda".into(),
            command: "jot.today".into(),
            arguments: None,
        }));
        Ok(Some(result))
    }
    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        match params.command.as_str() {
            "jot.timer" => {
                let args = &params.arguments;
                let strings: Option<Vec<_>> = args.iter().map(|v| v.as_str()).collect();
                let strings = strings.filter(|v| v.len() == 3).ok_or_else(|| {
                    Error::invalid_params("Timer command expects URI, name, and action")
                })?;
                let uri =
                    Url::parse(strings[0]).map_err(|e| Error::invalid_params(e.to_string()))?;
                let path = file(&uri)?;
                // Reload closed notes; preserve open/unsaved buffers before making an edit.
                self.rescan().await;
                let edit = {
                    let state = self.state.read().await;
                    let (origin, edit) = timers::edit(
                        &state.workspace,
                        &path,
                        strings[1],
                        strings[2],
                        Local::now().fixed_offset(),
                    )
                    .map_err(Error::invalid_params)?;
                    edit_for(&state, &origin.path, vec![edit])
                };
                let response = self.client.apply_edit(edit).await?;
                if !response.applied {
                    return Err(Error::invalid_params(
                        response
                            .failure_reason
                            .unwrap_or_else(|| "Editor declined the timer edit".into()),
                    ));
                }
                // The client's didChange/didSave reports the applied edit and starts/stops refreshes.
            }
            "jot.refresh" => {
                let mut workspace = self.state.read().await.workspace.clone();
                let errors = crate::cli::refresh(&mut workspace).await;
                self.state.write().await.workspace.cache = workspace.cache;
                self.notify_changes().await;
                self.client
                    .show_message(
                        if errors.is_empty() {
                            MessageType::INFO
                        } else {
                            MessageType::WARNING
                        },
                        if errors.is_empty() {
                            "GitHub resource status refreshed".into()
                        } else {
                            errors.join("\n")
                        },
                    )
                    .await;
            }
            "jot.today" => {
                self.rescan().await;
                let today = Local::now().date_naive();
                let workspace = self.state.read().await.workspace.clone();
                let rows = crate::cli::entries(&workspace, today);
                let lines: Vec<_> = rows
                    .iter()
                    .filter(|e| crate::cli::agenda_entry(e, today, today))
                    .map(|e| {
                        format!(
                            "- [{}:{}](<{}>) — {}{}",
                            e.path.file_name().unwrap_or_default().to_string_lossy(),
                            e.line,
                            e.uri,
                            e.title,
                            if e.blocked_by.is_empty() {
                                String::new()
                            } else {
                                format!(" (blocked by {})", e.blocked_by.join(", "))
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
                let dir = workspace.root().join(".jot");
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
            _ => return Err(Error::invalid_params("Unknown Jot command")),
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
        let Some(doc) = ws.documents.get(&path) else {
            return Ok(None);
        };
        #[allow(deprecated)]
        let symbols = doc
            .sections
            .iter()
            .map(|section| DocumentSymbol {
                name: section.title.clone(),
                detail: None,
                kind: tower_lsp::lsp_types::SymbolKind::NAMESPACE,
                tags: None,
                deprecated: None,
                range: Range::new(
                    Position::new(section.line as u32, 0),
                    doc.line_end(section.end_line.saturating_sub(1)),
                ),
                selection_range: Range::new(
                    Position::new(section.line as u32, 0),
                    doc.line_end(section.line),
                ),
                children: None,
            })
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }
}
pub async fn serve() {
    let (service, socket) = LspService::new(Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
