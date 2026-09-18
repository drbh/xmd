use chrono::{DateTime, Utc};
use lsp_types::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub target: String,
    pub origin: Option<PathBuf>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub state: String,
    pub merged: Option<bool>,
    pub checks: Option<String>,
    pub review: Option<String>,
    pub fetched_at: DateTime<Utc>,
}
pub type Cache = BTreeMap<String, Metadata>;

impl Resource {
    pub fn parse(s: &str) -> Option<Self> {
        (s.starts_with("https://")
            || s.starts_with("http://")
            || s.starts_with("geo:")
            || s.starts_with("./")
            || s.starts_with("../")
            || s.starts_with("~/")
            || s.starts_with('/')
            || s.starts_with("file://")
            || bare_file_path(s))
        .then(|| Self {
            target: s.into(),
            origin: None,
        })
    }
    pub fn url(&self, document: &Path) -> Result<Url, String> {
        let document = self.origin.as_deref().unwrap_or(document);
        if let Some(coords) = self.target.strip_prefix("geo:") {
            let (lat, lon) = coords
                .split_once(',')
                .ok_or("Expected geo:latitude,longitude")?;
            let lat: f64 = lat.parse().map_err(|_| "Invalid latitude")?;
            let lon: f64 = lon.parse().map_err(|_| "Invalid longitude")?;
            if !lat.is_finite()
                || !lon.is_finite()
                || !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
            {
                return Err("Coordinates are out of range".into());
            }
            return Url::parse(&format!(
                "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}"
            ))
            .map_err(|e| e.to_string());
        }
        if self.target.starts_with("http://")
            || self.target.starts_with("https://")
            || self.target.starts_with("file://")
        {
            // A malformed URL is an error, never a relative file path.
            return Url::parse(&self.target).map_err(|e| e.to_string());
        }
        if let Ok(url) = Url::parse(&self.target) {
            if matches!(url.scheme(), "https" | "http" | "file") {
                return Ok(url);
            }
            return Err("Unsupported link scheme".into());
        }
        if let Some(relative) = self.target.strip_prefix("~/") {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let home_dir = std::env::var_os("HOME").ok_or("Home directory is unavailable")?;
                return resolved_file_url(&PathBuf::from(home_dir).join(relative));
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = relative;
                return Err(
                    "Home-directory paths can be opened in the native editor, not the browser"
                        .into(),
                );
            }
        }
        let path = document
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.target);
        resolved_file_url(&path)
    }
    pub fn is_image(&self) -> bool {
        let target = self
            .target
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_lowercase();
        [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"]
            .iter()
            .any(|e| target.ends_with(e))
    }
    /// The inline status shown next to a resource: cached GitHub state with
    /// its age, or what kind of thing the link opens.
    pub fn label(&self, cache: &Cache, now: DateTime<Utc>) -> String {
        if let Some(m) = cache.get(&self.target) {
            return m.badge(now);
        }
        if self.target.starts_with("geo:") {
            return "place · open map".into();
        }
        if let Some((_, kind, number)) = github(&self.target) {
            return format!(
                "{} {} · refresh for status",
                crate::glyphs::PENDING,
                match kind.as_str() {
                    "pull" => format!("PR #{number}"),
                    "issues" => format!("issue #{number}"),
                    _ => format!("commit {}", &number[..number.len().min(7)]),
                }
            );
        }
        if self.is_image() {
            return "image · open preview".into();
        }
        if self.target.starts_with("http") {
            "link".into()
        } else {
            "file".into()
        }
    }
    pub fn hover(&self, document: &Path, cache: &Cache) -> String {
        let mut out = match self.url(document) {
            Ok(url) => format!(
                "[Open {}](<{}>)",
                if self.target.starts_with("geo:") {
                    "map"
                } else {
                    "resource"
                },
                url
            ),
            Err(err) => err,
        };
        if self.is_image()
            && let Ok(url) = self.url(document)
        {
            out.push_str(&format!("\n\n![Preview](<{url}>)"));
        }
        if let Some(m) = cache.get(&self.target) {
            out.push_str(&format!(
                "\n\n**{}**\n\n{}\n\nLast refreshed: {}. Use **Refresh GitHub status** to update.",
                m.title,
                m.summary(),
                m.fetched_at.to_rfc3339()
            ));
        } else if github(&self.target).is_some() {
            #[cfg(target_arch = "wasm32")]
            out.push_str("\n\nGitHub status refresh is available in the native WTF app, not this browser workspace.");
            #[cfg(not(target_arch = "wasm32"))]
            out.push_str("\n\nNo cached status. Run `wtf refresh`, or use the Refresh GitHub status code action on this resource. Requires the GitHub CLI (`gh`).");
        }
        out
    }
}

