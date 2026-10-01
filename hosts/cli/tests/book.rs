//! The book in `book/` is tested like code, so it cannot drift from it.
//!
//! Every chapter's fenced blocks run against the real binary in one shared
//! workspace, with the clock frozen and no user modules:
//!
//! - ```` ```xmd trip.x.md ```` writes a file into the workspace for later
//!   blocks and chapters to use; ```` ```xmd ```` alone is an anonymous note.
//!   Either kind must report no diagnostics.
//! - Each line of a ```` ```bash ```` block that runs `xmd` is a command, run
//!   with `sh` in the workspace. A trailing `#=> text` is its expected stdout.
//!
//! What every block renders (`xmd render --format text`) and every command
//! prints is compared with `book/snapshots/<chapter>.txt`, so a change to the
//! language or the CLI shows up as a diff to the book. Rewrite the snapshots
//! with `UPDATE_SNAPSHOTS=1`.
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

const NOW: &str = "2026-09-16T14:00:00-04:00";

struct Chapter {
    name: String,
    blocks: Vec<Block>,
}

enum Block {
    Note { file: String },
    Commands(Vec<String>),
}

#[test]
fn book_runs_as_written() {
    let book = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../book");
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().canonicalize().unwrap();
    let config = tempfile::tempdir().unwrap();
    let chapters = load(&book, &root);
    assert!(!chapters.is_empty(), "no chapters in {}", book.display());

    let update = std::env::var("UPDATE_SNAPSHOTS").is_ok_and(|v| !v.is_empty() && v != "0");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xmd"))
            .args(args)
            .current_dir(&root)
            .env("XDG_CONFIG_HOME", config.path())
            .env("XMD_NOW", NOW)
            .output()
            .unwrap()
    };
    let mut failures = Vec::new();
    for chapter in &chapters {
        let mut transcript = String::new();
        for block in &chapter.blocks {
            match block {
                Block::Note { file } => {
                    let output = run(&["query", file, "diagnostics | map(.message)", "--json"]);
                    let diagnostics = String::from_utf8_lossy(&output.stdout);
                    if !output.status.success() || diagnostics.trim() != "[]" {
                        failures.push(format!(
                            "{}: {file} reports diagnostics:\n{diagnostics}{}",
                            chapter.name,
                            String::from_utf8_lossy(&output.stderr)
                        ));
                    }
                    let output = run(&["render", file, "--format", "text"]);
                    writeln!(transcript, "=== {file}").unwrap();
                    transcript.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                Block::Commands(commands) => {
                    for command in commands {
                        let output = Command::new("sh")
                            .arg("-c")
                            .arg(command)
                            .current_dir(&root)
                            .env("PATH", path_with_binary())
                            .env("XDG_CONFIG_HOME", config.path())
                            .env("XMD_NOW", NOW)
                            .output()
                            .unwrap();
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        writeln!(transcript, "=== $ {command}").unwrap();
                        transcript.push_str(&stdout);
                        transcript.push_str(&String::from_utf8_lossy(&output.stderr));
                        if let Some(code) = output.status.code().filter(|&c| c != 0) {
                            writeln!(transcript, "[exit {code}]").unwrap();
                        }
                        if let Some((_, expected)) = command.split_once("#=>")
                            && stdout.trim() != expected.trim()
                        {
                            failures.push(format!(
                                "{}: `{command}` printed {:?}",
                                chapter.name,
                                stdout.trim()
                            ));
                        }
                    }
                }
            }
        }
        if transcript.is_empty() {
            continue;
        }
        let transcript = transcript.replace(&root.display().to_string(), "<root>");
        let snapshot = book.join("snapshots").join(format!("{}.txt", chapter.name));
        let expected = std::fs::read_to_string(&snapshot);
        if update {
            std::fs::create_dir_all(snapshot.parent().unwrap()).unwrap();
            std::fs::write(&snapshot, &transcript).unwrap();
        } else if let Ok(expected) = expected {
            if expected != transcript {
                failures.push(format!(
                    "{}: output differs from {}\n{}",
                    chapter.name,
                    snapshot.display(),
                    first_difference(&expected, &transcript)
                ));
            }
        } else {
            failures.push(format!(
                "{}: no snapshot at {}",
                chapter.name,
                snapshot.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}\nRerun with UPDATE_SNAPSHOTS=1 to accept output changes.",
        failures.join("\n\n")
    );
}

/// Reads the chapters in order and writes every note block into `root`.
fn load(book: &Path, root: &Path) -> Vec<Chapter> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(book)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.ends_with(".md") && name.starts_with(|c: char| c.is_ascii_digit())
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(path).unwrap();
            let blocks = fences(&text)
                .into_iter()
                .enumerate()
                .filter_map(|(index, (info, body))| {
                    let mut words = info.split_whitespace();
                    match words.next()? {
                        "xmd" => {
                            let file = match words.next() {
                                Some(file) => file.to_string(),
                                None => format!("{name}-{}.x.md", index + 1),
                            };
                            let target = root.join(&file);
                            assert!(!target.exists(), "{name}: {file} is written twice");
                            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                            std::fs::write(&target, body).unwrap();
                            Some(Block::Note { file })
                        }
                        "bash" => Some(Block::Commands(
                            body.lines()
                                .map(str::trim)
                                .filter(|line| !line.starts_with('#') && line.contains("xmd"))
                                .map(String::from)
                                .collect(),
                        )),
                        _ => None,
                    }
                })
                .collect();
            Chapter { name, blocks }
        })
        .collect()
}

/// The fenced blocks of a markdown page as (info string, body).
fn fences(text: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in text.lines() {
        match (&mut open, line.strip_prefix("```")) {
            (None, Some(info)) => open = Some((info.trim().to_string(), String::new())),
            (Some(_), Some(_)) => blocks.push(open.take().unwrap()),
            (Some((_, body)), None) => {
                body.push_str(line);
                body.push('\n');
            }
            (None, None) => {}
        }
    }
    blocks
}

fn path_with_binary() -> String {
    let binary = Path::new(env!("CARGO_BIN_EXE_xmd")).parent().unwrap();
    format!(
        "{}:{}",
        binary.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn first_difference(expected: &str, actual: &str) -> String {
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    for number in 1.. {
        match (expected_lines.next(), actual_lines.next()) {
            (None, None) => break,
            (e, a) if e == a => continue,
            (e, a) => {
                return format!(
                    "line {number}\n  expected: {}\n  actual:   {}",
                    e.unwrap_or("<end>"),
                    a.unwrap_or("<end>")
                );
            }
        }
    }
    String::new()
}
