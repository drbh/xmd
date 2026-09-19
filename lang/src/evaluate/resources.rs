use crate::error::{EvalError, EvalResult};
use crate::link_features::{self, LinkFeatures};
use chrono::{DateTime, Utc};
use lsp_types::Url;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub target: String,
    pub origin: Option<PathBuf>,
}
/// One cached link status, as `.wtf/cache.json` stores it.
///
/// `data` is what the link module's `decode` returned, and is the whole truth
/// about a link: modules see it as `ctx.cached`, and each names its own fields.
///
/// The five fields above it are the legacy GitHub projection. They predate link
/// modules, when every cache entry was a pull request, and they remain because
/// caches written back then have no `data` at all: `Module::context` falls back
/// to serializing this struct, which is the only way those entries still render.
/// Nothing in Rust reads them by name; they are the file format, not an API.
/// Removing them would silently blank the labels of every pre-module cache.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(default)]
    pub title: String,
    /// Provider-defined, so it stays text: each link provider names its own
    /// states, and the workspace only passes them through.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub merged: Option<bool>,
    #[serde(default)]
    pub checks: Option<String>,
    #[serde(default)]
    pub review: Option<String>,
    pub fetched_at: DateTime<Utc>,
    /// Which link module wrote this entry; `None` is the GitHub projection above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
pub type Cache = BTreeMap<String, Metadata>;

/// Common presentation for a URL or local resource in any syntactic position.
pub struct ResourcePresentation {
    pub label: String,
    pub hover: String,
    pub known_link: bool,
    pub time_dependent: bool,
}

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
    pub fn url(&self, document: &Path) -> EvalResult<Url> {
        let document = self.origin.as_deref().unwrap_or(document);
        if let Some(coords) = self.target.strip_prefix("geo:") {
            let (lat, lon) = coords
                .split_once(',')
                .ok_or(EvalError::Expected("geo:latitude,longitude"))?;
            let lat: f64 = lat
                .parse()
                .map_err(|_| EvalError::Message("Invalid latitude".into()))?;
            let lon: f64 = lon
                .parse()
                .map_err(|_| EvalError::Message("Invalid longitude".into()))?;
            if !lat.is_finite()
                || !lon.is_finite()
                || !(-90.0..=90.0).contains(&lat)
                || !(-180.0..=180.0).contains(&lon)
            {
                return Err(EvalError::Message("Coordinates are out of range".into()));
            }
            return Url::parse(&format!(
                "https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}"
            ))
            .map_err(|e| EvalError::Message(e.to_string()));
        }
        if self.target.starts_with("http://")
            || self.target.starts_with("https://")
            || self.target.starts_with("file://")
        {
            // A malformed URL is an error, never a relative file path.
            return Url::parse(&self.target).map_err(|e| EvalError::Message(e.to_string()));
        }
        if let Ok(url) = Url::parse(&self.target) {
            if matches!(url.scheme(), "https" | "http" | "file") {
                return Ok(url);
            }
            return Err(EvalError::Message("Unsupported link scheme".into()));
        }
        if let Some(relative) = self.target.strip_prefix("~/") {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let home_dir = std::env::var_os("HOME")
                    .ok_or(EvalError::Message("Home directory is unavailable".into()))?;
                return resolved_file_url(&PathBuf::from(home_dir).join(relative));
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = relative;
                return Err(EvalError::Message(
                    "Home-directory paths can be opened in the native editor, not the browser"
                        .into(),
                ));
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
    /// Resolve provider semantics once for both the inline label and tooltip.
    pub fn presentation(
        &self,
        document: &Path,
        cache: &Cache,
        now: DateTime<Utc>,
        features: LinkFeatures<'_>,
    ) -> ResourcePresentation {
        let known = features.presentation(&self.target, cache, now);
        let label = known
            .as_ref()
            .map(|p| p.label.clone())
            .unwrap_or_else(|| self.fallback_label());
        let mut hover = self.open_hover(document);
        if let Some(details) = known.as_ref().and_then(|p| p.hover.as_ref()) {
            hover.push_str("\n\n");
            hover.push_str(details);
        }
        ResourcePresentation {
            label,
            hover,
            known_link: known.is_some(),
            time_dependent: known.is_some_and(|p| p.time_dependent),
        }
    }
    pub fn label(&self, cache: &Cache, now: DateTime<Utc>) -> String {
        link_features::BUILTINS
            .presentation(&self.target, cache, now)
            .map(|p| p.label)
            .unwrap_or_else(|| self.fallback_label())
    }
    fn fallback_label(&self) -> String {
        if self.target.starts_with("geo:") {
            "place · open map".into()
        } else if self.is_image() {
            "image · open preview".into()
        } else if self.target.starts_with("http") {
            "link".into()
        } else {
            "file".into()
        }
    }
    pub fn hover(&self, document: &Path, cache: &Cache) -> String {
        self.hover_at(document, cache, Utc::now())
    }
    pub fn hover_at(&self, document: &Path, cache: &Cache, now: DateTime<Utc>) -> String {
        self.presentation(document, cache, now, link_features::BUILTINS)
            .hover
    }
    fn open_hover(&self, document: &Path) -> String {
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
            Err(err) => err.to_string(),
        };
        if self.is_image()
            && let Ok(url) = self.url(document)
        {
            out.push_str(&format!("\n\n![Preview](<{url}>)"));
        }
        out
    }
}

fn resolved_file_url(path: &Path) -> EvalResult<Url> {
    let url = crate::paths::file_url(path)?;
    // from_file_path preserves dot segments; parsing normalizes them without IO.
    // Native and browser links must use the same canonical URI to find open notes.
    Url::parse(url.as_str()).map_err(|e| EvalError::Message(e.to_string()))
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
// Preserve the existing public helpers while keeping GitHub semantics in its provider.
pub use crate::github::{metadata, parse as github};
#[cfg(feature = "native")]
pub fn load_cache(root: &Path) -> Cache {
    std::fs::read(root.join(".wtf/cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
#[cfg(feature = "native")]
pub async fn fetch(target: &str) -> Result<Metadata, String> {
    link_features::BUILTINS.fetch(target).await
}
