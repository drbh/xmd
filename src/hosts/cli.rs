use crate::{
    engine::Engine,
    query::{self, QueryValue},
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeSet,
    io::{self, Read, Write},
    path::{Path, PathBuf},
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
    /// Convert between WTF plans and alps problem files, writing to stdout.
    Convert {
        #[arg(long, required_unless_present = "to_alps", conflicts_with = "to_alps")]
        from_alps: Option<PathBuf>,
        #[arg(long, required_unless_present = "from_alps")]
        to_alps: Option<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}
#[derive(Args)]
#[command(
    after_help = "Bindings: ast, graph, tasks, events, stops, entries, values, plans, tables, rows, resources, diagnostics, notes\nFunctions: map, filter, fold, get, sort_by, group_by, sum, length\nStages: where, select, sort, limit, count, sum, group\nSaved views: @today, @week, @tasks, @check\nExamples: wtf query 'map(tasks, fn(t) => t.title)' --in note.wtf --json\n          wtf ast note.wtf --query 'filter(ast, fn(n) => n.kind == \"definition\")'"
)]
pub struct QueryOptions {
    /// A functional expression, collection pipeline or saved view such as @today.
    #[arg(
        value_name = "QUERY",
        required_unless_present = "file",
        conflicts_with = "file"
    )]
    pub source: Option<String>,
    /// Read a query file; use - for stdin.
    #[arg(short = 'f', long)]
    pub file: Option<PathBuf>,
    /// Read input records from this note; paths are relative to --root.
    #[arg(long = "in", value_name = "NOTE")]
    pub within: Option<PathBuf>,
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
    after_help = "Example: wtf render trip.wtf --root notes --now 2026-09-18T12:00:00Z > trip.html\nReads saved notes, plugins and cached data; never refreshes or edits them."
)]
pub struct RenderOptions {
    /// A .wtf note, relative to --root or an absolute path inside the workspace.
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
        Command::Convert {
            from_alps,
            to_alps,
            root,
        } => {
            if let Some(file) = from_alps {
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                let problem: serde_json::Value =
                    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("plan")
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
                print!("{}", crate::plans::import(&stem, &problem)?);
                return Ok(());
            }
            let name = to_alps.ok_or("Choose --from-alps FILE or --to-alps NAME")?;
            let workspace = load(root)?;
            let symbol = workspace.resolve(workspace.root(), &name)?;
            let (_, plan) = crate::plans::plan(&workspace, &symbol)
                .ok_or_else(|| format!("'{name}' is not a plan"))?;
            let mut engine = Engine::new(&workspace, Local::now().date_naive());
            let problem = crate::plans::export(&mut engine, &symbol, plan)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&problem).map_err(|e| e.to_string())?
            );
            Ok(())
        }
    }
}
fn inspect_command(view: &str, mut options: InspectOptions) -> Result<(), String> {
    options.output.json = !options.output.jsonl;
    query_command(QueryOptions {
        source: Some(options.query.unwrap_or_else(|| view.into())),
        file: None,
        within: Some(options.file),
        output: options.output,
    })
}
fn query_command(options: QueryOptions) -> Result<(), String> {
    let source = match (options.source, options.file) {
        (Some(source), None) => source,
        (None, Some(path)) if path == Path::new("-") => {
            let mut source = String::new();
            io::stdin()
                .take(65_537)
                .read_to_string(&mut source)
                .map_err(|e| e.to_string())?;
            source
        }
        (None, Some(path)) => {
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
        }
        _ => return Err("Supply a query or --file PATH".into()),
    };
    let compiled = query::Query::parse(&source)?;
    let now = request_time(options.output.on, options.output.now)?;
    let workspace = load(options.output.root.clone())?;
    let only = options
        .within
        .map(|file| {
            let path = workspace.root().join(file);
            std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))
        })
        .transpose()?;
    let result = query::execute_scoped_in(
        &crate::RequestContext::new(&workspace, now),
        &compiled,
        only.as_deref(),
    )?;
    let options = options.output;
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
        Ok(Local::now().fixed_offset())
    }
}
fn render_command(options: RenderOptions) -> Result<(), String> {
    let now = request_time(options.on, options.now)?;
    let workspace = load(options.root)?;
    let path = workspace.root().join(options.file);
    let path = std::fs::canonicalize(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let request = crate::RequestContext::new(&workspace, now);
    let text = match options.format {
        RenderFormat::Html => crate::rendering::html_in(&request, &path)?,
        RenderFormat::Text => crate::presentation::render_text_in(&request, &path)?,
    };
    let diagnostics = crate::diagnostics::collect_in(&request, &path, false);
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
        let severity = match diagnostic.severity {
            Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
            Some(lsp_types::DiagnosticSeverity::INFORMATION) => "info",
            Some(lsp_types::DiagnosticSeverity::HINT) => "hint",
            _ => {
                errors += 1;
                "error"
            }
        };
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