fn resolved_file_url(path: &Path) -> Result<Url, String> {
    let url = crate::paths::file_url(path)?;
    // from_file_path preserves dot segments; parsing normalizes them without IO.
    // Native and browser links must use the same canonical URI to find open notes.
    Url::parse(url.as_str()).map_err(|e| e.to_string())
}

/// Recognize unprefixed paths without turning fractions, domains, or ordinary
/// prose such as "and/or" into file links. Use ./ for ambiguous extensionless paths.
fn bare_file_path(s: &str) -> bool {
    if s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || "<>\"`|:?!*".contains(c))
    {
        return false;
    }
    if matches!(s, "Makefile" | "Dockerfile" | "LICENSE") {
        return true;
    }
    let file = s.rsplit('/').next().unwrap_or(s);
    if file.starts_with('.')
        && !file.starts_with("..")
        && file.chars().any(|c| c.is_alphabetic() || c == '_')
    {
        return true;
    }
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    if !stem.chars().any(char::is_alphabetic)
        || extension.is_empty()
        || !extension.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return false;
    }
    s.contains('/')
        || matches!(
            extension.to_ascii_lowercase().as_str(),
            "wtf"
                | "md"
                | "txt"
                | "pdf"
                | "rs"
                | "js"
                | "mjs"
                | "cjs"
                | "jsx"
                | "ts"
                | "tsx"
                | "json"
                | "jsonc"
                | "toml"
                | "yaml"
                | "yml"
                | "lock"
                | "html"
                | "css"
                | "scss"
                | "py"
                | "go"
                | "rb"
                | "sh"
                | "zsh"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "swift"
                | "java"
                | "kt"
                | "sql"
                | "csv"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "mp4"
                | "mov"
                | "mp3"
                | "wav"
                | "zip"
                | "tar"
                | "gz"
                | "log"
                | "wasm"
                | "env"
                | "ini"
        )
}

