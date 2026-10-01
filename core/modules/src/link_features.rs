//! Optional URL semantics, adapted into the common inlay pipeline by LinkInlays.
//! A link module supplies them; its hooks are pure, and native hosts execute
//! explicitly requested refreshes.
use crate::module::{Hook, Module, ModuleKind};
use chrono::FixedOffset;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;
use values::{EvalError, EvalResult};
use values::{Fields, FromValue, ToValue, Value, from_json, json, record};

/// What a link module's hooks see of a recognized URL: the URL, the status
/// cached under the module's own namespace, and the request clock.
struct LinkContext<'a> {
    url: Url,
    cached: Option<&'a Metadata>,
    now: DateTime<Utc>,
}

/// One cached link status, as `.xmd/cache.json` stores it.
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
#[non_exhaustive]
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

pub struct LinkPresentation {
    pub label: String,
    pub hover: Option<String>,
    pub time_dependent: bool,
}
/// A program and distinct arguments, not a shell expression. Cached status is
/// replaced only after the process succeeds and the provider decodes its JSON.
pub struct RefreshRequest {
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// How the host reads the program's output before `decode` sees it.
    pub format: RefreshFormat,
}

/// What the refreshed bytes are. A module decodes records, never markup, so any
/// format other than JSON names a conversion the host performs first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum RefreshFormat {
    /// The program prints JSON, which reaches `decode` as it stands.
    #[default]
    Json,
    /// The program prints an RSS or Atom document, which the host's feed parser
    /// turns into the JSON `decode` receives.
    Feed,
}

/// The link modules a request consults. First matching module wins. No
/// mutable process-global module state.
#[derive(Clone, Copy, Default)]
pub struct LinkFeatures<'a>(&'a [Module]);
impl<'a> LinkFeatures<'a> {
    pub(crate) const fn new(modules: &'a [Module]) -> Self {
        Self(modules)
    }
    fn matching(&self, target: &str) -> Option<(&'a Module, Url)> {
        let url = Url::parse(target).ok()?;
        self.0
            .iter()
            .find(|module| module.matches(&url))
            .map(|module| (module, url))
    }
    fn context<'c>(
        &self,
        target: &str,
        cache: &'c Cache,
        now: DateTime<Utc>,
    ) -> Option<(&'a Module, LinkContext<'c>)> {
        let (module, url) = self.matching(target)?;
        let cached = cache
            .get(target)
            .filter(|m| m.provider.as_deref() == module.cache_key.as_deref());
        Some((module, LinkContext { url, cached, now }))
    }
    pub fn presentation(
        &self,
        target: &str,
        cache: &Cache,
        now: DateTime<Utc>,
    ) -> Option<LinkPresentation> {
        let (module, context) = self.context(target, cache, now)?;
        Some(LinkPresentation {
            label: module.inlay(&context),
            hover: module.hover(&context),
            time_dependent: module.time_dependent(&context),
        })
    }
    pub fn property_names(&self, target: &str) -> Vec<String> {
        self.matching(target)
            .map(|(module, url)| module.property_names(&url))
            .unwrap_or_default()
    }
    pub fn time_dependent(&self, target: &str, cache: &Cache, now: DateTime<Utc>) -> bool {
        self.context(target, cache, now)
            .is_some_and(|(module, context)| module.time_dependent(&context))
    }
    pub fn property(
        &self,
        target: &str,
        cache: &Cache,
        now: DateTime<Utc>,
        name: &str,
    ) -> EvalResult<Value> {
        let found = self.context(target, cache, now);
        let (module, context) = found.ok_or_else(|| unknown_property(name))?;
        module.property(&context, name)
    }
    pub fn refresh_request(&self, target: &str) -> Option<RefreshRequest> {
        let (module, url) = self.matching(target)?;
        module.refresh_request(&url)
    }
    pub fn decode_refresh(
        &self,
        target: &str,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> EvalResult<Metadata> {
        let (module, url) = self
            .matching(target)
            .ok_or("No feature recognizes this link")?;
        module.decode_refresh(&url, data, now)
    }
}

