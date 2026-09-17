use crate::{
    actions,
    document::Document,
    engine::{Engine, Value},
    resources,
    workspace::Workspace,
};
use chrono::{Duration, Local, NaiveDate};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "jot",
    version,
    about = "Reactive notes, checklists, resources, and a daily agenda. No arguments starts the language server."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand)]
pub enum Command {
    /// Start the language server over stdin/stdout (also the default).
    Lsp,
    /// Today's tasks and appointments, including overdue and undated tasks.
    Today(Query),
    /// Show the agenda (use --week for seven days).
    Agenda {
        #[command(flatten)]
        query: Query,
        #[arg(long)]
        week: bool,
    },
    /// List tasks across all .jot notes.
    Tasks {
        #[command(flatten)]
        query: Query,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Append a task to inbox.jot, or to journal/YYYY-MM-DD.jot.
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
    /// Toggle a task at file.jot:LINE (one-based); recurring tasks advance.
    Complete {
        target: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        on: Option<NaiveDate>,
    },
    /// Refresh cached GitHub status using your installed GitHub CLI (gh).
    Refresh {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Solve a linear plan, or exchange it with an alps problem file.
    Plan {
        /// The plan's name; omit with --import.
        name: Option<String>,
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Print the solution as JSON.
        #[arg(long)]
        json: bool,
        /// Print the plan as an alps problem file, with note values substituted.
        #[arg(long)]
        export: bool,
        /// Print Jot source for an alps problem file, named after the file.
        #[arg(long)]
        import: Option<PathBuf>,
    },
    /// Print syntax/evaluation problems, returning nonzero when any exist.
    Check {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}
#[derive(Args)]
pub struct Query {
    #[arg(long, default_value = ".")]
    pub root: PathBuf,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub on: Option<NaiveDate>,
}
#[derive(Debug, Serialize)]
pub struct Entry {
    pub path: PathBuf,
    pub line: usize,
    pub uri: String,
    pub title: String,
    pub kind: String,
    pub done: bool,
    pub due: Option<NaiveDate>,
    pub scheduled: Option<NaiveDate>,
    pub at: Option<String>,
    pub tags: Vec<String>,
    pub blocked_by: Vec<String>,
    pub estimate_minutes: Option<f64>,
    pub estimate_seconds: Option<i64>,
    pub errors: Vec<String>,
    #[serde(skip)]
    pub at_date: Option<NaiveDate>,
}
pub fn entries(workspace: &Workspace, today: NaiveDate) -> Vec<Entry> {
    let mut engine = Engine::new(workspace, today);
    let mut entries = Vec::new();
    for (path, doc) in &workspace.documents {
        for (i, task) in doc.tasks.iter().enumerate() {
            if doc.tasks.iter().any(|t| t.parent == Some(i)) {
                continue;
            }
            let mut errors = Vec::new();
            let mut when = |key: &str| {
                task.attributes
                    .get(key)
                    .and_then(|a| match engine.when(path, &a.value) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            errors.push(format!("@{key}: {e}"));
                            None
                        }
                    })
            };
            let due = when("due")
                .and_then(|v| v.date().ok())
                .or_else(|| task.attributes.contains_key("every").then_some(today));
            let scheduled = when("scheduled").and_then(|v| v.date().ok());
            let at = when("at");
            let estimate =
                task.attributes
                    .get("estimate")
                    .and_then(|a| match engine.eval(path, &a.value) {
                        Ok(Value::Duration(m)) if m >= 0 => Some(m),
                        _ => {
                            errors.push("@estimate requires a positive duration".into());
                            None
                        }
                    });
            let blocked_by = match engine.blocked(path, i) {
                Ok(b) => b,
                Err(e) => {
                    errors.push(e);
                    Vec::new()
                }
            };
            entries.push(Entry {
                path: path.clone(),
                line: task.line + 1,
                uri: location_uri(path, task.line + 1),
                title: task.title.clone(),
                kind: "task".into(),
                done: engine.task_done(path, i),
                due,
                scheduled,
                at: at.as_ref().map(Value::display),
                at_date: at.and_then(|v| v.date().ok()),
                tags: task.tags.clone(),
                blocked_by,
                estimate_minutes: estimate.map(|s| s as f64 / 60.0),
                estimate_seconds: estimate,
                errors,
            });
        }
        let dates = crate::itinerary::dates(&doc.days, today);
        for (day, date) in doc.days.iter().zip(&dates) {
            for stop in &day.stops {
                entries.push(Entry {
                    path: path.clone(),
                    line: stop.line + 1,
                    uri: location_uri(path, stop.line + 1),
                    title: crate::itinerary::label(stop),
                    kind: "stop".into(),
                    done: false,
                    due: None,
                    scheduled: None,
                    at: Some(match date {
                        Some(d) => format!("{d} {}", stop.time.format("%H:%M")),
                        None => stop.time.format("%H:%M").to_string(),
                    }),
                    at_date: *date,
                    tags: Vec::new(),
                    blocked_by: Vec::new(),
                    estimate_minutes: None,
                    estimate_seconds: None,
                    errors: Vec::new(),
                });
            }
        }
        for event in &doc.events {
            let result = engine.when(path, &event.attributes["at"].value);
            let mut errors = Vec::new();
            let at = match result {
                Ok(v) => Some(v),
                Err(e) => {
                    errors.push(e);
                    None
                }
            };
            entries.push(Entry {
                path: path.clone(),
                line: event.line + 1,
                uri: location_uri(path, event.line + 1),
                title: event.title.clone(),
                kind: "event".into(),
                done: false,
                due: None,
                scheduled: None,
                at: at.as_ref().map(Value::display),
                at_date: at.and_then(|v| v.date().ok()),
                tags: Vec::new(),
                blocked_by: Vec::new(),
                estimate_minutes: None,
                estimate_seconds: None,
                errors,
            });
        }
    }
    entries.sort_by_key(|e| {
        (
            e.due.or(e.at_date).or(e.scheduled).unwrap_or(today),
            e.at.clone(),
            e.path.clone(),
            e.line,
        )
    });
    entries
}
fn location_uri(path: &Path, line: usize) -> String {
    let mut url = tower_lsp::lsp_types::Url::from_file_path(path).unwrap();
    url.set_fragment(Some(&format!("L{line}")));
    url.into()
}
pub fn agenda_entry(e: &Entry, today: NaiveDate, end: NaiveDate) -> bool {
    if e.done {
        return false;
    }
    if e.kind == "event" || e.kind == "stop" {
        return e.at_date.is_some_and(|d| d >= today && d <= end) || !e.errors.is_empty();
    }
    if e.due.is_some_and(|d| d <= end)
        || e.scheduled.is_some_and(|d| d <= end)
        || e.at_date.is_some_and(|d| d <= end)
    {
        return true;
    }
    e.due.is_none() && e.scheduled.is_none() && e.at_date.is_none()
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
        Command::Today(query) => report(query, false, None, false, false),
        Command::Agenda { query, week } => report(query, week, None, false, false),
        Command::Tasks { query, tag, all } => report(query, false, tag, all, true),
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
                    PathBuf::from(format!("journal/{today}.jot"))
                } else {
                    PathBuf::from("inbox.jot")
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
            let (file, row) = target.rsplit_once(':').ok_or("Use file.jot:LINE")?;
            let row: usize = row.parse().map_err(|_| "Invalid line number")?;
            let path =
                std::fs::canonicalize(workspace.root().join(file)).map_err(|e| e.to_string())?;
            let doc = workspace
                .documents
                .get(&path)
                .ok_or("File is not in this workspace's indexed .jot notes")?;
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
        Command::Plan {
            name,
            root,
            json,
            export,
            import,
        } => {
            if let Some(file) = import {
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
            let name = name.ok_or("Give a plan name, or --import a problem file")?;
            let workspace = load(root)?;
            let root = workspace.root().to_path_buf();
            let symbol = workspace.resolve(&root, &name)?;
            let (_, plan) = crate::plans::plan(&workspace, &symbol)
                .ok_or_else(|| format!("'{name}' is not a plan"))?;
            let mut engine = Engine::new(&workspace, Local::now().date_naive());
            if export {
                let problem = crate::plans::export(&mut engine, &symbol, plan)?;
                println!("{}", serde_json::to_string_pretty(&problem).unwrap());
                return Ok(());
            }
            let Value::Plan(solved) = engine.symbol(&symbol)? else {
                return Err(format!("'{name}' is not a plan"));
            };
            if json {
                let out = serde_json::json!({
                    "goal": solved.goal.keyword(),
                    "objective": solved.objective.display(),
                    "variables": solved.variables.iter().map(|(n, v)| (n.clone(), serde_json::json!(v.display()))).collect::<serde_json::Map<_, _>>(),
                    "constraints": solved.constraints.iter().map(|c| serde_json::json!({
                        "name": c.name, "lhs": c.lhs.display(), "op": c.op, "rhs": c.rhs.display(),
                        "slack": c.slack.display(), "binding": c.binding,
                    })).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
                return Ok(());
            }
            println!(
                "{name}: {} {}",
                solved.goal.keyword(),
                solved.objective.display()
            );
            for (n, v) in &solved.variables {
                println!("  {n} = {}", v.display());
            }
            for c in &solved.constraints {
                println!(
                    "  {}: {} {} {} · {}",
                    c.name,
                    c.lhs.display(),
                    c.op,
                    c.rhs.display(),
                    if c.binding {
                        "binding".to_string()
                    } else {
                        format!("slack {}", c.slack.display())
                    }
                );
            }
            Ok(())
        }
        Command::Check { root } => {
            let workspace = load(root)?;
            let mut count = 0;
            for (path, doc) in &workspace.documents {
                for p in crate::editor::problems(&workspace, path, Local::now().date_naive()) {
                    count += 1;
                    println!(
                        "{}:{}:{}: {}",
                        path.display(),
                        p.span.line + 1,
                        p.span.range(&doc.text).start.character + 1,
                        p.message
                    );
                }
            }
            if count > 0 {
                Err(format!("{count} problem(s)"))
            } else {
                println!("All notes are valid");
                Ok(())
            }
        }
    }
}
fn report(
    query: Query,
    week: bool,
    tag: Option<String>,
    all: bool,
    tasks: bool,
) -> Result<(), String> {
    let workspace = load(query.root)?;
    let today = query.on.unwrap_or_else(|| Local::now().date_naive());
    let end = today
        .checked_add_signed(Duration::days(if week { 6 } else { 0 }))
        .ok_or("Date overflow")?;
    let rows: Vec<_> = entries(&workspace, today)
        .into_iter()
        .filter(|e| {
            if tasks {
                e.kind == "task" && (all || !e.done)
            } else {
                agenda_entry(e, today, end)
            }
        })
        .filter(|e| tag.as_ref().is_none_or(|tag| e.tags.contains(tag)))
        .collect();
    if query.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if rows.is_empty() {
        println!("No matching tasks or appointments.");
    }
    for e in rows {
        let mut labels = Vec::new();
        if let Some(due) = e.due {
            labels.push(format!(
                "due {due}{}",
                if due < today { " (overdue)" } else { "" }
            ));
        }
        if let Some(s) = e.scheduled {
            labels.push(format!("scheduled {s}"));
        }
        if let Some(at) = e.at {
            labels.push(at);
        }
        if let Some(s) = e.estimate_seconds {
            labels.push(Value::Duration(s).display());
        }
        if !e.blocked_by.is_empty() {
            labels.push(format!("blocked by {}", e.blocked_by.join(", ")));
        }
        labels.extend(e.errors);
        println!(
            "{}:{}  {} {}{}",
            e.path.display(),
            e.line,
            if e.kind == "event" || e.kind == "stop" {
                "•"
            } else if e.done {
                "[x]"
            } else {
                "[ ]"
            },
            e.title,
            if labels.is_empty() {
                String::new()
            } else {
                format!("  — {}", labels.join(" · "))
            }
        );
    }
    Ok(())
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
    let tmp = parent.join(format!(".jot-write-{}.tmp", std::process::id()));
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
