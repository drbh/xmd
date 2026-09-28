use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use clap::{Args, Parser, Subcommand, ValueEnum};
use eval::Workspace;
use eval::engine::{Value, value_json};
use features::query::{self, display};
use host::WorkspaceFiles;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
};

/// A note file name as a literal for help text: `note!("trip")` is `trip.x.md`.
macro_rules! note {
    ($stem:literal) => {
        concat!($stem, ".", common::note_extension!())
    };
}

#[derive(Parser)]
#[command(
    name = "xmd",
    version,
    about = "Reactive notes with a typed workspace query API. The default command is a query: name a note or pipe one in.",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true,
    override_usage = "xmd [OPTIONS] <QUERY>                (note on stdin)\n       xmd [OPTIONS] <FILE> <QUERY>\n       xmd [OPTIONS] --workspace <QUERY>\n       xmd <COMMAND> ...",
    after_help = concat!("Examples: cat ", note!("note"), " | xmd 'total'\n          xmd ", note!("note"), " 'tasks | where !done' --json\n          xmd --workspace 'filter(tasks, fn(t) => !t.done)' --json\n          xmd lsp   # the language server, for editors\nRun `xmd query --help` for the query bindings, functions and stages.")
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub query: QueryOptions,
}
#[derive(Subcommand)]
pub(crate) enum Command {
    /// Start the language server over stdin/stdout (for editor integrations).
    Lsp,
    /// Query tasks, values, tables, plans, resources and diagnostics.
    #[command(
        alias = "q",
        override_usage = "xmd query [OPTIONS] <QUERY>                (note on stdin)\n       xmd query [OPTIONS] <FILE> <QUERY>\n       xmd query [OPTIONS] --workspace <QUERY>",
        after_help = concat!("Bindings: ast, graph, tasks, events, stops, entries, values, plans, tables, rows, resources, diagnostics, notes\nFunctions: map, filter, fold, get, sort_by, group_by, sum, length\nStages: where, select, sort, limit, count, sum, group\nUse - as QUERY to read an expression from stdin; the note then has to be a file.\nExamples: xmd query ", note!("note"), " 'map(tasks, fn(t) => t.title)' --json\n          cat ", note!("note"), " | xmd query 'length(tasks)'\n          xmd query --workspace 'filter(tasks, fn(t) => !t.done)' --json\n          printf 'length(tasks)' | xmd query ", note!("note"), " -")
    )]
    Query(QueryOptions),
    /// Export a saved note with the language server's colors and inline values.
    Render(RenderOptions),
    /// Inspect syntax nodes, source text and expression trees for one note (JSON).
    Ast(InspectOptions),
    /// Inspect dependencies between values, tasks and checklists in one note (JSON).
    Graph(InspectOptions),
    /// Explicitly refresh cached resource status and external lookups.
    Refresh {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Keep a directory of notes in step with a folder in the web app.
    Sync(crate::sync::SyncOptions),
}
#[derive(Args)]
pub(crate) struct QueryOptions {
    /// A note relative to --root, or the query itself when the note is piped in or --workspace is set.
    #[arg(value_name = "FILE_OR_QUERY")]
    pub input: Option<OsString>,
    /// A functional expression or collection pipeline; use - for stdin.
    #[arg(value_name = "QUERY", conflicts_with = "workspace")]
    pub source: Option<String>,
    /// Query all indexed notes instead of supplying a positional note file.
    #[arg(long)]
    pub workspace: bool,
    #[command(flatten)]
    pub output: QueryOutput,
}
#[derive(Args)]
pub(crate) struct InspectOptions {
    /// A saved note, relative to --root or an absolute path.
    pub file: PathBuf,
    /// A query over ast, graph or any other collection in this note.
    #[arg(short = 'q', long, value_name = "QUERY")]
    pub query: Option<String>,
    #[command(flatten)]
    pub output: QueryOutput,
}
#[derive(Args)]
pub(crate) struct QueryOutput {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Emit a JSON array, preserving dates, currencies and units.
    #[arg(long, conflicts_with = "jsonl")]
    pub json: bool,
    /// Emit one JSON value per line.
    #[arg(long)]
    pub jsonl: bool,
    /// Evaluate at local midnight on this date.
    #[arg(long, conflicts_with = "now")]
    pub on: Option<NaiveDate>,
    /// Freeze the clock and timezone offset using an RFC3339 timestamp.
    #[arg(long)]
    pub now: Option<DateTime<FixedOffset>>,
    /// Return exit status 1 if the query returns any rows (useful for diagnostics).
    #[arg(long)]
    pub fail_on_match: bool,
}
#[derive(Args)]
#[command(
    after_help = concat!("Example: xmd render ", note!("trip"), " --root notes --now 2026-09-18T12:00:00Z > trip.html\nReads saved notes, modules and cached data; never refreshes or edits them.")
)]
pub(crate) struct RenderOptions {
    /// A note, relative to --root or an absolute path.
    pub file: PathBuf,
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    /// Standalone styled HTML, or source with inline values as plain text.
    #[arg(long, value_enum, default_value_t = RenderFormat::Html)]
    pub format: RenderFormat,
    /// Evaluate at local midnight on this date.
    #[arg(long, conflicts_with = "now")]
    pub on: Option<NaiveDate>,
    /// Freeze the clock and timezone offset using an RFC3339 timestamp.
    #[arg(long)]
    pub now: Option<DateTime<FixedOffset>>,
}
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum RenderFormat {
    Html,
    Text,
}
pub(crate) async fn refresh(workspace: &mut Workspace) -> Vec<String> {
    let mut errors = refresh_in_memory(workspace, None).await;
    if let Err(e) = workspace.save_cache() {
        errors.push(e);
    }
    errors
}
/// The editor validates its registry snapshot before persisting resource results.
pub(crate) async fn refresh_in_memory(
    workspace: &mut Workspace,
    only: Option<&std::path::Path>,
) -> Vec<String> {
    let targets: BTreeSet<_> = workspace
        .documents
        .iter()
        .filter(|(path, _)| only.is_none_or(|only| *path == only))
        .flat_map(|(_, doc)| {
            doc.definitions
                .iter()
                .filter(|d| !d.expression)
                .map(|d| d.source.as_str())
                .chain(doc.links.iter().map(|l| l.target.as_str()))
        })
        .filter(|s| workspace.link_features().refresh_request(s).is_some())
        .map(str::to_owned)
        .collect();
    let mut errors = Vec::new();
    for target in targets {
        match host::fetch_link(workspace.link_features(), &target).await {
            Ok(metadata) => {
                workspace.cache.insert(target, metadata);
            }
            Err(e) => errors.push(format!("{target}: {e}")),
        }
    }
    errors.extend(host::lookups::refresh(workspace, crate::editor::now(), only).await);
    errors
}
fn load(root: PathBuf) -> Result<Workspace, String> {
    Workspace::load(vec![
        std::fs::canonicalize(root).map_err(|e| e.to_string())?,
    ])
}
pub(crate) async fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Lsp => unreachable!(),
        Command::Query(options) => query_command(options),
        Command::Render(options) => render_command(options),
        Command::Ast(options) => inspect_command("ast", options),
        Command::Graph(options) => inspect_command("graph", options),
        Command::Sync(options) => crate::sync::run(options),
        Command::Refresh { root } => {
            let mut workspace = load(root)?;
            let errors = refresh(&mut workspace).await;
            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }
            println!(
                "Resource cache updated ({} resources)",
                workspace.cache.len()
            );
            Ok(())
        }
    }
}

