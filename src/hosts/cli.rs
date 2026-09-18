use crate::{
    actions,
    document::Document,
    engine::Engine,
    query::{self, QueryContext, QueryValue},
    resources,
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use clap::{Args, Parser, Subcommand};
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
    /// Append a task to inbox.wtf, or to journal/YYYY-MM-DD.wtf.
    Capture {
        #[arg(required=true,num_args=1..)]
        text: Vec<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        journal: bool,
        #[arg(long)]
        on: Option<NaiveDate>,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Toggle a task at file.wtf:LINE; recurring tasks advance.
    Complete {
        target: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        on: Option<NaiveDate>,
    },
    /// Explicitly refresh cached GitHub status and external lookups.
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
    after_help = "Collections: tasks, events, stops, entries, values, plans, tables, rows, resources, diagnostics, notes\nStages: where, select, sort, limit, count, sum, group\nSaved views: @today, @week, @tasks, @check\nExample: wtf query 'tasks | where leaf && !done | select {title, due, source}'"
)]
pub struct QueryOptions {
    /// A collection pipeline or a saved view such as @today.
    #[arg(
        value_name = "QUERY",
        required_unless_present = "file",
        conflicts_with = "file"
    )]
    pub source: Option<String>,
    /// Read a query file; use - for stdin.
    #[arg(short = 'f', long)]
    pub file: Option<PathBuf>,
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
pub async fn refresh(workspace: &mut Workspace) -> Vec<String> {
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
        .filter(|s| resources::github(s).is_some())
        .map(str::to_owned)
        .collect();
    let mut errors = Vec::new();
    for target in targets {
        match resources::fetch(&target).await {
            Ok(metadata) => {
                workspace.cache.insert(target, metadata);
            }
            Err(e) => errors.push(format!("{target}: {e}")),
        }
    }
    if let Err(e) = workspace.save_cache() {
        errors.push(e);
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
        Command::Capture {
            text,
            root,
            file,
            journal,
            on,
            due,
            tag,
        } => {
            let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
            let today = on.unwrap_or_else(|| Local::now().date_naive());
            if file.is_some() && journal {
                return Err("Choose --file or --journal".into());
            }
            let path = root.join(file.unwrap_or_else(|| {
                if journal {
                    PathBuf::from(format!("journal/{today}.wtf"))
                } else {
                    PathBuf::from("inbox.wtf")
                }
            }));
            let mut title = text.join(" ");
            if title.contains(['\n', '\r']) {
                return Err("Capture expects a single task line".into());
            }
            if let Some(due) = due {
                if due.contains(['\n', '\r']) {
                    return Err("Invalid due date".into());
                }
                title.push_str(&format!(" @due({due})"));
            }
            if let Some(tag) = tag {
                if !crate::document::identifier(&tag) {
                    return Err("Tags must be identifiers".into());
                }
                title.push_str(&format!(" #{tag}"));
            }
            let mut line = format!("- [ ] {}\n", title.trim_start_matches("- [ ] "));
            let previous = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(e.to_string()),
            };
            let mut next = previous.clone();
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            let row = next.lines().count() + 1;
            if previous.contains("\r\n") {
                line = line.replace('\n', "\r\n");
            }
            next.push_str(&line);
            let mut workspace = load(root)?;
            workspace
                .documents
                .insert(path.clone(), Document::parse(next.clone()));
            let edits: Vec<_> = actions::freeze_dates(&workspace, &path, today)
                .into_iter()
                .filter(|e| e.range.start.line as usize >= row - 1)
                .collect();
            next = actions::apply_edits(&next, &edits)?;
            workspace
                .documents
                .insert(path.clone(), Document::parse(next.clone()));
            let issues = crate::editor::problems(&workspace, &path, today);
            if let Some(issue) = issues.iter().find(|p| p.span.line >= row - 1) {
                return Err(issue.message.clone());
            }
            write_note(&path, &previous, &next)?;
            println!("{}:{row}", path.display());
            Ok(())
        }
        Command::Complete { target, root, on } => {
            let workspace = load(root)?;
            let (file, row) = target.rsplit_once(':').ok_or("Use file.wtf:LINE")?;
            let row: usize = row.parse().map_err(|_| "Invalid line number")?;
            let path =
                std::fs::canonicalize(workspace.root().join(file)).map_err(|e| e.to_string())?;
            let doc = workspace
                .documents
                .get(&path)
                .ok_or("File is not in this workspace's indexed .wtf notes")?;
            let i = doc
                .tasks
                .iter()
                .position(|t| t.line + 1 == row)
                .ok_or("No task on that line")?;
            let edits = actions::toggle_task(
                &workspace,
                &path,
                i,
                on.unwrap_or_else(|| Local::now().date_naive()),
            )?;
            let next = actions::apply_edits(&doc.text, &edits)?;
            write_note(&path, &doc.text, &next)?;
            println!("Updated {}:{row}", path.display());
            Ok(())
        }
        Command::Refresh { root } => {
            let mut workspace = load(root)?;
            let errors = refresh(&mut workspace).await;
            if !errors.is_empty() {
                return Err(errors.join("\n"));
            }
            println!("GitHub cache updated ({} resources)", workspace.cache.len());
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
    let now = if let Some(now) = options.now {
        now
    } else if let Some(date) = options.on {
        Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .ok_or("Local midnight is ambiguous or nonexistent; use --now with an explicit offset")?
            .fixed_offset()
    } else {
        Local::now().fixed_offset()
    };
    let workspace = load(options.root)?;
    let result = query::execute(&workspace, &compiled, &QueryContext::new(now))?;
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
fn write_note(path: &Path, previous: &str, next: &str) -> Result<(), String> {
    let actual = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    if actual != previous {
        return Err("Note changed while the command was running; retry".into());
    }
    let parent = path.parent().ok_or("Invalid note path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = parent.join(format!(".wtf-write-{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(next.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    if let Ok(metadata) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, metadata.permissions()).map_err(|e| e.to_string())?;
    }
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}
