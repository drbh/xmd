//! `wtf sync`: mirror a directory of notes with a folder in the web app.
//!
//! The directory keeps a manifest and the last synced copy of every file under
//! `.wtf-sync/`, so each run is a three-way comparison per file: unchanged on
//! one side means the other side wins; changed on both sides is merged line by
//! line, and a real conflict is written beside the file as `NAME.conflict.x.md`
//! (the note extension comes from `common`)
//! without touching either version. The web app addresses documents by file
//! name within a folder, so relative imports mean the same thing on both sides.
//! Transport is `curl`, like the rest of the command line.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const STATE_DIR: &str = ".wtf-sync";

#[derive(clap::Args)]
#[command(
    after_help = "The key comes from --key, WTF_API_KEY, or the credentials saved by an earlier --key.\nCreate one in the web app under your account menu, API keys.\nExamples: wtf sync ./notes --url https://wtf-docs.example.com --folder Notes\n          wtf sync ./notes --watch"
)]
pub(crate) struct SyncOptions {
    /// The directory of notes to keep in step with a folder in the web app.
    pub dir: PathBuf,
    /// The web app's address (remembered in DIR/.wtf-sync/config.json).
    #[arg(long)]
    pub url: Option<String>,
    /// The folder in the web app; created if missing (remembered too).
    #[arg(long)]
    pub folder: Option<String>,
    /// An API key (or WTF_API_KEY); saved for this address in the user's config directory.
    #[arg(long)]
    pub key: Option<String>,
    /// Keep running, syncing whenever either side changes.
    #[arg(long)]
    pub watch: bool,
    /// Seconds between checks in --watch mode.
    #[arg(long, default_value_t = 5)]
    pub interval: u64,
    /// Report what would change without touching anything.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Config {
    url: String,
    folder: String,
}
#[derive(Serialize, Deserialize, Default)]
struct Manifest {
    folder_id: Option<String>,
    files: BTreeMap<String, Entry>,
}
#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    id: String,
    version: u64,
}
#[derive(Deserialize, Clone)]
struct RemoteDocument {
    id: String,
    name: String,
    #[serde(default)]
    file: Option<String>,
    text: String,
    version: u64,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    folder: Option<String>,
}
#[derive(Deserialize)]
struct RemoteFolder {
    id: String,
    name: String,
    #[serde(default)]
    role: Option<String>,
}

