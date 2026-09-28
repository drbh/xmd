use crate::{
    engine_impl::{Engine, Value},
    modules_impl::record,
};
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
    /// A link module that recognizes the target words its label and details;
    /// the stdlib's `resource` module words everything else.
    fn presentation(&self, engine: &mut Engine<'_>, document: &Path) -> ResourcePresentation;
    /// What the `resource` module reads: the target, the URL it opens (or why
    /// it has none) and whether it previews as an image.
    fn record(&self, document: &Path) -> Value;
}
impl ResourcePresenting for Resource {
    fn presentation(&self, engine: &mut Engine<'_>, document: &Path) -> ResourcePresentation {
        let known = engine.link_features().presentation(
            &self.target,
            &engine.workspace().cache,
            engine.now().to_utc(),
        );
        let mut word = |name: &str| engine.present("resource", name, vec![self.record(document)]);
        let label = match &known {
            Some(p) => p.label.clone(),
            None => word("label"),
        };
        let mut hover = word("hover");
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
    fn record(&self, document: &Path) -> Value {
        let url = self.url(document);
        record([
            ("target".into(), Value::Text(self.target.clone())),
            (
                "url".into(),
                url.as_ref()
                    .map_or(Value::Null, |url| Value::Text(url.to_string())),
            ),
            (
                "error".into(),
                url.err()
                    .map_or(Value::Null, |e| Value::Text(e.to_string())),
            ),
            ("image".into(), Value::Bool(self.is_image())),
        ])
    }
}
#[cfg(feature = "native")]
pub(crate) fn load_cache(root: &Path) -> Cache {
    std::fs::read(root.join(".wtf/cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
