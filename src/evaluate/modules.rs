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

#[derive(Clone, Debug)]
pub struct ModuleRegistry {
    pub modules: Vec<Module>,
}
impl Default for ModuleRegistry {
    fn default() -> Self {
        Self {
            modules: bundled().to_vec(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Module {
    pub id: String,
    pub kind: String,
    pub path: PathBuf,
    pub live: bool,
    own_live: bool,
    pub enabled: bool,
    pub inputs: Vec<String>,
    pub fields: BTreeMap<String, Vec<String>>,
    pub imports: Vec<String>,
    pub(crate) expressions: Arc<BTreeMap<String, crate::engine::Expr>>,
    hosts: Vec<String>,
    prefix: String,
    properties: Vec<String>,
    cache_key: Option<String>,
    workspace: Arc<Workspace>,
}
pub fn is_module_path(path: &Path) -> bool {
    path.extension().is_some_and(|s| s == "wtf")
        && path.parent().is_some_and(|p| {
            p.file_name().is_some_and(|s| s == "stdlib")
                || (p.file_name().is_some_and(|s| s == "modules")
                    && p.parent()
                        .is_some_and(|p| p.file_name().is_some_and(|s| s == ".wtf")))
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
        Value::Number(v)
            if v.is_finite() && *v >= 0.0 && *v < u64::MAX as f64 && v.fract() == 0.0 =>
        {
            (*v as u64).into()
        }
        Value::Number(v) => serde_json::Number::from_f64(*v)
            .ok_or("Nonfinite module number")?
            .into(),
        Value::Count(v) => (*v).into(),
        Value::Text(v) => v.clone().into(),
        Value::List(v) => serde_json::Value::Array(v.iter().map(json).collect::<Result<_, _>>()?),
        Value::Record(v) => serde_json::Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), json(v)?)))
                .collect::<Result<_, String>>()?,
        ),
        _ => return Err("Cached module data must contain JSON values".into()),
    })
}
pub fn url_value(url: &Url) -> Value {
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
            return Err("Module modules are limited to 64 KiB".into());
        }
        let document = Document::parse(source);
        if let Some(problem) = document.problems.first() {
            return Err(problem.message.clone());
        }
        let mut names = BTreeSet::new();
        let mut live = false;
        let mut expressions = BTreeMap::new();
        for def in &document.definitions {
            if !names.insert(def.named.name.clone()) {
                return Err(format!("Duplicate definition '{}'", def.named.name));
            }
            if !def.expression {
                return Err("Module definitions must use :=".into());
            }
            expressions.insert(
                def.source.clone(),
                crate::engine::Parser::parse(&def.source)
                    .map_err(|e| format!("{}:{}: {e}", path.display(), def.value_span.line + 1))?,
            );
            live |= crate::engine::lex(&def.source)?
                .iter()
                .any(|t| matches!(&t.kind,Lexeme::Name(n) if n=="now" || n=="today"));
        }
        let workspace = Arc::new(Workspace {
            roots: vec![path.parent().unwrap_or(Path::new(".")).into()],
            documents: [(path.clone(), document)].into(),
            cache: Default::default(),
            lookups: Default::default(),
            modules: Arc::new(ModuleRegistry { modules: vec![] }),
        });
        let mut engine = Engine::at(&workspace, epoch())
            .pure()
            .with_link_features(crate::link_features::LinkFeatures::new(&[]));
        let Value::Record(config) = engine.named(&path, "module")? else {
            return Err("module must be a record".into());
        };
        if !matches!(config.get("api"),Some(Value::Number(n)) if *n==1.0) {
            return Err("module.api must be 1".into());
        }
        let id = text(config.get("id").ok_or("module.id is required")?)?;
        if id.is_empty()
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        {
            return Err("Invalid module id".into());
        }
        let kind = text(config.get("kind").ok_or("module.kind is required")?)?;
        let enabled = match config.get("enabled") {
            None => true,
            Some(Value::Bool(v)) => *v,
            _ => return Err("enabled must be boolean".into()),
        };
        let mut fields = BTreeMap::new();
        let inputs = match config.get("inputs") {
            None => vec![
                "sections".into(),
                "tasks".into(),
                "values".into(),
                "links".into(),
            ],
            Some(Value::Record(selections)) => {
                for (name, selection) in selections {
                    fields.insert(name.clone(), strings(selection)?);
                }
                fields.keys().cloned().collect()
            }
            Some(value) => strings(value)?,
        };
        for input in &inputs {
            if !crate::catalog::COLLECTIONS.contains(&input.as_str()) {
                return Err(format!("Unknown input collection: {input}"));
            }
        }
        let required = match kind.as_str() {
            "link" => "inlay",
            "feature" => "collect",
            "library" => "",
            _ => return Err("module.kind must be link, feature, or library".into()),
        };
        let hosts = config
            .get("hosts")
            .map(strings)
            .transpose()?
            .unwrap_or_default();
        if enabled
            && kind == "link"
            && (hosts.is_empty()
                || hosts.iter().any(|host| {
                    Url::parse(&format!("https://{host}")).is_err()
                        || host.contains(['/', '?', '#', '@', ':'])
                        || host.to_lowercase() != *host
                }))
        {
            return Err("Link modules require lowercase host names".into());
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
            ("matches", 1),
            ("property_names", 1),
            ("time_dependent", 1),
            ("actions", 1),
            ("reduce", 2),
            ("hovers", 1),
            ("diagnostics", 1),
            ("format", 1),
        ] {
            if kind == "library" {
                break;
            }
            if names.contains(name) {
                if !matches!(engine.named(&path,name)?,Value::Function(f) if f.params.len()==arity)
                {
                    return Err(format!("{name} must be a function with {arity} parameters"));
                }
            } else if name == required
                && enabled
                && kind != "library"
                && (kind == "link"
                    || !["actions", "hovers", "diagnostics", "format"]
                        .iter()
                        .any(|n| names.contains(*n)))
            {
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
        let cache_key = match config.get("cache_namespace") {
            Some(Value::Null) => None,
            None => Some(format!("{id}:{version}")),
            _ => return Err("cache_namespace may only be null (legacy cache) or omitted".into()),
        };
        Ok(Self {
            id,
            kind,
            path,
            live,
            own_live: live,
            enabled,
            inputs,
            imports: config
                .get("imports")
                .map(strings)
                .transpose()?
                .unwrap_or_default(),
            fields,
            hosts,
            prefix,
            properties,
            cache_key,
            expressions: Arc::new(expressions),
            workspace,
        })
    }
    pub(crate) fn environment(&self) -> Arc<Workspace> {
        self.workspace.clone()
    }
    pub fn revision(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.workspace.documents[&self.path].text.hash(&mut hash);
        for module in &self.workspace.modules.modules {
            module.revision().hash(&mut hash);
        }
        format!("{:016x}", hash.finish())
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
        if !self.enabled {
            return Err(format!("Module '{}' is disabled", self.id));
        }
        for arg in &args {
            crate::evaluate::functional::check_size(arg)?;
        }
        let mut engine = Engine::at(&self.workspace, now)
            .pure()
            .with_link_features(crate::link_features::LinkFeatures::new(&[]))
            .with_environment(self.workspace.clone())
            .with_expressions(self.expressions.clone());
        let function = engine.named(&self.path, name)?;
        engine
            .call(function, args)
            .map_err(|e| format!("Module {}.{name}: {e}", self.id))
    }
    fn context(&self, ctx: &LinkContext<'_>) -> Value {
        let cached = ctx
            .cached
            .filter(|m| m.provider.as_deref() == self.cache_key.as_deref());
        record([
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
            && self.kind == "link"
            && matches!(url.scheme(), "http" | "https")
            && url
                .host_str()
                .is_some_and(|host| self.hosts.iter().any(|h| h == host))
            && url.path().starts_with(&self.prefix)
            && (!self.has("matches")
                || matches!(
                    self.call("matches", vec![url_value(url)], epoch()),
                    Ok(Value::Bool(true))
                ))
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        self.call("inlay", vec![self.context(ctx)], ctx.now.fixed_offset())
            .and_then(|v| text(&v))
            .unwrap_or_else(|e| format!("module error · {e}"))
    }
    fn hover(&self, ctx: &LinkContext<'_>) -> Option<String> {
        self.has("hover").then(|| {
            self.call("hover", vec![self.context(ctx)], ctx.now.fixed_offset())
                .and_then(|v| text(&v))
                .unwrap_or_else(|e| e)
        })
    }
    fn time_dependent(&self, ctx: &LinkContext<'_>) -> bool {
        if self.has("time_dependent") {
            return !matches!(
                self.call(
                    "time_dependent",
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
        if self.has("property_names") {
            return self
                .call("property_names", vec![url_value(url)], epoch())
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
            merged: data["merged"].as_bool(),
            checks: data["checks"].as_str().map(str::to_owned),
            review: data["review"].as_str().map(str::to_owned),
            fetched_at: now,
            provider: self.cache_key.clone(),
            data: Some(data),
        })
    }
}
impl ModuleRegistry {
    pub fn overrides(&self, id: &str) -> bool {
        self.modules
            .iter()
            .any(|m| m.id == id && !m.path.starts_with("/__wtf_stdlib__"))
    }
    pub fn active(&self) -> impl Iterator<Item = &Module> {
        self.modules.iter().filter(|m| m.enabled)
    }
    /// Resolve every call against this immutable workspace snapshot.
    pub fn call(
        &self,
        id: &str,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> Result<Value, String> {
        self.active()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("Module '{id}' is unavailable or disabled"))?
            .call(name, args, now)
            .map_err(|e| {
                e.strip_prefix(&format!("Module {id}.{name}: "))
                    .unwrap_or(&e)
                    .to_owned()
            })
    }
    pub fn same_sources(&self, other: &Self) -> bool {
        self.modules.len() == other.modules.len()
            && self.modules.iter().zip(&other.modules).all(|(a, b)| {
                a.path == b.path
                    && a.workspace.documents[&a.path].text == b.workspace.documents[&b.path].text
            })
    }
    /// Compile a complete replacement before the caller swaps its Arc snapshot.
    pub fn compile(sources: BTreeMap<PathBuf, String>) -> Result<Self, String> {
        Self::compile_over(sources, bundled())
    }
    fn compile_over(sources: BTreeMap<PathBuf, String>, base: &[Module]) -> Result<Self, String> {
        if sources.len() > 64 {
            return Err("At most 64 module modules may be loaded".into());
        }
        let mut modules = Vec::new();
        let mut ids = BTreeSet::new();
        for (path, source) in sources {
            let module = Module::compile(path.clone(), source)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if !ids.insert(module.id.clone()) {
                return Err(format!("Duplicate module id '{}'", module.id));
            }
            modules.push(module);
        }
        modules.extend(base.iter().filter(|m| !ids.contains(&m.id)).cloned());
        Ok(Self {
            modules: link(modules)?,
        })
    }
    #[cfg(feature = "native")]
    pub fn load(roots: &[PathBuf]) -> Result<Self, String> {
        let standard = Self::compile(Self::read_sources(roots, "stdlib")?)?;
        Self::compile_over(
            Self::read_sources(roots, ".wtf/modules")?,
            &standard.modules,
        )
    }
    #[cfg(feature = "native")]
    fn read_sources(
        roots: &[PathBuf],
        directory: &str,
    ) -> Result<BTreeMap<PathBuf, String>, String> {
        let mut sources = BTreeMap::new();
        for root in roots {
            let directory = root.join(directory);
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
        Ok(sources)
    }
}

/// Bundled modules use exactly the same compiler and adapters as workspace modules.
pub fn bundled() -> &'static [Module] {
    static MODULES: std::sync::OnceLock<Vec<Module>> = std::sync::OnceLock::new();
    MODULES.get_or_init(|| {
        let modules = [
            (
                "itinerary_core",
                include_str!("../../stdlib/itinerary_core.wtf"),
            ),
            ("itinerary", include_str!("../../stdlib/itinerary.wtf")),
            ("timers", include_str!("../../stdlib/timers.wtf")),
            ("plans", include_str!("../../stdlib/plans.wtf")),
            ("plan", include_str!("../../stdlib/plan.wtf")),
            ("timer", include_str!("../../stdlib/timer.wtf")),
            ("format", include_str!("../../stdlib/format.wtf")),
            ("github", include_str!("../../stdlib/github.wtf")),
            ("table_cells", include_str!("../../stdlib/table_cells.wtf")),
            ("checklists", include_str!("../../stdlib/checklists.wtf")),
            (
                "calculations",
                include_str!("../../stdlib/calculations.wtf"),
            ),
        ]
        .into_iter()
        .map(|(id, source)| {
            Module::compile(
                format!("/__wtf_stdlib__/stdlib/{id}.wtf").into(),
                source.into(),
            )
            .expect("valid bundled module")
        })
        .collect();
        link(modules).expect("valid standard imports")
    })
}

fn link(modules: Vec<Module>) -> Result<Vec<Module>, String> {
    fn resolve(
        id: &str,
        sources: &BTreeMap<String, Module>,
        ready: &mut BTreeMap<String, Module>,
        stack: &mut Vec<String>,
    ) -> Result<Module, String> {
        if let Some(module) = ready.get(id) {
            return Ok(module.clone());
        }
        if stack.iter().any(|s| s == id) {
            return Err(format!(
                "Module import cycle: {} -> {id}",
                stack.join(" -> ")
            ));
        }
        let mut module = match sources.get(id) {
            Some(m) => m.clone(),
            None => return Err(format!("Unknown module import '{id}'")),
        };
        stack.push(id.into());
        let dependencies = module
            .imports
            .iter()
            .map(|id| resolve(id, sources, ready, stack))
            .collect::<Result<Vec<_>, _>>()?;
        stack.pop();
        module.live = module.own_live || dependencies.iter().any(|m| m.live);
        Arc::make_mut(&mut module.workspace).modules = Arc::new(ModuleRegistry {
            modules: dependencies,
        });
        ready.insert(id.into(), module.clone());
        Ok(module)
    }
    let order: Vec<_> = modules.iter().map(|m| m.id.clone()).collect();
    let sources = modules.into_iter().map(|m| (m.id.clone(), m)).collect();
    let mut ready = BTreeMap::new();
    order
        .into_iter()
        .map(|id| resolve(&id, &sources, &mut ready, &mut vec![]))
        .collect()
}
/// Run shared standard-library behavior with an explicit request clock.
pub fn standard(
    id: &str,
    name: &str,
    args: Vec<Value>,
    now: DateTime<FixedOffset>,
) -> Result<Value, String> {
    bundled()
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("Unknown standard module '{id}'"))?
        .call(name, args, now)
        .map_err(|e| {
            e.strip_prefix(&format!("Module {id}.{name}: "))
                .unwrap_or(&e)
                .to_owned()
        })
}

pub(crate) fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    if let Value::Record(fields) = value {
        fields
            .get(key)
            .ok_or_else(|| format!("Missing field '{key}'"))
    } else {
        Err("Expected a record".into())
    }
}
pub(crate) fn list(value: &Value) -> Result<&[Value], String> {
    if let Value::List(items) = value {
        Ok(items)
    } else {
        Err("Expected a list".into())
    }
}