pub(crate) fn run(options: SyncOptions) -> Result<(), String> {
    let dir = options
        .dir
        .canonicalize()
        .map_err(|e| format!("Cannot open {}: {e}", options.dir.display()))?;
    let state = dir.join(STATE_DIR);
    fs::create_dir_all(state.join("base"))
        .map_err(|e| format!("Cannot create {}: {e}", state.display()))?;
    let config_path = state.join("config.json");
    let mut config: Config = read_json(&config_path).unwrap_or_default();
    if let Some(url) = &options.url {
        config.url = url.trim_end_matches('/').to_string();
    }
    if let Some(folder) = &options.folder {
        config.folder = folder.clone();
    }
    if config.url.is_empty() {
        return Err("Pass --url the first time, e.g. --url https://wtf-docs.example.com".into());
    }
    if config.folder.is_empty() {
        config.folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Notes".into());
    }
    write_json(&config_path, &config)?;
    let given = options
        .key
        .clone()
        .or_else(|| std::env::var("WTF_API_KEY").ok().filter(|k| !k.is_empty()));
    let key = resolve_key(&config.url, given.as_deref())?;
    let client = Client {
        url: config.url.clone(),
        key,
    };

    let mut manifest: Manifest = read_json(&state.join("manifest.json")).unwrap_or_default();
    loop {
        let report = sync_once(
            &dir,
            &state,
            &client,
            &config,
            &mut manifest,
            options.dry_run,
        )?;
        if !options.dry_run {
            write_json(&state.join("manifest.json"), &manifest)?;
        }
        for line in &report {
            println!("{line}");
        }
        if !options.watch {
            if report.is_empty() {
                println!("Up to date.");
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(options.interval.max(1)));
    }
}

fn sync_once(
    dir: &Path,
    state: &Path,
    client: &Client,
    config: &Config,
    manifest: &mut Manifest,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    let mut report = Vec::new();
    // The folder in the web app, created on first use.
    let folder_id = match &manifest.folder_id {
        Some(id) => id.clone(),
        None => {
            let folders: Vec<RemoteFolder> = client.get("/folders")?;
            match folders
                .into_iter()
                .find(|f| f.name.eq_ignore_ascii_case(&config.folder))
            {
                Some(f) => {
                    if matches!(f.role.as_deref(), Some("viewer")) {
                        return Err(format!("You can only view the folder \"{}\"", f.name));
                    }
                    manifest.folder_id = Some(f.id.clone());
                    f.id
                }
                None => {
                    if dry_run {
                        report.push(format!("would create folder \"{}\"", config.folder));
                        return Ok(report);
                    }
                    let id = new_id();
                    let created: RemoteFolder = client.put(
                        &format!("/folders/{id}"),
                        &serde_json::json!({ "name": config.folder }),
                    )?;
                    report.push(format!("created folder \"{}\"", created.name));
                    manifest.folder_id = Some(created.id.clone());
                    created.id
                }
            }
        }
    };
    let remote: BTreeMap<String, RemoteDocument> = client
        .get::<Vec<RemoteDocument>>("/documents")?
        .into_iter()
        .filter(|d| d.folder.as_deref() == Some(folder_id.as_str()))
        .filter(|d| !matches!(d.role.as_deref(), Some("viewer")))
        .map(|d| {
            (
                common::note_file(&d.file.clone().unwrap_or_else(|| file_name_for(&d.name))),
                d,
            )
        })
        .collect();
    let local: BTreeMap<String, String> = fs::read_dir(dir)
        .map_err(|e| format!("Cannot read {}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            (common::note_stem(&name).is_some_and(|stem| !stem.ends_with(".conflict"))
                && !name.starts_with('.'))
            .then(|| {
                fs::read_to_string(entry.path())
                    .ok()
                    .map(|text| (name, text))
            })
            .flatten()
        })
        .collect();
    let names: BTreeSet<String> = local
        .keys()
        .chain(remote.keys())
        .chain(manifest.files.keys())
        .cloned()
        .collect();

    for name in names {
        let base_path = state.join("base").join(&name);
        let base = fs::read_to_string(&base_path).ok();
        let entry = manifest.files.get(&name).cloned();
        let local_text = local.get(&name);
        let remote_doc = remote.get(&name);
        let local_changed = match (&base, local_text) {
            (Some(b), Some(l)) => b != l,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => false,
        };
        let remote_changed = match (&entry, remote_doc) {
            (Some(e), Some(r)) => e.version != r.version,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => false,
        };
        let stem = common::note_stem(&name).unwrap_or(&name).to_string();
        let conflict = conflict_file(&stem);
        // A conflict waits for a person: nothing moves until the marker file is gone.
        if dir.join(&conflict).exists() {
            report.push(format!(
                "{stem}: still has {conflict}; resolve it to continue syncing this file"
            ));
            continue;
        }
        let action = match (
            entry.is_some(),
            local_text,
            remote_doc,
            local_changed,
            remote_changed,
        ) {
            // Gone on both sides: forget it.
            (_, None, None, _, _) => Action::Forget,
            // New here, unknown there.
            (false, Some(l), None, _, _) => Action::Create(l.clone()),
            // New there, unknown here.
            (false, None, Some(r), _, _) => Action::Pull(r.clone()),
            // Both sides have a file of this name that was never synced.
            (false, Some(l), Some(r), _, _) => {
                if *l == r.text {
                    Action::Adopt(r.clone())
                } else {
                    Action::Merge(String::new(), l.clone(), r.clone())
                }
            }
            (true, None, Some(r), _, false) => Action::DeleteRemote(r.clone()),
            (true, None, Some(r), _, true) => Action::Pull(r.clone()), // deleted here but changed there: bring it back
            (true, Some(_), None, false, _) => Action::TrashLocal,
            (true, Some(l), None, true, _) => Action::Create(l.clone()), // deleted there but changed here: keep it
            (true, Some(_), Some(_), false, false) => Action::Nothing,
            (true, Some(_), Some(r), false, true) => Action::Pull(r.clone()),
            (true, Some(l), Some(r), true, false) => Action::Push(l.clone(), r.clone()),
            (true, Some(l), Some(r), true, true) => {
                Action::Merge(base.clone().unwrap_or_default(), l.clone(), r.clone())
            }
        };
        if dry_run {
            if let Some(line) = describe(&stem, &action) {
                report.push(format!("would {line}"));
            }
            continue;
        }
        match action {
            Action::Nothing => {}
            Action::Forget => {
                manifest.files.remove(&name);
                let _ = fs::remove_file(&base_path);
            }
            Action::Create(text) => {
                let id = new_id();
                let saved: serde_json::Value = client.put(
                    &format!("/documents/{id}"),
                    &serde_json::json!({ "name": title_of(&text, &stem), "file": stem, "text": text, "folder": folder_id }),
                )?;
                let version = saved["version"].as_u64().unwrap_or(1);
                manifest.files.insert(name.clone(), Entry { id, version });
                fs::write(&base_path, &text).map_err(|e| e.to_string())?;
                report.push(format!("created {stem} in the web app"));
            }
            Action::Adopt(r) => {
                manifest.files.insert(
                    name.clone(),
                    Entry {
                        id: r.id,
                        version: r.version,
                    },
                );
                fs::write(&base_path, &r.text).map_err(|e| e.to_string())?;
            }
            Action::Pull(r) => {
                fs::write(dir.join(&name), &r.text)
                    .map_err(|e| format!("Cannot write {name}: {e}"))?;
                fs::write(&base_path, &r.text).map_err(|e| e.to_string())?;
                manifest.files.insert(
                    name.clone(),
                    Entry {
                        id: r.id,
                        version: r.version,
                    },
                );
                report.push(format!("pulled {stem}"));
            }
            Action::Push(text, r) => {
                match client.push(&r.id, &stem, &text, Some(r.version), &folder_id) {
                    Ok(version) => {
                        manifest
                            .files
                            .insert(name.clone(), Entry { id: r.id, version });
                        fs::write(&base_path, &text).map_err(|e| e.to_string())?;
                        report.push(format!("pushed {stem}"));
                    }
                    Err(Conflict::Changed(current)) => {
                        // It moved on since we listed it; merge against what is there now.
                        let merged = merge(base.as_deref().unwrap_or(""), &text, &current.text);
                        finish_merge(
                            dir,
                            state,
                            &name,
                            &stem,
                            merged,
                            *current,
                            client,
                            manifest,
                            &folder_id,
                            &mut report,
                        )?;
                    }
                    Err(Conflict::Other(e)) => return Err(e),
                }
            }
            Action::Merge(base_text, l, r) => {
                let merged = merge(&base_text, &l, &r.text);
                finish_merge(
                    dir,
                    state,
                    &name,
                    &stem,
                    merged,
                    r,
                    client,
                    manifest,
                    &folder_id,
                    &mut report,
                )?;
            }
            Action::DeleteRemote(r) => {
                client.delete(&format!("/documents/{}", r.id))?;
                manifest.files.remove(&name);
                let _ = fs::remove_file(&base_path);
                report.push(format!(
                    "removed {stem} from the web app (it was deleted here)"
                ));
            }
            Action::TrashLocal => {
                let trash = state.join("trash");
                fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
                fs::rename(dir.join(&name), trash.join(&name))
                    .map_err(|e| format!("Cannot move {name}: {e}"))?;
                manifest.files.remove(&name);
                let _ = fs::remove_file(&base_path);
                report.push(format!(
                    "moved {stem} to {}/trash (it was removed in the web app)",
                    STATE_DIR
                ));
            }
        }
    }
    Ok(report)
}

