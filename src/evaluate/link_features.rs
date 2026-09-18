//! Optional URL semantics, adapted into the common inlay pipeline by LinkInlays.
//! Implementations are pure; native hosts execute explicitly requested refreshes.
use crate::{
    engine::Value,
    resources::{Cache, Metadata},
};
use chrono::{DateTime, Utc};
use lsp_types::Url;

pub struct LinkContext<'a> {
    pub url: &'a Url,
    pub cached: Option<&'a Metadata>,
    pub now: DateTime<Utc>,
}
/// The second extension point: semantics of a recognized URL, independent of where
/// it appears. The adapter covers raw/Markdown links, definitions and references.
/// Only matching and an inlay label are required. Other capabilities are optional.
pub trait LinkFeature: Send + Sync {
    fn matches(&self, url: &Url) -> bool;
    fn inlay(&self, context: &LinkContext<'_>) -> String;
    fn hover(&self, _context: &LinkContext<'_>) -> Option<String> {
        None
    }
    /// Whether presentation or properties depend on the request clock.
    fn time_dependent(&self, _context: &LinkContext<'_>) -> bool {
        false
    }
    fn property_names(&self, _url: &Url) -> Vec<String> {
        vec![]
    }
    fn property(&self, _context: &LinkContext<'_>, name: &str) -> Result<Value, String> {
        Err(format!("Unknown resource property '{name}'"))
    }
    fn cache_namespace(&self) -> Option<&str> {
        None
    }
    /// Return a command specification, never execute it while rendering a feature.
    fn refresh_request(&self, _url: &Url) -> Option<RefreshRequest> {
        None
    }
    fn decode_refresh(
        &self,
        _url: &Url,
        _data: &serde_json::Value,
        _now: DateTime<Utc>,
    ) -> Result<Metadata, String> {
        Err("This link feature does not support refresh".into())
    }
}

pub struct LinkPresentation {
    pub label: String,
    pub hover: Option<String>,
    pub time_dependent: bool,
}
/// A program and distinct arguments, not a shell expression. Cached status is
/// replaced only after the process succeeds and the provider decodes its JSON.
pub struct RefreshRequest {
    pub title: &'static str,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// First matching registration wins. No mutable process-global plugin state.
#[derive(Clone, Copy)]
pub struct LinkFeatures<'a> {
    features: &'a [&'a dyn LinkFeature],
    plugins: &'a [crate::evaluate::plugins::Module],
}
pub const BUILTINS: LinkFeatures<'static> = LinkFeatures::new(&[&crate::github::GitHub]);
impl<'a> LinkFeatures<'a> {
    pub const fn new(features: &'a [&'a dyn LinkFeature]) -> Self {
        Self {
            features,
            plugins: &[],
        }
    }
    pub fn with_plugins(mut self, plugins: &'a [crate::evaluate::plugins::Module]) -> Self {
        self.plugins = plugins;
        self
    }
    fn matching(&self, target: &str) -> Option<(&'a dyn LinkFeature, Url)> {
        let url = Url::parse(target).ok()?;
        self.plugins
            .iter()
            .map(|p| p as &dyn LinkFeature)
            .chain(self.features.iter().copied())
            .find(|feature| feature.matches(&url))
            .map(|feature| (feature, url))
    }
    pub fn presentation(
        &self,
        target: &str,
        cache: &Cache,
        now: DateTime<Utc>,
    ) -> Option<LinkPresentation> {
        let (feature, url) = self.matching(target)?;
        let context = LinkContext {
            url: &url,
            cached: cache
                .get(target)
                .filter(|m| m.provider.as_deref() == feature.cache_namespace()),
            now,
        };
        Some(LinkPresentation {
            label: feature.inlay(&context),
            hover: feature.hover(&context),
            time_dependent: feature.time_dependent(&context),
        })
    }
    pub fn property_names(&self, target: &str) -> Vec<String> {
        self.matching(target)
            .map(|(feature, url)| feature.property_names(&url))
            .unwrap_or_default()
    }
    pub fn time_dependent(&self, target: &str, cache: &Cache, now: DateTime<Utc>) -> bool {
        self.matching(target).is_some_and(|(feature, url)| {
            feature.time_dependent(&LinkContext {
                url: &url,
                cached: cache
                    .get(target)
                    .filter(|m| m.provider.as_deref() == feature.cache_namespace()),
                now,
            })
        })
    }
    pub fn property(
        &self,
        target: &str,
        cache: &Cache,
        now: DateTime<Utc>,
        name: &str,
    ) -> Result<Value, String> {
        let (feature, url) = self
            .matching(target)
            .ok_or_else(|| format!("Unknown resource property '{name}'"))?;
        feature.property(
            &LinkContext {
                url: &url,
                cached: cache
                    .get(target)
                    .filter(|m| m.provider.as_deref() == feature.cache_namespace()),
                now,
            },
            name,
        )
    }
    pub fn refresh_request(&self, target: &str) -> Option<RefreshRequest> {
        let (feature, url) = self.matching(target)?;
        feature.refresh_request(&url)
    }
    pub fn decode_refresh(
        &self,
        target: &str,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Metadata, String> {
        let (feature, url) = self
            .matching(target)
            .ok_or("No feature recognizes this link")?;
        feature.decode_refresh(&url, data, now)
    }
    #[cfg(feature = "native")]
    pub async fn fetch(&self, target: &str) -> Result<Metadata, String> {
        let (feature, url) = self
            .matching(target)
            .ok_or("No feature recognizes this link")?;
        let request = feature
            .refresh_request(&url)
            .ok_or("This link feature does not support refresh")?;
        let mut command = tokio::process::Command::new(&request.program);
        command
            .args(&request.args)
            .envs(request.env)
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(20), command.output())
            .await
            .map_err(|_| format!("{} timed out", request.title))?
            .map_err(|e| format!("Cannot run {}: {e}", request.program))?;
        if !output.status.success() {
            return Err(format!(
                "{} failed: {}",
                request.program,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let data = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
        feature.decode_refresh(&url, &data, Utc::now())
    }
}
