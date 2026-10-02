use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use clap::{Args, Parser, Subcommand, ValueEnum};
use lang::eval::Workspace;
use lang::eval::engine::value_json;
use native::WorkspaceFiles;
use services::{Query, Request, display_row, severity_name};
use std::{
    ffi::OsString,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
};

/// A note file name as a literal for help text: `note!("trip")` is `trip.x.md`.
macro_rules! note {
    ($stem:literal) => {
        concat!($stem, ".", lang::common::note_extension!())
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
    after_help = concat!("Examples: cat ", note!("note"), " | xmd 'total'\n          xmd ", note!("note"), " 'tasks | filter(fn(t) => !t.done) | map(.{title, due})' --json\n          xmd --workspace 'filter(tasks, fn(t) => !t.done)' --json\n          xmd lsp   # the language server, for editors\nRun `xmd query --help` for the query bindings and functions.")
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
        after_help = concat!("Bindings: ast, graph, days, timers, mentions, links, tasks, checkboxes, events, stops, entries, values, forms, plans, decisions, tables, rows, resources, diagnostics, notes, sections, calculations, references, cells, recognized, attributed\nFunctions: map, filter, fold, get, sort_by, desc, group_by, slice, concat, sum, length\nxs | f(a) is f(xs, a); .due is fn(x) => x.due; .{title, due} picks fields.\nUse - as QUERY to read an expression from stdin; the note then has to be a file.\nExamples: xmd query ", note!("note"), " 'tasks | sort_by(desc(.due)) | map(.title)' --json\n          cat ", note!("note"), " | xmd query 'tasks | length'\n          xmd query --workspace 'filter(tasks, fn(t) => !t.done)' --json\n          printf 'length(tasks)' | xmd query ", note!("note"), " -")
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
    /// Run a command module: its id among --root's activated modules, or a path to its file.
    #[command(
        after_help = concat!("The command works inside DIRECTORY (default .) and can only read and write there.\nExamples: xmd run sync ./notes --url https://xmd.example.com\n          xmd run sync ./notes --root .")
    )]
    Run {
        /// The command module's id, or a path to its file.
        module: String,
        /// The directory the command works in, then its own arguments and --flags.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "DIRECTORY AND ARGS"
        )]
        args: Vec<String>,
        /// Where to find activated modules when MODULE is an id.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}
#[derive(Args)]
pub(crate) struct QueryOptions {
    /// A note relative to --root, or the query itself when the note is piped in or --workspace is set.
    #[arg(value_name = "FILE_OR_QUERY")]
    pub input: Option<OsString>,
    /// An expression over the workspace's collections; use - for stdin.
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
pub(crate) async fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Lsp => unreachable!(),
        Command::Query(options) => query_command(options),
        Command::Render(options) => render_command(options),
        Command::Ast(options) => inspect_command("ast", options),
        Command::Graph(options) => inspect_command("graph", options),
        Command::Run { module, args, root } => {
            native::run_command_named(&module, &args, &root, |line| println!("{line}"))
        }
        Command::Refresh { root } => {
            let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
            let mut workspace = Workspace::load(vec![root])?;
            let mut errors = native::refresh_workspace(&mut workspace, native::now(), None).await;
            errors.extend(native::save_cache(workspace.root(), workspace.cache()).err());
            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }
            println!(
                "Resource cache updated ({} resources)",
                workspace.cache().len()
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
    let compiled = Query::parse(&source)?;
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
    compiled.load_imports(&mut workspace, only.as_deref(), &native::DiskFiles);
    let result = Request::new(&workspace, now).query(&compiled, only.as_deref())?;
    let mut output = io::BufWriter::new(io::stdout().lock());
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
                    writeln!(output, "{}", display_row(row))?;
                }
            }
        }
        output.flush()
    })();
    if stdout_closed(write_result)? {
        return Ok(());
    }
    if options.fail_on_match && !result.rows.is_empty() {
        return Err(format!("{} matching result(s)", result.rows.len()));
    }
    Ok(())
}
/// Whether writing the output failed because whoever read it has stopped
/// reading, which ends a command quietly.
fn stdout_closed(written: io::Result<()>) -> Result<bool, String> {
    match written {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(true),
        Err(e) => Err(e.to_string()),
        Ok(()) => Ok(false),
    }
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
        // `XMD_NOW` freezes the clock; see `native::now`.
        Ok(native::now())
    }
}
fn render_command(options: RenderOptions) -> Result<(), String> {
    let now = request_time(options.on, options.now)?;
    let root = std::fs::canonicalize(options.root).map_err(|e| e.to_string())?;
    let path = root.join(options.file);
    let path = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let workspace = Workspace::load_file(vec![root], &path)?;
    let request = Request::new(&workspace, now);
    let text = match options.format {
        RenderFormat::Html => request.render_html(&path)?,
        RenderFormat::Text => request.render_text(&path)?,
    };
    let diagnostics = request.diagnostics(&path, false);
    let mut output = io::stdout().lock();
    if stdout_closed(
        output
            .write_all(text.as_bytes())
            .and_then(|_| output.flush()),
    )? {
        return Ok(());
    }
    let mut errors = 0;
    for diagnostic in diagnostics {
        let severity = severity_name(diagnostic.severity);
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
