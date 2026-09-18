use crate::{
    query::{self, QueryValue},
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{self, Read, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    name = "wtf",
    version,
    about = "Reactive notes with a typed workspace query API. No arguments starts the language server."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    /// Start the language server over stdin/stdout (also the default).
    Lsp,
    /// Query tasks, values, tables, plans, resources and diagnostics.
    #[command(alias = "q")]
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
}
#[derive(Args)]
#[command(
    override_usage = "wtf query [OPTIONS] <FILE> <QUERY>\n       wtf query [OPTIONS] --workspace <QUERY>",
    after_help = "Bindings: ast, graph, tasks, events, stops, entries, values, plans, tables, rows, resources, diagnostics, notes\nFunctions: map, filter, fold, get, sort_by, group_by, sum, length\nStages: where, select, sort, limit, count, sum, group\nUse - as QUERY to read an expression from stdin.\nExamples: wtf query note.wtf 'map(tasks, fn(t) => t.title)' --json\n          wtf query --workspace 'filter(tasks, fn(t) => !t.done)' --json\n          printf 'length(tasks)' | wtf query note.wtf -"
)]
pub struct QueryOptions {
    /// A note relative to --root, or the query expression when --workspace is set.
    #[arg(value_name = "FILE_OR_QUERY")]
    pub input: OsString,
    /// A functional expression or collection pipeline; use - for stdin.
    #[arg(
        value_name = "QUERY",
        required_unless_present = "workspace",
        conflicts_with = "workspace"
    )]
    pub source: Option<String>,
    /// Query all indexed notes instead of supplying a positional note file.
    #[arg(long)]
    pub workspace: bool,
    #[command(flatten)]
    pub output: QueryOutput,
}
#[derive(Args)]
pub struct InspectOptions {
    /// A saved .wtf note, relative to --root or an absolute path.
    pub file: PathBuf,
    /// A query over ast, graph or any other collection in this note.
    #[arg(short = 'q', long, value_name = "QUERY")]
    pub query: Option<String>,
    #[command(flatten)]
    pub output: QueryOutput,
}
#[derive(Args)]
pub struct QueryOutput {
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
    after_help = "Example: wtf render trip.wtf --root notes --now 2026-09-18T12:00:00Z > trip.html\nReads saved notes, modules and cached data; never refreshes or edits them."
)]
pub struct RenderOptions {
    /// A .wtf note, relative to --root or an absolute path.
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
pub enum RenderFormat {
    Html,
    Text,
}
pub async fn refresh(workspace: &mut Workspace) -> Vec<String> {
    let mut errors = refresh_in_memory(workspace).await;
    if let Err(e) = workspace.save_cache() {
        errors.push(e);
    }
    errors
}
/// The editor validates its registry snapshot before persisting resource results.
pub(crate) async fn refresh_in_memory(workspace: &mut Workspace) -> Vec<String> {
    let targets: BTreeSet<_> = workspace
        .documents
        .values()
        .flat_map(|doc| {
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
        match workspace.link_features().fetch(&target).await {
            Ok(metadata) => {
                workspace.cache.insert(target, metadata);
            }
            Err(e) => errors.push(format!("{target}: {e}")),
        }
    }
    errors.extend(crate::lookups::native::refresh(workspace).await);
    errors
}
fn load(root: PathBuf) -> Result<Workspace, String> {
    Workspace::load(vec![
        std::fs::canonicalize(root).map_err(|e| e.to_string())?,
    ])
}
pub async fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Lsp => unreachable!(),
        Command::Query(options) => query_command(options),
        Command::Render(options) => render_command(options),
        Command::Ast(options) => inspect_command("ast", options),
        Command::Graph(options) => inspect_command("graph", options),
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
        Some(options.file),
        options.output,
    )
}
fn query_command(options: QueryOptions) -> Result<(), String> {
    let (within, mut source) = if options.workspace {
        (
            None,
            options
                .input
                .into_string()
                .map_err(|_| "Query must be UTF-8")?,
        )
    } else {
        (
            Some(PathBuf::from(options.input)),
            options.source.ok_or("Supply a query expression")?,
        )
    };
    if source == "-" {
        source.clear();
        io::stdin()
            .take(65_537)
            .read_to_string(&mut source)
            .map_err(|e| e.to_string())?;
    }
    run_query(source, within, options.output)
}
fn run_query(source: String, within: Option<PathBuf>, options: QueryOutput) -> Result<(), String> {
    let compiled = query::Query::parse(&source)?;
    let now = request_time(options.on, options.now)?;
    let root = std::fs::canonicalize(options.root).map_err(|e| e.to_string())?;
    let only = within
        .map(|file| {
            let path = root.join(file);
            std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))
        })
        .transpose()?;
    let mut workspace = match &only {
        Some(path) => Workspace::load_file(vec![root], path)?,
        None => Workspace::load(vec![root])?,
    };
    compiled.load_imports(&mut workspace, only.as_deref());
    let result = crate::RequestContext::new(&workspace, now).query(&compiled, only.as_deref())?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    let write_result = (|| -> io::Result<()> {
        if options.json {
            serde_json::to_writer_pretty(&mut output, &result.rows)?;
            writeln!(output)?;
        } else {
            for row in &result.rows {
                if options.jsonl {
                    serde_json::to_writer(&mut output, row)?;
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
        // `WTF_NOW` freezes the clock; see the `editor` module.
        Ok(crate::editor::now())
    }
}
fn render_command(options: RenderOptions) -> Result<(), String> {
    let now = request_time(options.on, options.now)?;
    let root = std::fs::canonicalize(options.root).map_err(|e| e.to_string())?;
    let path = root.join(options.file);
    let path = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let workspace = Workspace::load_file(vec![root], &path)?;
    let request = crate::RequestContext::new(&workspace, now);
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
        let severity = crate::diagnostics::severity_name(diagnostic.severity);
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
fn render(value: &QueryValue) -> String {
    let QueryValue::Object(fields) = value else {
        return value.display();
    };
    fields
        .iter()
        .map(|(name, value)| {
            let text = if name == "source"
                && let QueryValue::Object(source) = value
                && let Some(QueryValue::Scalar(crate::engine::Value::Text(path))) =
                    source.get("path")
                && let Some(line) = source.get("line")
            {
                format!("{path}:{}", line.display())
            } else {
                value.display()
            };
            format!("{name}={text}")
        })
        .collect::<Vec<_>>()
        .join("\t")
}