fn inspect_command(view: &str, mut options: InspectOptions) -> Result<(), String> {
    options.output.json = !options.output.jsonl;
    run_query(
        options.query.unwrap_or_else(|| view.into()),
        Some(Note::File(options.file)),
        options.output,
    )
}
/// The note a query runs against: one saved file, or the text piped to stdin.
pub(crate) enum Note {
    File(PathBuf),
    Piped(String),
}
/// Notes read from stdin are capped at the 1 MB the browser host allows.
const MAX_PIPED_NOTE: usize = 1_000_000;
/// The synthetic path a piped note is filed under, inside `--root` so its
/// relative imports and `.xmd/modules.json` resolve like a saved note's.
pub(crate) const PIPED_NOTE_NAME: &str = note!("<stdin>");

pub(crate) fn query_command(options: QueryOptions) -> Result<(), String> {
    let input = options.input.ok_or(concat!(
        "Supply a query expression: xmd 'total' < ",
        note!("note")
    ))?;
    let (note, mut source) = if options.workspace {
        (None, into_query(input)?)
    } else if let Some(source) = options.source {
        (Some(Note::File(PathBuf::from(input))), source)
    } else if io::stdin().is_terminal() {
        return Err(concat!(
            "Supply a note file, or pipe one in: cat ",
            note!("note"),
            " | xmd 'total'"
        )
        .into());
    } else {
        (Some(Note::Piped(read_piped_note()?)), into_query(input)?)
    };
    if source == "-" {
        if matches!(note, Some(Note::Piped(_))) {
            return Err("Stdin is already the note; pass the query as an argument".into());
        }
        source.clear();
        io::stdin()
            .take(65_537)
            .read_to_string(&mut source)
            .map_err(|e| e.to_string())?;
    }
    run_query(source, note, options.output)
}
fn into_query(input: OsString) -> Result<String, String> {
    input
        .into_string()
        .map_err(|_| "Query must be UTF-8".into())
}
fn read_piped_note() -> Result<String, String> {
    let mut text = String::new();
    io::stdin()
        .take(MAX_PIPED_NOTE as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("Reading the note from stdin: {e}"))?;
    if text.len() > MAX_PIPED_NOTE {
        return Err("Notes from stdin are limited to 1 MB".into());
    }
    Ok(text)
}
fn run_query(source: String, note: Option<Note>, options: QueryOutput) -> Result<(), String> {
    let compiled = query::Query::parse(&source)?;
    let now = request_time(options.on, options.now)?;
    let root = std::fs::canonicalize(options.root).map_err(|e| e.to_string())?;
    let (only, mut workspace) = match note {
        Some(Note::File(file)) => {
            let path = root.join(file);
            let path =
                std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let workspace = Workspace::load_file(vec![root], &path)?;
            (Some(path), workspace)
        }
        Some(Note::Piped(text)) => {
            let path = root.join(PIPED_NOTE_NAME);
            let workspace = Workspace::load_source(vec![root], &path, text)?;
            (Some(path), workspace)
        }
        None => (None, Workspace::load(vec![root])?),
    };
    compiled.load_imports(&mut workspace, only.as_deref());
    let result = features::Request::new(&workspace, now).query(&compiled, only.as_deref())?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    let write_result = (|| -> io::Result<()> {
        if options.json {
            serde_json::to_writer_pretty(&mut output, &result.json())?;
            writeln!(output)?;
        } else {
            for row in &result.rows {
                if options.jsonl {
                    serde_json::to_writer(&mut output, &value_json(row))?;
                    writeln!(output)?;
                } else {
                    writeln!(output, "{}", render(row))?;
                }
            }
        }
        output.flush()
    })();
    if let Err(e) = write_result {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.to_string());
    }
    if options.fail_on_match && !result.rows.is_empty() {
        return Err(format!("{} matching result(s)", result.rows.len()));
    }
    Ok(())
}
fn request_time(
    on: Option<NaiveDate>,
    now: Option<DateTime<FixedOffset>>,
) -> Result<DateTime<FixedOffset>, String> {
    if let Some(now) = now {
        Ok(now)
    } else if let Some(date) = on {
        Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .map(|t| t.fixed_offset())
            .ok_or(
                "Local midnight is ambiguous or nonexistent; use --now with an explicit offset"
                    .into(),
            )
    } else {
        // `XMD_NOW` freezes the clock; see the `editor` module.
        Ok(crate::editor::now())
    }
}
fn render_command(options: RenderOptions) -> Result<(), String> {
    let now = request_time(options.on, options.now)?;
    let root = std::fs::canonicalize(options.root).map_err(|e| e.to_string())?;
    let path = root.join(options.file);
    let path = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let workspace = Workspace::load_file(vec![root], &path)?;
    let request = features::Request::new(&workspace, now);
    let text = match options.format {
        RenderFormat::Html => request.render_html(&path)?,
        RenderFormat::Text => request.render_text(&path)?,
    };
    let diagnostics = request.diagnostics(&path, false);
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if let Err(e) = output
        .write_all(text.as_bytes())
        .and_then(|_| output.flush())
    {
        if e.kind() == io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.to_string());
    }
    let mut errors = 0;
    for diagnostic in diagnostics {
        let severity = features::diagnostics::severity_name(diagnostic.severity);
        if severity == "error" {
            errors += 1;
        }
        eprintln!(
            "{}:{}:{}: {severity}: {}",
            path.display(),
            diagnostic.range.start.line + 1,
            diagnostic.range.start.character + 1,
            diagnostic.message
        );
    }
    if errors > 0 {
        return Err(format!("{errors} error(s) while rendering"));
    }
    Ok(())
}
fn render(value: &Value) -> String {
    let Value::Record(fields) = value else {
        return display(value);
    };
    fields
        .iter()
        .map(|(name, value)| {
            let text = if name == "source"
                && let Value::Record(source) = value
                && let Some(Value::Text(path)) = source.get("path")
                && let Some(line) = source.get("line")
            {
                format!("{path}:{}", display(line))
            } else {
                display(value)
            };
            format!("{name}={text}")
        })
        .collect::<Vec<_>>()
        .join("\t")
}