enum Action {
    Nothing,
    Forget,
    Create(String),
    Adopt(RemoteDocument),
    Pull(RemoteDocument),
    Push(String, RemoteDocument),
    Merge(String, String, RemoteDocument),
    DeleteRemote(RemoteDocument),
    TrashLocal,
}
fn describe(stem: &str, action: &Action) -> Option<String> {
    Some(match action {
        Action::Nothing | Action::Forget | Action::Adopt(_) => return None,
        Action::Create(_) => format!("create {stem} in the web app"),
        Action::Pull(_) => format!("pull {stem}"),
        Action::Push(..) => format!("push {stem}"),
        Action::Merge(..) => format!("merge {stem} (changed on both sides)"),
        Action::DeleteRemote(_) => format!("remove {stem} from the web app"),
        Action::TrashLocal => format!("move {stem} to trash"),
    })
}

/// A line-based three-way merge; `Err` carries the text with conflict markers.
pub(crate) fn merge(base: &str, ours: &str, theirs: &str) -> Result<String, String> {
    if ours == theirs {
        return Ok(ours.to_string());
    }
    diffy::merge(base, ours, theirs)
}

#[allow(clippy::too_many_arguments)]
fn finish_merge(
    dir: &Path,
    state: &Path,
    name: &str,
    stem: &str,
    merged: Result<String, String>,
    remote: RemoteDocument,
    client: &Client,
    manifest: &mut Manifest,
    folder_id: &str,
    report: &mut Vec<String>,
) -> Result<(), String> {
    let base_path = state.join("base").join(name);
    match merged {
        Ok(text) => {
            fs::write(dir.join(name), &text).map_err(|e| format!("Cannot write {name}: {e}"))?;
            match client.push(&remote.id, stem, &text, Some(remote.version), folder_id) {
                Ok(version) => {
                    manifest.files.insert(
                        name.to_string(),
                        Entry {
                            id: remote.id,
                            version,
                        },
                    );
                    fs::write(&base_path, &text).map_err(|e| e.to_string())?;
                    report.push(format!("merged {stem}"));
                }
                Err(Conflict::Changed(_)) => {
                    report.push(format!("{stem} keeps changing in the web app; try again"))
                }
                Err(Conflict::Other(e)) => return Err(e),
            }
        }
        Err(conflicted) => {
            let conflict = conflict_file(stem);
            let path = dir.join(&conflict);
            fs::write(&path, conflicted)
                .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
            // Remember the version we saw so the remote side is not "changed" again until it really changes.
            manifest.files.insert(
                name.to_string(),
                Entry {
                    id: remote.id,
                    version: remote.version,
                },
            );
            fs::write(&base_path, &remote.text).map_err(|e| e.to_string())?;
            report.push(format!(
                "conflict in {stem}: resolve {conflict}, copy it over {name}, and delete it"
            ));
        }
    }
    Ok(())
}

