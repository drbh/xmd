//! The adapter that presents a link module as an ordinary link feature.
use super::{
    Hook, Module, ModuleKind, epoch, from_json, json, url_value,
    values::{strings, text},
};
use crate::error::{EvalError, EvalResult, PropertyOwner};
use crate::{
    engine_impl::Value,
    link_features_impl::{LinkContext, LinkFeature, RefreshFormat, RefreshRequest},
    records::{FromValue, LinkContextRecord, RefreshRecord, ToValue, UrlRecord},
    resources_impl::Metadata,
};
use chrono::{DateTime, Utc};
use url::Url;

impl Module {
    fn context(&self, ctx: &LinkContext<'_>) -> Value {
        let cached = ctx
            .cached
            .filter(|m| m.provider.as_deref() == self.cache_key.as_deref());
        LinkContextRecord {
            url: UrlRecord::from(ctx.url),
            native: !cfg!(target_arch = "wasm32"),
            cached: cached
                .map(|m| {
                    from_json(
                        &m.data.clone().unwrap_or_else(|| {
                            serde_json::to_value(m).expect("metadata serializes")
                        }),
                    )
                })
                .unwrap_or(Value::Null),
            fetched_at: cached.map(|m| m.fetched_at.fixed_offset()),
        }
        .to_value()
    }
}
impl LinkFeature for Module {
    fn id(&self) -> &str {
        &self.id
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
                || matches!(
                    self.call(Hook::Matches, vec![url_value(url)], epoch()),
                    Ok(Value::Bool(true))
                ))
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        self.call(Hook::Inlay, vec![self.context(ctx)], ctx.now.fixed_offset())
            .and_then(|v| text(&v))
            .unwrap_or_else(|e| format!("module error · {e}"))
    }
    fn hover(&self, ctx: &LinkContext<'_>) -> Option<String> {
        self.has(Hook::Hover).then(|| {
            self.call(Hook::Hover, vec![self.context(ctx)], ctx.now.fixed_offset())
                .and_then(|v| text(&v))
                .unwrap_or_else(|e| e.to_string())
        })
    }
    fn time_dependent(&self, ctx: &LinkContext<'_>) -> bool {
        if self.has(Hook::TimeDependent) {
            return !matches!(
                self.call(
                    Hook::TimeDependent,
                    vec![self.context(ctx)],
                    ctx.now.fixed_offset()
                ),
                Ok(Value::Bool(false))
            );
        }
        self.live
    }
    fn cache_namespace(&self) -> Option<&str> {
        self.cache_key.as_deref()
    }
    fn property_names(&self, url: &Url) -> Vec<String> {
        if self.has(Hook::PropertyNames) {
            return self
                .call(Hook::PropertyNames, vec![url_value(url)], epoch())
                .and_then(|v| strings(&v))
                .unwrap_or_default()
                .into_iter()
                .filter(|p| self.properties.contains(p))
                .collect();
        }
        self.properties.clone()
    }
    fn property(&self, ctx: &LinkContext<'_>, name: &str) -> EvalResult<Value> {
        if !self.property_names(ctx.url).iter().any(|p| p == name) {
            return Err(EvalError::UnknownProperty {
                owner: PropertyOwner::Resource,
                name: name.into(),
            });
        }
        self.call(
            Hook::Property,
            vec![self.context(ctx), Value::Text(name.into())],
            ctx.now.fixed_offset(),
        )
    }
    fn refresh_request(&self, url: &Url) -> Option<RefreshRequest> {
        if !self.has(Hook::Refresh) {
            return None;
        }
        // A request is data. The native host alone executes it on explicit refresh.
        let request = RefreshRecord::from_value(
            &self
                .call(Hook::Refresh, vec![url_value(url)], epoch())
                .ok()?,
        )
        .ok()?;
        // A relative program is relative to the module that asked for it.
        let program = if request.program.starts_with("./") || request.program.starts_with("../") {
            self.path
                .parent()?
                .join(request.program)
                .to_string_lossy()
                .into_owned()
        } else {
            request.program
        };
        Some(RefreshRequest {
            title: request.title.unwrap_or_else(|| "Module refresh".into()),
            program,
            args: request.args,
            env: request.env.unwrap_or_default().into_iter().collect(),
            format: match request.format {
                None => RefreshFormat::default(),
                Some(format) => format.parse().ok()?,
            },
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
            return Err(EvalError::Message("decode must return a record".into()));
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