/// A link module's hooks, called with the record shapes the module contract
/// names. Only matching and an inlay label are required.
impl Module {
    fn context(&self, ctx: &LinkContext<'_>) -> Value {
        LinkContextRecord {
            url: UrlRecord::from(&ctx.url),
            native: !cfg!(target_arch = "wasm32"),
            cached: ctx.cached.map_or(Value::Null, |m| {
                let legacy = || serde_json::to_value(m).expect("metadata serializes");
                from_json(&m.data.clone().unwrap_or_else(legacy))
            }),
            fetched_at: ctx.cached.map(|m| m.fetched_at.fixed_offset()),
        }
        .to_value()
    }
    fn matches(&self, url: &Url) -> bool {
        self.enabled
            && self.kind == ModuleKind::Link
            && matches!(url.scheme(), "http" | "https")
            // No declared hosts means the module's own `matches` decides, which
            // is how a feed module recognizes a shape rather than a site.
            && (self.hosts.is_empty()
                || url
                    .host_str()
                    .is_some_and(|host| self.hosts.iter().any(|h| h == host)))
            && url.path().starts_with(&self.prefix)
            && (!self.has(Hook::Matches)
                || matches!(self.on_url(Hook::Matches, url), Ok(Value::Bool(true))))
    }
    /// Call `hook` with the link's context, at the request's clock.
    fn on_link(&self, hook: Hook, ctx: &LinkContext<'_>) -> EvalResult<Value> {
        self.call(hook, vec![self.context(ctx)], ctx.now.fixed_offset())
    }
    /// Call `hook` with the URL alone, at no clock.
    fn on_url(&self, hook: Hook, url: &Url) -> EvalResult<Value> {
        self.call(hook, vec![url_value(url)], crate::module::no_clock())
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        self.on_link(Hook::Inlay, ctx)
            .and_then(|v| String::from_value(&v))
            .unwrap_or_else(|e| format!("module error · {e}"))
    }
    fn hover(&self, ctx: &LinkContext<'_>) -> Option<String> {
        self.has(Hook::Hover).then(|| {
            self.on_link(Hook::Hover, ctx)
                .and_then(|v| String::from_value(&v))
                .unwrap_or_else(|e| e.to_string())
        })
    }
    /// Whether presentation or properties depend on the request clock.
    fn time_dependent(&self, ctx: &LinkContext<'_>) -> bool {
        if self.has(Hook::TimeDependent) {
            return !matches!(
                self.on_link(Hook::TimeDependent, ctx),
                Ok(Value::Bool(false))
            );
        }
        self.live
    }
    fn property_names(&self, url: &Url) -> Vec<String> {
        if self.has(Hook::PropertyNames) {
            return self
                .on_url(Hook::PropertyNames, url)
                .and_then(|v| Vec::<String>::from_value(&v))
                .unwrap_or_default()
                .into_iter()
                .filter(|p| self.properties.contains(p))
                .collect();
        }
        self.properties.clone()
    }
    fn property(&self, ctx: &LinkContext<'_>, name: &str) -> EvalResult<Value> {
        if !self.property_names(&ctx.url).iter().any(|p| p == name) {
            return Err(unknown_property(name));
        }
        self.call(
            Hook::Property,
            vec![self.context(ctx), Value::Text(name.into())],
            ctx.now.fixed_offset(),
        )
    }
    /// Return a command specification, never execute it while rendering.
    fn refresh_request(&self, url: &Url) -> Option<RefreshRequest> {
        if !self.has(Hook::Refresh) {
            return None;
        }
        // A request is data: `{program, args, title?, env?, format?}`. The
        // native host alone executes it on explicit refresh.
        let request = self.on_url(Hook::Refresh, url).ok()?;
        let request = Fields::new(&request).ok()?;
        let program: String = request.required("program").ok()?;
        let title: Option<String> = request.present("title").ok()?;
        let env: Option<BTreeMap<String, String>> = request.present("env").ok()?;
        let format: Option<String> = request.present("format").ok()?;
        // A relative program is relative to the module that asked for it.
        let program = if program.starts_with("./") || program.starts_with("../") {
            let program = self.path.parent()?.join(program);
            program.to_string_lossy().into_owned()
        } else {
            program
        };
        Some(RefreshRequest {
            title: title.unwrap_or_else(|| "Module refresh".into()),
            program,
            args: request.required("args").ok()?,
            env: env.unwrap_or_default().into_iter().collect(),
            format: format
                .map(|format| format.parse())
                .transpose()
                .ok()?
                .unwrap_or_default(),
        })
    }
    fn decode_refresh(
        &self,
        url: &Url,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> EvalResult<Metadata> {
        let value = self.call(
            Hook::Decode,
            vec![url_value(url), from_json(data)],
            now.fixed_offset(),
        )?;
        let Value::Record(_) = &value else {
            return Err("decode must return a record".into());
        };
        let data = json(&value)?;
        Ok(Metadata {
            title: data["title"].as_str().unwrap_or_default().into(),
            state: data["state"].as_str().unwrap_or_default().into(),
            merged: data["merged"].as_bool(),
            checks: data["checks"].as_str().map(str::to_owned),
            review: data["review"].as_str().map(str::to_owned),
            fetched_at: now,
            provider: self.cache_key.clone(),
            data: Some(data),
        })
    }
}

/// A resource property no link module gives the URL.
fn unknown_property(name: &str) -> EvalError {
    let (owner, name) = (common::ValueType::Resource, name.into());
    EvalError::UnknownProperty { owner, name }
}

fn url_value(url: &Url) -> Value {
    UrlRecord::from(url).to_value()
}
record! {
    /// A URL, split the way a link module reads it.
    pub(crate) struct UrlRecord {
        pub raw: String,
        pub host: String,
        pub path: String,
        pub scheme: String,
    }
}
impl From<&url::Url> for UrlRecord {
    fn from(url: &url::Url) -> Self {
        Self {
            raw: url.to_string(),
            host: url.host_str().unwrap_or_default().into(),
            path: url.path().into(),
            scheme: url.scheme().into(),
        }
    }
}

record! {
    /// Everything a link module's hook is handed about one URL.
    pub(crate) struct LinkContextRecord {
        pub url: UrlRecord,
        /// False in the browser, where a refresh cannot run a program.
        pub native: bool,
        pub cached: Value,
        pub fetched_at: Option<DateTime<FixedOffset>>,
    }
}