/// The display name the web app would derive: the first heading, else the file name.
fn title_of(text: &str, fallback: &str) -> String {
    text.lines()
        .find(|l| {
            l.starts_with('#')
                && l.trim_start_matches('#').starts_with(' ')
                && !l.trim_start_matches('#').trim().is_empty()
        })
        .map(|l| {
            let t = l.trim_start_matches('#').trim();
            match t.rfind(" :") {
                Some(at) if t[at + 2..].chars().all(|c| c.is_alphanumeric() || c == '_') => {
                    t[..at].trim().to_string()
                }
                _ => t.to_string(),
            }
        })
        .unwrap_or_else(|| fallback.to_string())
}
/// Where a conflicted merge is written beside the note: `NAME.conflict.x.md`.
fn conflict_file(stem: &str) -> String {
    common::note_file(&format!("{stem}.conflict"))
}
fn file_name_for(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let clean = common::note_stem(&clean)
        .unwrap_or(&clean)
        .trim()
        .chars()
        .take(120)
        .collect::<String>();
    if clean.is_empty() {
        "Untitled document".into()
    } else {
        clean
    }
}
fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for salt in 0..4u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(
            salt ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        );
        out.push_str(&format!("{:016x}", h.finish()));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &out[..8],
        &out[8..12],
        &out[12..16],
        &out[16..20],
        &out[20..32]
    )
}

// Credentials: --key is saved per address under the user's config directory.
fn credentials_path() -> Option<PathBuf> {
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(home.join("wtf").join("credentials.json"))
}
fn resolve_key(url: &str, given: Option<&str>) -> Result<String, String> {
    let path = credentials_path();
    let mut saved: BTreeMap<String, String> =
        path.as_ref().and_then(|p| read_json(p)).unwrap_or_default();
    if let Some(key) = given {
        if let Some(path) = &path {
            saved.insert(url.to_string(), key.to_string());
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            write_json(path, &saved)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            }
        }
        return Ok(key.to_string());
    }
    saved
        .get(url)
        .cloned()
        .ok_or_else(|| "No API key: pass --key once, or set WTF_API_KEY".to_string())
}
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("Cannot write {}: {e}", path.display()))
}

