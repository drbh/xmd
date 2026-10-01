//! The file and process plumbing the rest of the crate shares.
use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

/// An error about a file, as every host message names one: `{path}: {error}`.
pub(crate) fn at<E: Display>(path: &Path) -> impl Fn(E) -> String + '_ {
    move |error| format!("{}: {error}", path.display())
}

/// The user's home directory, `$HOME`, which `~/` links resolve against.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The user's own settings: `$XDG_CONFIG_HOME/xmd` or `~/.config/xmd`.
pub(crate) fn config_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".config")))?;
    Some(config.join("xmd"))
}

/// A JSON file the host keeps for itself; missing or unreadable means empty.
pub(crate) fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Save pretty JSON through a `{name}-{pid}.tmp` sibling, since an atomic
/// replacement leaves no partially written file after a crash.
pub(crate) fn write_json_atomic<T: serde::Serialize + ?Sized>(
    path: &Path,
    value: &T,
) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!("{stem}-{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

/// Run a program for at most 20 seconds and return its stdout. The messages
/// read `{timed_out}`, `Cannot run {program}: …` and `{failed} failed: …`.
pub(crate) async fn output(
    command: &mut tokio::process::Command,
    timed_out: &str,
    program: &str,
    failed: &str,
) -> Result<Vec<u8>, String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        command.kill_on_drop(true).output(),
    )
    .await
    .map_err(|_| timed_out.to_string())?
    .map_err(|e| format!("Cannot run {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{failed} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}
