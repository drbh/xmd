//! Hot-reloadable XMD modules: what one is, how it compiles, and how the
//! engine calls into it.
use chrono::{DateTime, FixedOffset};
use model::Document;
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};
use syntax::{Expr, Lexeme, Parser, lex};
use url::Url;
use values::{EvalError, EvalResult, FromValue, LookupKind, Value};

use crate::registry::ModuleRegistry;
use values::Collection;

/// What a compiled module evaluates against: its own note and the libraries it
/// imports, as the evaluator holds them. The evaluator implements it, so this
/// vocabulary describes and validates modules without naming the engine.
pub trait ModuleEnvironment: Any + Send + Sync + Debug {
    /// The note at `path`, which for a module is its own source.
    fn document(&self, path: &Path) -> &Document;
    /// Whether `name` resolves to exactly one symbol of the note at `path`.
    fn resolves(&self, path: &Path, name: &str) -> bool;
    /// The libraries the module imports, linked.
    fn modules(&self) -> &ModuleRegistry;
    /// The same notes over another set of linked libraries.
    fn with_modules(&self, modules: ModuleRegistry) -> Arc<dyn ModuleEnvironment>;
    /// One evaluator of the note at `path`'s definitions by name. A module's
    /// manifest checks share one, so they share its budget and memo.
    fn evaluator<'s>(&'s self, path: &'s Path) -> Box<Evaluator<'s>>;
    /// Call `module`'s function `name` in this environment at `now`.
    fn call(
        self: Arc<Self>,
        module: &Module,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value>;
}

/// Evaluates a definition of one note by name.
pub type Evaluator<'s> = dyn FnMut(&str) -> EvalResult<Value> + 's;

/// Builds the environment a module compiled from the note at `path` evaluates
/// against: that note alone, importing nothing until the registry links it.
pub type NewEnvironment = fn(&Path, Document) -> Arc<dyn ModuleEnvironment>;

/// What a module plugs into. `module.kind` in the source names one of these.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::EnumString,
    strum::Display,
    strum::VariantArray,
    strum::IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
pub enum ModuleKind {
    /// Decorates matching URLs: inlays, hovers, properties and refreshes.
    Link,
    /// Drives editor features over the document catalog.
    Feature,
    /// Plain functions other modules and the engine import by name.
    Library,
    /// A command a person runs with `xmd run`: its pure `step` hook asks the
    /// host for effects (HTTP, files in the run's directory) and reads their
    /// results on the next step. Nothing runs while editing or rendering.
    Command,
    /// Fetches external data a note's lookups read, such as exchange rates or
    /// forecasts: the same `step` loop as a command, limited to HTTP, run only
    /// on an explicit refresh. `provides` names the lookups it answers.
    Provider,
}
impl ModuleKind {
    /// The one hook a module of this kind must supply.
    pub fn required_hook(self) -> Option<Hook> {
        match self {
            Self::Link => Some(Hook::Inlay),
            Self::Feature => Some(Hook::Collect),
            Self::Library => None,
            Self::Command | Self::Provider => Some(Hook::Step),
        }
    }
}

/// The fixed entry points a link or feature module may define. Library exports
/// are user-chosen names and stay text; these are the contract the hosts call.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::AsRefStr,
    strum::Display,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum Hook {
    Collect,
    Inlay,
    Hover,
    Property,
    Refresh,
    Decode,
    Matches,
    PropertyNames,
    TimeDependent,
    Actions,
    Reduce,
    Hovers,
    Diagnostics,
    Format,
    Step,
}
impl Hook {
    pub fn arity(self) -> usize {
        match self {
            Self::Property | Self::Reduce | Self::Decode => 2,
            _ => 1,
        }
    }
    /// Whether some kind of module must supply this hook; such hooks are
    /// validated only as the required one.
    pub(crate) fn required(self) -> bool {
        <ModuleKind as strum::VariantArray>::VARIANTS
            .iter()
            .any(|kind| kind.required_hook() == Some(self))
    }
}

#[derive(Clone, Debug)]
pub struct Module {
    pub id: String,
    pub kind: ModuleKind,
    pub path: PathBuf,
    pub live: bool,
    pub(crate) own_live: bool,
    pub enabled: bool,
    pub inputs: Vec<Collection>,
    pub fields: BTreeMap<Collection, Vec<String>>,
    pub(crate) imports: Vec<String>,
    /// The lookup kinds a provider module answers (`rate`, `quote`, `forecast`).
    pub provides: Vec<LookupKind>,
    /// The declared public API, or `None` for the older rule that every
    /// non-`_` definition is public. See [`Module::public_names`].
    pub(crate) exports: Option<Vec<String>>,
    pub(crate) expressions: Arc<BTreeMap<String, Expr>>,
    pub(crate) hosts: Vec<String>,
    pub(crate) prefix: String,
    pub(crate) properties: Vec<String>,
    pub(crate) cache_key: Option<String>,
    pub(crate) environment: Arc<dyn ModuleEnvironment>,
}
impl Module {
    /// What the module's closures evaluate against.
    pub fn environment(&self) -> &Arc<dyn ModuleEnvironment> {
        &self.environment
    }
    /// The module's definitions, which its closures resolve names in.
    pub fn expressions(&self) -> &Arc<BTreeMap<String, Expr>> {
        &self.expressions
    }
    pub fn revision(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.environment.document(&self.path).text.hash(&mut hash);
        for module in &self.environment.modules().modules {
            module.revision().hash(&mut hash);
        }
        format!("{:016x}", hash.finish())
    }
    /// The members a note sees through `import(id)`, in the order the module
    /// declares them: its `exports` list, or, when it declares none, every
    /// definition that is not `module` and not `_`-prefixed. The reference
    /// and completion describe exactly this list, so there is one answer to
    /// "what is this library's API".
    pub fn public_names(&self) -> Vec<String> {
        match &self.exports {
            Some(exports) => exports.clone(),
            None => self.member_names(),
        }
    }
    /// Every name another module's `imports:` may reach: all definitions but
    /// `module` and the `_`-prefixed ones, exported or not. Module code is
    /// trusted the way the Rust adapters are, so the engine contract of
    /// `timer` or `plan` stays callable from `timers` or `plans` while
    /// `exports` keeps it out of notes.
    pub fn member_names(&self) -> Vec<String> {
        self.environment
            .document(&self.path)
            .definitions
            .iter()
            .map(|d| d.named.name.as_str())
            .filter(|name| *name != "module" && !name.starts_with('_'))
            .map(str::to_owned)
            .collect()
    }
    /// Whether the module defines `entry`: a typed [`Hook`] or, for library
    /// modules whose exports are user-defined, a plain function name.
    pub fn has(&self, entry: impl AsRef<str>) -> bool {
        self.environment.resolves(&self.path, entry.as_ref())
    }
    pub fn call(
        &self,
        entry: impl AsRef<str>,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value> {
        let name = entry.as_ref();
        if !self.enabled {
            return Err(EvalError::ModuleDisabled(self.id.clone()));
        }
        for arg in &args {
            values::check_size(arg)?;
        }
        self.environment.clone().call(self, name, args, now)
    }
}

pub fn is_module_path(path: &Path) -> bool {
    common::is_note(path)
        && path.parent().is_some_and(|p| {
            p.file_name().is_some_and(|s| s == "stdlib")
                || (p.file_name().is_some_and(|s| s == "modules")
                    && p.parent()
                        .is_some_and(|p| p.file_name().is_some_and(|s| s == ".xmd")))
        })
}

impl Module {
    /// Parse a module's source and validate its contract.
    ///
    /// A module is a `.x.md` file whose `module :=` record says what it is:
    /// `{api: 1, id, kind, inputs?, imports?, hosts?, path_prefix?, properties?,
    /// enabled?, cache_version?, cache_namespace?, exports?}`.
    ///
    /// `exports` is an optional list of text naming a library's public API.
    /// `import(id)` from a note returns exactly those members, the reference lists
    /// exactly those, and completion offers exactly those. Each name must be a
    /// top-level definition that is neither `module` nor `_`-prefixed, and may
    /// appear once. A library that declares no `exports` keeps the older rule,
    /// every non-`_` definition is public; `exports: []` is a library only the
    /// engine and other modules call. Link and feature modules have hooks, not
    /// exports, so for them the field must be absent or empty. Another module's
    /// `imports:` is not bound by `exports`: module code may reach any non-`_`
    /// name of a library it declares (see `Module::member_names`).
    pub fn compile(path: PathBuf, source: String, environment: NewEnvironment) -> EvalResult<Self> {
        if source.len() > 65_536 {
            return Err("Modules are limited to 64 KiB".into());
        }
        let document = Document::parse(source);
        if let Some(problem) = document.problems.first() {
            return Err(problem.message.clone().into());
        }
        let mut names = BTreeSet::new();
        let mut live = false;
        let mut expressions = BTreeMap::new();
        for def in &document.definitions {
            if !names.insert(def.named.name.clone()) {
                return Err(format!("Duplicate definition '{}'", def.named.name).into());
            }
            if !def.expression {
                return Err("Module definitions must use :=".into());
            }
            expressions.insert(
                def.source.clone(),
                Parser::parse(&def.source)
                    .map_err(|e| format!("{}:{}: {e}", path.display(), def.value_span.line + 1))?,
            );
            live |= lex(&def.source)
                .map_err(EvalError::Message)?
                .iter()
                .any(|t| matches!(&t.kind,Lexeme::Name(n) if n=="now" || n=="today"));
        }
        let environment = environment(&path, document);
        let mut named = environment.evaluator(&path);
        let Value::Record(config) = named("module")? else {
            return Err("module must be a record".into());
        };
        let opt_strings = |key| {
            config
                .get(key)
                .map(strings)
                .transpose()
                .map(Option::unwrap_or_default)
        };
        if !matches!(config.get("api"),Some(Value::Number(n)) if *n==1.0) {
            return Err("module.api must be 1".into());
        }
        let id = String::from_value(config.get("id").ok_or("module.id is required")?)?;
        if id.is_empty()
            || !id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        {
            return Err("Invalid module id".into());
        }
        let kind: ModuleKind =
            String::from_value(config.get("kind").ok_or("module.kind is required")?)?
                .parse()
                .map_err(|_| "module.kind must be link, feature, command, provider, or library")?;
        let enabled = match config.get("enabled") {
            None => true,
            Some(Value::Bool(v)) => *v,
            _ => return Err("enabled must be boolean".into()),
        };
        let mut fields = BTreeMap::new();
        let inputs: Vec<Collection> = match config.get("inputs") {
            None => vec![
                Collection::Sections,
                Collection::Tasks,
                Collection::Values,
                Collection::Links,
            ],
            Some(Value::Record(selections)) => {
                for (name, selection) in selections {
                    fields.insert(name.parse()?, strings(selection)?);
                }
                fields.keys().copied().collect()
            }
            Some(value) => strings(value)?
                .iter()
                .map(|input| input.parse())
                .collect::<Result<_, String>>()?,
        };
        // `hosts: "*"` is the host-agnostic spelling of an empty list: the
        // module recognizes a URL shape on any site, through its own `matches`.
        let hosts = match config.get("hosts") {
            Some(Value::Text(any)) if any == "*" => vec![],
            _ => opt_strings("hosts")?,
        };
        if enabled
            && kind == ModuleKind::Link
            && hosts.iter().any(|host| {
                Url::parse(&format!("https://{host}")).is_err()
                    || host.contains(['/', '?', '#', '@', ':'])
                    || host.to_lowercase() != *host
            })
        {
            return Err("Link modules require lowercase host names".into());
        }
        // Without hosts nothing narrows the module but its own predicate, so a
        // host-agnostic link module has to supply one.
        if enabled && kind == ModuleKind::Link && hosts.is_empty() && !names.contains("matches") {
            return Err("Link modules require hosts or a matches function".into());
        }
        let prefix = config
            .get("path_prefix")
            .map(String::from_value)
            .transpose()?
            .unwrap_or_default();
        let properties = opt_strings("properties")?;
        if properties
            .iter()
            .any(|p| !model::identifier(p) || matches!(p.as_str(), "url" | "exists"))
        {
            return Err("Invalid or reserved property name".into());
        }
        // A library's exports are its own names, so only link and feature
        // modules are checked against the hook table: required hook first.
        if let Some(required) = kind.required_hook() {
            let optional = <Hook as strum::VariantArray>::VARIANTS
                .iter()
                .copied()
                .filter(|h| !h.required());
            for hook in std::iter::once(required).chain(optional) {
                let name = hook.as_ref();
                let arity = hook.arity();
                if names.contains(name) {
                    if !matches!(named(name)?,Value::Function(f) if f.params.len()==arity) {
                        return Err(
                            format!("{name} must be a function with {arity} parameters").into()
                        );
                    }
                } else if hook == required
                    && enabled
                    && (kind == ModuleKind::Link
                        || ![Hook::Actions, Hook::Hovers, Hook::Diagnostics, Hook::Format]
                            .iter()
                            .any(|h| names.contains(h.as_ref())))
                {
                    return Err(format!("Missing {name} function").into());
                }
            }
        }
        if names.contains(Hook::Refresh.as_ref()) != names.contains(Hook::Decode.as_ref()) {
            return Err("refresh and decode must be supplied together".into());
        }
        if !properties.is_empty() && !names.contains(Hook::Property.as_ref()) {
            return Err("Declared properties need a property function".into());
        }
        let provides = opt_strings("provides")?;
        let lookups: Option<Vec<LookupKind>> = provides.iter().map(|p| p.parse().ok()).collect();
        if kind == ModuleKind::Provider && (provides.is_empty() || lookups.is_none()) {
            return Err(
                "A provider module provides one or more of rate, quote and forecast".into(),
            );
        }
        if kind != ModuleKind::Provider && !provides.is_empty() {
            return Err("Only provider modules declare provides".into());
        }
        let exports = config
            .get("exports")
            .map(strings)
            .transpose()
            .map_err(|_| EvalError::from("module.exports must be a list of text"))?;
        if let Some(exports) = &exports {
            if kind != ModuleKind::Library && !exports.is_empty() {
                return Err(format!(
                    "A {kind} module has no exports; its hooks are called by the host"
                )
                .into());
            }
            let mut seen = BTreeSet::new();
            for name in exports {
                if name == "module" || name.starts_with('_') {
                    return Err(format!(
                        "'{name}' cannot be exported; 'module' and '_' names are private"
                    )
                    .into());
                }
                if !names.contains(name) {
                    return Err(format!(
                        "exports names '{name}', which this module does not define"
                    )
                    .into());
                }
                if !seen.insert(name) {
                    return Err(format!("Duplicate export '{name}'").into());
                }
            }
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
        drop(named);
        Ok(Self {
            id,
            kind,
            path,
            live,
            own_live: live,
            enabled,
            inputs,
            imports: opt_strings("imports")?,
            provides: lookups.unwrap_or_default(),
            fields,
            hosts,
            prefix,
            properties,
            exports,
            cache_key,
            expressions: Arc::new(expressions),
            environment,
        })
    }
}

/// A list of text, as a module's manifest fields declare them.
fn strings(value: &Value) -> EvalResult<Vec<String>> {
    if let Value::List(items) = value {
        items.iter().map(String::from_value).collect()
    } else {
        Err(EvalError::Expected("a list of text"))
    }
}