// Transport: curl, as the rest of the command line uses.
struct Client {
    url: String,
    key: String,
}
enum Conflict {
    Changed(Box<RemoteDocument>),
    Other(String),
}
impl Client {
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<(u16, String), String> {
        let mut command = Command::new("curl");
        command
            .arg("-sS")
            .arg("-X")
            .arg(method)
            .arg(format!("{}/sync/v1{path}", self.url))
            .arg("-H")
            .arg(format!("Authorization: Bearer {}", self.key))
            .arg("-H")
            .arg("Accept: application/json")
            .arg("-w")
            .arg("\n%{http_code}")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if body.is_some() {
            command
                .arg("-H")
                .arg("Content-Type: application/json")
                .arg("--data-binary")
                .arg("@-")
                .stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot run curl: {e}"))?;
        if let Some(body) = body {
            use io::Write;
            let json = serde_json::to_vec(body).map_err(|e| e.to_string())?;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(&json)
                .map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "curl failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let (body, code) = text.rsplit_once('\n').ok_or("Unexpected response")?;
        Ok((code.trim().parse().unwrap_or(0), body.to_string()))
    }
    fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, String> {
        let (status, body) = self.request("GET", path, None)?;
        parse(status, &body)
    }
    fn put<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        let (status, text) = self.request("PUT", path, Some(body))?;
        parse(status, &text)
    }
    fn delete(&self, path: &str) -> Result<(), String> {
        let (status, body) = self.request("DELETE", path, None)?;
        parse::<serde_json::Value>(status, &body).map(|_| ())
    }
    /// Save a document, or learn that it changed since `version`.
    fn push(
        &self,
        id: &str,
        stem: &str,
        text: &str,
        version: Option<u64>,
        folder: &str,
    ) -> Result<u64, Conflict> {
        let body = serde_json::json!({ "name": title_of(text, stem), "file": stem, "text": text, "version": version, "folder": folder });
        let (status, response) = self
            .request("PUT", &format!("/documents/{id}"), Some(&body))
            .map_err(Conflict::Other)?;
        if status == 409 {
            let value: serde_json::Value = serde_json::from_str(&response).unwrap_or_default();
            if let Ok(current) = serde_json::from_value::<RemoteDocument>(value["current"].clone())
            {
                return Err(Conflict::Changed(Box::new(current)));
            }
            return Err(Conflict::Other(error_message(status, &response)));
        }
        let saved: serde_json::Value = parse(status, &response).map_err(Conflict::Other)?;
        Ok(saved["version"]
            .as_u64()
            .unwrap_or(version.unwrap_or(0) + 1))
    }
}
fn parse<T: for<'de> Deserialize<'de>>(status: u16, body: &str) -> Result<T, String> {
    if !(200..300).contains(&status) {
        return Err(error_message(status, body));
    }
    serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))
}
fn error_message(status: u16, body: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    match value["error"].as_str() {
        Some(message) if status == 401 => {
            format!("{message} (create one in the web app under API keys)")
        }
        Some(message) => message.to_string(),
        None => format!("The web app answered {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merges_independent_changes_and_flags_real_conflicts() {
        // Changes in different places merge; the same line changed both ways is a conflict, like git.
        let base = "# Budget\n\nrent := $900\n\nfood := $200\n";
        let ours = "# Budget\n\nrent := $950\n\nfood := $200\n";
        let theirs = "# Budget\n\nrent := $900\n\nfood := $250\n";
        assert_eq!(
            merge(base, ours, theirs).unwrap(),
            "# Budget\n\nrent := $950\n\nfood := $250\n"
        );
        let clash = merge(base, ours, "# Budget\n\nrent := $1,000\n\nfood := $200\n").unwrap_err();
        assert!(clash.contains("<<<<<<<") && clash.contains("$950") && clash.contains("$1,000"));
        assert_eq!(merge(base, ours, ours).unwrap(), ours);
    }
    #[test]
    fn titles_and_file_names_follow_the_app() {
        assert_eq!(title_of("# Trip budget :prep\n\ntext", "x"), "Trip budget");
        assert_eq!(title_of("no heading", "fallback"), "fallback");
        assert_eq!(
            file_name_for(&format!(" a/b\\c  {} ", common::note_file("d"))),
            "a b c d"
        );
        assert_eq!(file_name_for(""), "Untitled document");
    }
}
