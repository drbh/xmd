use crate::link_features_impl::{self, LinkFeatures};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
// What a resource target is and where it points is `common::Resource`, since
// a note's literal parser needs it too; fetching, caching and presenting one
// is what this module adds.
pub use common::Resource;
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
pub(crate) type Cache = BTreeMap<String, Metadata>;

/// Common presentation for a URL or local resource in any syntactic position.
pub struct ResourcePresentation {
    pub label: String,
    pub hover: String,
    pub known_link: bool,
    pub time_dependent: bool,
}

/// The runtime half of a resource: fetching, caching and presenting it. What
/// it is and where it points is `common::Resource`, defined next to the
/// literal parser that needs it too. `Resource` is foreign to this crate, so
/// these methods are an extension trait rather than an inherent impl.
pub trait ResourcePresenting {
    /// Resolve provider semantics once for both the inline label and tooltip.
    fn presentation(
        &self,
        document: &Path,
        cache: &Cache,
        now: DateTime<Utc>,
        features: LinkFeatures<'_>,
    ) -> ResourcePresentation;
    fn label(&self, cache: &Cache, now: DateTime<Utc>) -> String;
    fn hover(&self, document: &Path, cache: &Cache) -> String;
    fn hover_at(&self, document: &Path, cache: &Cache, now: DateTime<Utc>) -> String;
}
impl ResourcePresenting for Resource {
    fn presentation(
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
            .unwrap_or_else(|| fallback_label(self));
        let mut hover = open_hover(self, document);
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
    fn label(&self, cache: &Cache, now: DateTime<Utc>) -> String {
        link_features_impl::BUILTINS
            .presentation(&self.target, cache, now)
            .map(|p| p.label)
            .unwrap_or_else(|| fallback_label(self))
    }
    fn hover(&self, document: &Path, cache: &Cache) -> String {
        self.hover_at(document, cache, Utc::now())
    }
    fn hover_at(&self, document: &Path, cache: &Cache, now: DateTime<Utc>) -> String {
        self.presentation(document, cache, now, link_features_impl::BUILTINS)
            .hover
    }
}
fn fallback_label(resource: &Resource) -> String {
    if resource.target.starts_with("geo:") {
        "place · open map".into()
    } else if resource.is_image() {
        "image · open preview".into()
    } else if resource.target.starts_with("http") {
        "link".into()
    } else {
        "file".into()
    }
}
fn open_hover(resource: &Resource, document: &Path) -> String {
    let mut out = match resource.url(document) {
        Ok(url) => format!(
            "[Open {}](<{}>)",
            if resource.target.starts_with("geo:") {
                "map"
            } else {
                "resource"
            },
            url
        ),
        Err(err) => err.to_string(),
    };
    if resource.is_image()
        && let Ok(url) = resource.url(document)
    {
        out.push_str(&format!("\n\n![Preview](<{url}>)"));
    }
    out
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
#[cfg(feature = "native")]
pub(crate) fn load_cache(root: &Path) -> Cache {
    std::fs::read(root.join(".wtf/cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