/// End byte of a raw resource at a prose boundary. Uses no filesystem/network IO.
pub(crate) fn raw_link_end(line: &str, start: usize) -> Option<usize> {
    if start > 0
        && !line[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || "(<\"'“‘".contains(c))
    {
        return None;
    }
    let rest = &line[start..];
    let end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || "<>\"'`“”‘’".contains(*c))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    let mut candidate = &rest[..end];
    // Keep balanced parentheses (common in URLs), but exclude sentence punctuation.
    loop {
        let last = candidate.chars().next_back()?;
        let trim = ".,;:!?".contains(last)
            || match last {
                ')' => candidate.matches(')').count() > candidate.matches('(').count(),
                ']' => candidate.matches(']').count() > candidate.matches('[').count(),
                '}' => candidate.matches('}').count() > candidate.matches('{').count(),
                _ => false,
            };
        if !trim {
            break;
        }
        candidate = &candidate[..candidate.len() - last.len_utf8()];
    }
    if matches!(candidate, "/" | "./" | "../" | "~/") {
        return None;
    }
    let resource = Resource::parse(candidate)?;
    // A browser has no home directory, but can still recognize and highlight ~/.
    if !candidate.starts_with("~/") && resource.url(Path::new("/workspace/note.wtf")).is_err() {
        return None;
    }
    Some(start + candidate.len())
}
/// `just now`, `5m ago`, `2h ago`, `3d ago`, `2w ago`.
pub fn ago(from: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - from).num_seconds().max(0);
    match seconds {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s if s < 14 * 86_400 => format!("{}d ago", s / 86_400),
        s => format!("{}w ago", s / (7 * 86_400)),
    }
}
impl Metadata {
    /// A compact, scannable status: `✓ merged · ● checks · ✓ approved · 2h ago`.
    /// Failing checks and requested changes shout in caps; stale caches say so.
    pub fn badge(&self, now: DateTime<Utc>) -> String {
        use crate::glyphs::*;
        let state = match self.state.as_str() {
            "merged" => format!("{DONE} merged"),
            "open" => format!("{OFF} open"),
            "draft" => format!("{HALF} draft"),
            "closed" => format!("{FAIL} closed"),
            other => other.to_string(),
        };
        let mut parts = vec![state];
        if let Some(checks) = &self.checks {
            parts.push(match checks.as_str() {
                "passing" => format!("{ON} checks"),
                "failing" => format!("{FAIL} checks FAILING"),
                _ => format!("{PENDING} checks pending"),
            });
        }
        if let Some(review) = &self.review
            && !review.is_empty()
        {
            parts.push(match review.as_str() {
                "APPROVED" => format!("{DONE} approved"),
                "CHANGES_REQUESTED" => format!("{FLAG} CHANGES REQUESTED"),
                "REVIEW_REQUIRED" => format!("{FLAG} review needed"),
                other => other.to_lowercase().replace('_', " "),
            });
        }
        let age = ago(self.fetched_at, now);
        parts.push(if (now - self.fetched_at).num_days() >= 7 {
            format!("{ALERT} stale · {age}")
        } else {
            age
        });
        parts.join(" · ")
    }
    pub fn summary(&self) -> String {
        let mut s = self.state.clone();
        if let Some(checks) = &self.checks {
            s.push_str(&format!(" · checks {checks}"));
        }
        if let Some(review) = &self.review
            && !review.is_empty()
        {
            s.push_str(&format!(" · {}", review.to_lowercase().replace('_', " ")));
        }
        // Always identify cached information; never suggest it is a fresh remote observation.
        s.push_str(&format!(
            " · cached {}",
            self.fetched_at.format("%Y-%m-%d %H:%M UTC")
        ));
        s
    }
}
pub fn github(target: &str) -> Option<(String, String, String)> {
    let url = Url::parse(target).ok()?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return None;
    }
    let p: Vec<_> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if p.len() != 4 || !matches!(p[2], "pull" | "issues" | "commit") {
        return None;
    }
    if !p.iter().all(|s| {
        s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    }) {
        return None;
    }
    if p[2] != "commit" && !p[3].bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((format!("{}/{}", p[0], p[1]), p[2].into(), p[3].into()))
}
#[cfg(feature = "native")]
pub fn load_cache(root: &Path) -> Cache {
    std::fs::read(root.join(".wtf/cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
#[cfg(feature = "native")]
pub async fn fetch(target: &str) -> Result<Metadata, String> {
    let (repo, kind, id) = github(target).ok_or("Not a supported GitHub link")?;
    let args = match kind.as_str() {
        "pull" => vec![
            "pr".into(),
            "view".into(),
            id,
            "--repo".into(),
            repo,
            "--json".into(),
            "title,state,isDraft,mergedAt,reviewDecision,statusCheckRollup".into(),
        ],
        "issues" => vec![
            "issue".into(),
            "view".into(),
            id,
            "--repo".into(),
            repo,
            "--json".into(),
            "title,state".into(),
        ],
        _ => vec!["api".into(), format!("repos/{repo}/commits/{id}")],
    };
    let mut command = tokio::process::Command::new("gh");
    command
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(20), command.output())
        .await
        .map_err(|_| "GitHub request timed out")?
        .map_err(|e| format!("Cannot run gh: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "gh failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let data: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    metadata(&kind, &data, Utc::now())
}
pub fn metadata(kind: &str, data: &Value, now: DateTime<Utc>) -> Result<Metadata, String> {
    let title = if kind == "commit" {
        data["commit"]["message"].as_str()
    } else {
        data["title"].as_str()
    }
    .ok_or("Missing GitHub title")?
    .lines()
    .next()
    .unwrap_or("")
    .to_string();
    let merged =
        (kind == "pull").then(|| data["state"] == "MERGED" || data["mergedAt"].as_str().is_some());
    let state = if kind == "commit" {
        "commit".into()
    } else if merged == Some(true) {
        "merged".into()
    } else if data["isDraft"] == true {
        "draft".into()
    } else {
        data["state"].as_str().unwrap_or("unknown").to_lowercase()
    };
    let checks = data["statusCheckRollup"]
        .as_array()
        .filter(|a| !a.is_empty())
        .map(|checks| {
            let statuses: Vec<_> = checks
                .iter()
                .map(|c| {
                    c["conclusion"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or(c["state"].as_str())
                        .unwrap_or("PENDING")
                })
                .collect();
            if statuses.iter().any(|s| {
                matches!(
                    *s,
                    "FAILURE"
                        | "ERROR"
                        | "CANCELLED"
                        | "TIMED_OUT"
                        | "ACTION_REQUIRED"
                        | "STARTUP_FAILURE"
                        | "STALE"
                )
            }) {
                "failing"
            } else if statuses
                .iter()
                .all(|s| matches!(*s, "SUCCESS" | "NEUTRAL" | "SKIPPED"))
            {
                "passing"
            } else {
                "pending"
            }
            .to_string()
        });
    Ok(Metadata {
        title,
        state,
        merged,
        checks,
        review: data["reviewDecision"].as_str().map(str::to_owned),
        fetched_at: now,
    })
}
