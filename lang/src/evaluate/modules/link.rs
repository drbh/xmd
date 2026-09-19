//! The adapter that presents a link module as an ordinary link feature.
use super::{
    Hook, Module, ModuleKind, epoch, from_json, json, url_value,
    values::{strings, text},
};
use crate::{
    engine::Value,
    link_features::{LinkContext, LinkFeature, RefreshFormat, RefreshRequest},
    resources::Metadata,
};
use chrono::{DateTime, Utc};
use lsp_types::Url;

impl Module {
    fn context(&self, ctx: &LinkContext<'_>) -> Value {
        let cached = ctx
            .cached
            .filter(|m| m.provider.as_deref() == self.cache_key.as_deref());
        super::record([
            ("url".into(), url_value(ctx.url)),
            ("native".into(), Value::Bool(!cfg!(target_arch = "wasm32"))),
            (
                "cached".into(),
                cached
                    .map(|m| {
                        from_json(&m.data.clone().unwrap_or_else(|| {
                            serde_json::to_value(m).expect("metadata serializes")
                        }))
                    })
                    .unwrap_or(Value::Null),
            ),
            (
                "fetched_at".into(),
                cached
                    .map(|m| Value::DateTime(m.fetched_at.fixed_offset()))
                    .unwrap_or(Value::Null),
            ),
        ])
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
                .unwrap_or_else(|e| e)
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
    fn property(&self, ctx: &LinkContext<'_>, name: &str) -> Result<Value, String> {
        if !self.property_names(ctx.url).iter().any(|p| p == name) {
            return Err(format!("Unknown resource property '{name}'"));
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
        let Value::Record(fields) = self
            .call(Hook::Refresh, vec![url_value(url)], epoch())
            .ok()?
        else {
            return None;
        };
        let program = text(fields.get("program")?).ok()?;
        let program = if program.starts_with("./") || program.starts_with("../") {
            self.path
                .parent()?
                .join(program)
                .to_string_lossy()
                .into_owned()
        } else {
            program
        };
        Some(RefreshRequest {
            title: fields
                .get("title")
                .map(text)
                .transpose()
                .ok()?
                .unwrap_or_else(|| "Module refresh".into()),
            program,
            args: strings(fields.get("args")?).ok()?,
            env: match fields.get("env") {
                None => vec![],
                Some(Value::Record(env)) => env
                    .iter()
                    .map(|(k, v)| text(v).map(|v| (k.clone(), v)))
                    .collect::<Result<_, _>>()
                    .ok()?,
                _ => return None,
            },
            format: match fields.get("format") {
                None => RefreshFormat::default(),
                Some(value) => text(value).ok()?.parse().ok()?,
            },
        })
    }
    fn decode_refresh(
        &self,
        url: &Url,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Metadata, String> {
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
