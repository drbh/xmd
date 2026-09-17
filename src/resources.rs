use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tower_lsp::lsp_types::Url;

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
            || s.starts_with('/')
            || s.starts_with("file://"))
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
        if let Ok(url) = Url::parse(&self.target) {
            if matches!(url.scheme(), "https" | "http" | "file") {
                return Ok(url);
            }
            return Err("Unsupported link scheme".into());
        }
        let path = document
            .parent()
            .unwrap_or(Path::new("."))
            .join(&self.target);
        Url::from_file_path(&path).map_err(|_| "Cannot resolve file path".into())
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
    pub fn label(&self, cache: &Cache) -> String {
        if let Some(m) = cache.get(&self.target) {
            return m.summary();
        }
        if self.target.starts_with("geo:") {
            return "place · open map".into();
        }
        if github(&self.target).is_some() {
            return "GitHub · not refreshed".into();
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
                "\n\n{}\n\n{}\n\nLast refreshed: {}. Use **Refresh GitHub resources** to update.",
                m.title,
                m.summary(),
                m.fetched_at.to_rfc3339()
            ));
        } else if github(&self.target).is_some() {
            out.push_str("\n\nNo cached status. Run `jot refresh`, or use the Refresh GitHub resources code action. Requires the GitHub CLI (`gh`).");
        }
        out
    }
}
impl Metadata {
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
pub fn load_cache(root: &Path) -> Cache {
    std::fs::read(root.join(".jot/cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
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
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
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
