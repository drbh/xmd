//! Hot-reloadable WTF modules. Adapters use the ordinary expression evaluator.
use crate::{
    document::Document,
    engine::{Engine, Lexeme, Value},
    link_features::{LinkContext, LinkFeature, RefreshRequest},
    resources::Metadata,
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, Utc};
use lsp_types::Url;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Default)]
pub struct Plugins {
    pub modules: Vec<Module>,
}
#[derive(Clone, Debug)]
pub struct Module {
    pub id: String,
    pub kind: String,
    pub path: PathBuf,
    pub live: bool,
    hosts: Vec<String>,
    prefix: String,
    properties: Vec<String>,
    cache_key: String,
    workspace: Arc<Workspace>,
}
pub fn is_plugin_path(path: &Path) -> bool {
    path.extension().is_some_and(|s| s == "wtf")
        && path.parent().is_some_and(|p| {
            p.file_name().is_some_and(|s| s == "plugins")
                && p.parent()
                    .is_some_and(|p| p.file_name().is_some_and(|s| s == ".wtf"))
        })
}
fn epoch() -> DateTime<FixedOffset> {
    DateTime::<Utc>::UNIX_EPOCH.fixed_offset()
}
fn text(value: &Value) -> Result<String, String> {
    if let Value::Text(s) = value {
        Ok(s.clone())
    } else {
        Err("Expected text".into())
    }
}
fn strings(value: &Value) -> Result<Vec<String>, String> {
    if let Value::List(items) = value {
        items.iter().map(text).collect()
    } else {
        Err("Expected a list of text".into())
    }
}
pub fn record(fields: impl IntoIterator<Item = (String, Value)>) -> Value {
    Value::Record(fields.into_iter().collect())
}
pub fn from_json(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(v) => Value::Bool(*v),
        serde_json::Value::Number(v) => Value::Number(v.as_f64().unwrap_or_default()),
        serde_json::Value::String(v) => Value::Text(v.clone()),
        serde_json::Value::Array(v) => Value::List(v.iter().map(from_json).collect()),
        serde_json::Value::Object(v) => record(v.iter().map(|(k, v)| (k.clone(), from_json(v)))),
    }
}
pub fn json(value: &Value) -> Result<serde_json::Value, String> {
    Ok(match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(v) => (*v).into(),
        Value::Number(v) => serde_json::Number::from_f64(*v)
            .ok_or("Nonfinite plugin number")?
            .into(),
        Value::Count(v) => (*v).into(),
        Value::Text(v) => v.clone().into(),
        Value::List(v) => serde_json::Value::Array(v.iter().map(json).collect::<Result<_, _>>()?),
        Value::Record(v) => serde_json::Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), json(v)?)))
                .collect::<Result<_, String>>()?,
        ),
        _ => return Err("Cached plugin data must contain JSON values".into()),
    })
}
fn url_value(url: &Url) -> Value {
    record([
        ("raw".into(), Value::Text(url.to_string())),
        (
            "host".into(),
            Value::Text(url.host_str().unwrap_or_default().into()),
        ),
        ("path".into(), Value::Text(url.path().into())),
        ("scheme".into(), Value::Text(url.scheme().into())),
    ])
}
impl Module {
    pub fn compile(path: PathBuf, source: String) -> Result<Self, String> {
        if source.len() > 65_536 {
            return Err("Plugin modules are limited to 64 KiB".into());
        }
        let document = Document::parse(source);
        if let Some(problem) = document.problems.first() {
            return Err(problem.message.clone());
        }
        let mut names = BTreeSet::new();
        let mut live = false;
        for def in &document.definitions {
            if !names.insert(def.named.name.clone()) {
                return Err(format!("Duplicate definition '{}'", def.named.name));
            }
            if !def.expression {
                return Err("Plugin definitions must use :=".into());
            }
            crate::engine::Parser::parse(&def.source)
                .map_err(|e| format!("{}:{}: {e}", path.display(), def.value_span.line + 1))?;
            live |= crate::engine::lex(&def.source)?
                .iter()
                .any(|t| matches!(&t.kind,Lexeme::Name(n) if n=="now" || n=="today"));
        }
        let workspace = Arc::new(Workspace {
            roots: vec![path.parent().unwrap_or(Path::new(".")).into()],
            documents: [(path.clone(), document)].into(),
            cache: Default::default(),
            lookups: Default::default(),
            plugins: Default::default(),
        });
        let mut engine = Engine::at(&workspace, epoch()).pure();
        let Value::Record(config) = engine.named(&path, "plugin")? else {
            return Err("plugin must be a record".into());
        };
        if !matches!(config.get("api"),Some(Value::Number(n)) if *n==1.0) {
            return Err("plugin.api must be 1".into());
        }
        let id = text(config.get("id").ok_or("plugin.id is required")?)?;
        if id.is_empty()
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        {
            return Err("Invalid plugin id".into());
        }
        let kind = text(config.get("kind").ok_or("plugin.kind is required")?)?;
        let required = match kind.as_str() {
            "link" => "inlay",
            "inlay" => "collect",
            _ => return Err("plugin.kind must be link or inlay".into()),
        };
        let hosts = config
            .get("hosts")
            .map(strings)
            .transpose()?
            .unwrap_or_default();
        if kind == "link"
            && (hosts.is_empty()
                || hosts.iter().any(|host| {
                    Url::parse(&format!("https://{host}")).is_err()
                        || host.contains(['/', '?', '#', '@', ':'])
                        || host.to_lowercase() != *host
                }))
        {
            return Err("Link plugins require lowercase host names".into());
        }
        let prefix = config
            .get("path_prefix")
            .map(text)
            .transpose()?
            .unwrap_or_default();
        let properties = config
            .get("properties")
            .map(strings)
            .transpose()?
            .unwrap_or_default();
        if properties
            .iter()
            .any(|p| !crate::document::identifier(p) || matches!(p.as_str(), "url" | "exists"))
        {
            return Err("Invalid or reserved property name".into());
        }
        for (name, arity) in [
            (required, 1),
            ("hover", 1),
            ("property", 2),
            ("refresh", 1),
            ("decode", 2),
        ] {
            if names.contains(name) {
                if !matches!(engine.named(&path,name)?,Value::Function(f) if f.params.len()==arity)
                {
                    return Err(format!("{name} must be a function with {arity} parameters"));
                }
            } else if name == required {
                return Err(format!("Missing {required} function"));
            }
        }
        if names.contains("refresh") != names.contains("decode") {
            return Err("refresh and decode must be supplied together".into());
        }
        if !properties.is_empty() && !names.contains("property") {
            return Err("Declared properties need a property function".into());
        }
        let version = match config.get("cache_version") {
            None => "1".into(),
            Some(Value::Number(n)) if *n >= 1.0 && n.fract() == 0.0 => n.to_string(),
            _ => return Err("cache_version must be a positive integer".into()),
        };
        let cache_key = format!("{id}:{version}");
        Ok(Self {
            id,
            kind,
            path,
            live,
            hosts,
            prefix,
            properties,
            cache_key,
            workspace,
        })
    }
    pub fn has(&self, name: &str) -> bool {
        self.workspace.resolve(&self.path, name).is_ok()
    }
    pub fn call(
        &self,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> Result<Value, String> {
        for arg in &args {
            crate::evaluate::functional::check_size(arg)?;
        }
        let mut engine = Engine::at(&self.workspace, now).pure();
        let function = engine.named(&self.path, name)?;
        engine
            .call(function, args)
            .map_err(|e| format!("Plugin {}.{name}: {e}", self.id))
    }
    fn context(&self, ctx: &LinkContext<'_>) -> Value {
        let cached = ctx
            .cached
            .filter(|m| m.provider.as_deref() == Some(&self.cache_key));
        record([
            ("url".into(), url_value(ctx.url)),
            (
                "cached".into(),
                cached
                    .and_then(|m| m.data.as_ref())
                    .map(from_json)
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
    fn matches(&self, url: &Url) -> bool {
        self.kind == "link"
            && matches!(url.scheme(), "http" | "https")
            && url
                .host_str()
                .is_some_and(|host| self.hosts.iter().any(|h| h == host))
            && url.path().starts_with(&self.prefix)
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        self.call("inlay", vec![self.context(ctx)], ctx.now.fixed_offset())
            .and_then(|v| text(&v))
            .unwrap_or_else(|e| format!("plugin error · {e}"))
    }
    fn hover(&self, ctx: &LinkContext<'_>) -> Option<String> {
        self.has("hover").then(|| {
            self.call("hover", vec![self.context(ctx)], ctx.now.fixed_offset())
                .and_then(|v| text(&v))
                .unwrap_or_else(|e| e)
        })
    }
    fn time_dependent(&self, _: &LinkContext<'_>) -> bool {
        self.live
    }
    fn cache_namespace(&self) -> Option<&str> {
        Some(&self.cache_key)
    }
    fn property_names(&self, _: &Url) -> Vec<String> {
        self.properties.clone()
    }
    fn property(&self, ctx: &LinkContext<'_>, name: &str) -> Result<Value, String> {
        if !self.properties.iter().any(|p| p == name) {
            return Err(format!("Unknown resource property '{name}'"));
        }
        self.call(
            "property",
            vec![self.context(ctx), Value::Text(name.into())],
            ctx.now.fixed_offset(),
        )
    }
    fn refresh_request(&self, url: &Url) -> Option<RefreshRequest> {
        if !self.has("refresh") {
            return None;
        }
        // A request is data. The native host alone executes it on explicit refresh.
        let Value::Record(fields) = self.call("refresh", vec![url_value(url)], epoch()).ok()?
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
            title: "Plugin refresh",
            program,
            args: strings(fields.get("args")?).ok()?,
            env: vec![],
        })
    }
    fn decode_refresh(
        &self,
        url: &Url,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Metadata, String> {
        let value = self.call(
            "decode",
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
            merged: None,
            checks: None,
            review: None,
            fetched_at: now,
            provider: Some(self.cache_key.clone()),
            data: Some(data),
        })
    }
}
impl Plugins {
    pub fn same_sources(&self, other: &Self) -> bool {
        self.modules.len() == other.modules.len()
            && self.modules.iter().zip(&other.modules).all(|(a, b)| {
                a.path == b.path
                    && a.workspace.documents[&a.path].text == b.workspace.documents[&b.path].text
            })
    }
    /// Compile a complete replacement before the caller swaps its Arc snapshot.
    pub fn compile(sources: BTreeMap<PathBuf, String>) -> Result<Self, String> {
        if sources.len() > 64 {
            return Err("At most 64 plugin modules may be loaded".into());
        }
        let mut modules = Vec::new();
        let mut ids = BTreeSet::new();
        for (path, source) in sources {
            let module = Module::compile(path.clone(), source)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if !ids.insert(module.id.clone()) {
                return Err(format!("Duplicate plugin id '{}'", module.id));
            }
            modules.push(module);
        }
        Ok(Self { modules })
    }
    #[cfg(feature = "native")]
    pub fn load(roots: &[PathBuf]) -> Result<Self, String> {
        let mut sources = BTreeMap::new();
        for root in roots {
            let directory = root.join(".wtf/plugins");
            let entries = match std::fs::read_dir(&directory) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.to_string()),
            };
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if entry.file_type().map_err(|e| e.to_string())?.is_file()
                    && path.extension().is_some_and(|s| s == "wtf")
                {
                    if entry.metadata().map_err(|e| e.to_string())?.len() > 65_536 {
                        return Err(format!("{} exceeds 64 KiB", path.display()));
                    }
                    sources.insert(
                        path.clone(),
                        std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
                    );
                }
            }
        }
        Self::compile(sources)
    }
}
