//! Compatibility entry points. All GitHub policy lives in the bundled WTF module.
use crate::{
    engine::Value,
    link_features::{LinkContext, LinkFeature, RefreshRequest},
    modules::{Module, from_json, url_value},
    resources::Metadata,
};
use chrono::{DateTime, Utc};
use lsp_types::Url;

fn module() -> &'static Module {
    crate::modules::bundled()
        .iter()
        .find(|m| m.id == "github")
        .expect("bundled GitHub module")
}
pub struct GitHub;
impl LinkFeature for GitHub {
    fn id(&self) -> &str {
        "github"
    }
    fn matches(&self, url: &Url) -> bool {
        module().matches(url)
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        module().inlay(ctx)
    }
    fn hover(&self, ctx: &LinkContext<'_>) -> Option<String> {
        module().hover(ctx)
    }
    fn time_dependent(&self, ctx: &LinkContext<'_>) -> bool {
        module().time_dependent(ctx)
    }
    fn cache_namespace(&self) -> Option<&str> {
        module().cache_namespace()
    }
    fn property_names(&self, url: &Url) -> Vec<String> {
        module().property_names(url)
    }
    fn property(&self, ctx: &LinkContext<'_>, name: &str) -> crate::error::EvalResult<Value> {
        module().property(ctx, name)
    }
    fn refresh_request(&self, url: &Url) -> Option<RefreshRequest> {
        if !self.matches(url) {
            return None;
        }
        module().refresh_request(url)
    }
    fn decode_refresh(
        &self,
        url: &Url,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> crate::error::EvalResult<Metadata> {
        if !self.matches(url) {
            return Err(crate::error::EvalError::Message(
                "Not a supported GitHub link".into(),
            ));
        }
        module().decode_refresh(url, data, now)
    }
}
impl Metadata {
    fn render(&self, function: &str, now: DateTime<Utc>) -> String {
        module()
            .call(
                function,
                vec![
                    from_json(&serde_json::to_value(self).expect("metadata serializes")),
                    Value::DateTime(self.fetched_at.fixed_offset()),
                ],
                now.fixed_offset(),
            )
            .map(|v| v.display())
            .unwrap_or_else(|e| e.to_string())
    }
    pub fn badge(&self, now: DateTime<Utc>) -> String {
        self.render("badge", now)
    }
    pub fn summary(&self) -> String {
        self.render("summary", self.fetched_at)
    }
}
pub fn parse(target: &str) -> Option<(String, String, String)> {
    let url = Url::parse(target).ok()?;
    if !module().matches(&url) {
        return None;
    }
    let Value::List(parts) = module()
        .call(
            "segments",
            vec![url_value(&url)],
            DateTime::<Utc>::UNIX_EPOCH.fixed_offset(),
        )
        .ok()?
    else {
        return None;
    };
    Some((
        format!("{}/{}", parts[0].display(), parts[1].display()),
        parts[2].display(),
        parts[3].display(),
    ))
}
pub fn metadata(
    kind: &str,
    data: &serde_json::Value,
    now: DateTime<Utc>,
) -> Result<Metadata, String> {
    let url = Url::parse(&format!("https://github.com/compat/compat/{kind}/0"))
        .map_err(|e| e.to_string())?;
    module()
        .decode_refresh(&url, data, now)
        .map_err(|e| e.to_string())
}
