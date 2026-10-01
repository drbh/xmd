//! What a module is and how it compiles: its kind and hooks, the
//! environment and clock it runs in, and `Module::compile`, which reads a
//! source's `module :=` manifest. The hooks' reference data is in `hooks`,
//! and the checks of what a feature module declares in `declarations`.
use chrono::{DateTime, FixedOffset};
use document::forms::Form;
use document::recognized::Rule;
use document::{Declaration, Document};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};
use syntax::{Expr, Lexeme, Parser, lex};
use url::Url;
use values::{Collection, EvalError, EvalResult, FromValue, Value};

use crate::declarations::{Entry, attributes, collections, flag, forms, rules, strings};
use crate::hooks::{HOOKS, HookContract};
use crate::registry::ModuleRegistry;

/// The clock a module call runs at when native code has no clock to give
/// it: the call is handed every date it needs as an argument instead. Module
/// code evaluated at it answers `now()` and `today()` with an error rather
/// than with 1970.
pub fn no_clock() -> DateTime<FixedOffset> {
    DateTime::UNIX_EPOCH.fixed_offset()
}
/// Whether `now` is a real clock rather than [`no_clock`].
pub fn has_clock(now: DateTime<FixedOffset>) -> bool {
    now != no_clock()
}

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
    pub(crate) fn required_hook(self) -> Option<Hook> {
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
    Records,
    Symbols,
    Completions,
    Define,
    Step,
}
impl Hook {
    pub(crate) fn arity(self) -> usize {
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
    /// What the host hands this hook and what it must return.
    pub fn contract(self) -> &'static HookContract {
        HOOKS
            .iter()
            .find(|contract| contract.hook == self)
            .expect("every hook is declared in HOOKS")
    }
}

/// A collection a feature module declares and builds: the other half of
/// [`Collection::Declared`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declared {
    pub name: Arc<str>,
    /// Which of its records join `entries`.
    pub entries: Joins,
    /// The collections only the `records` hook reads to build it, each with
    /// the fields it keeps, or all of them.
    pub from: BTreeMap<Collection, Option<Vec<String>>>,
}
/// Which records of a declared collection join `entries`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Joins {
    None,
    All,
    /// Those whose field of this name is true.
    Where(Arc<str>),
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
    /// What the `records` hook reads besides `inputs`: every collection a
    /// declared collection is built `from`, with the fields it keeps, or
    /// all of them. The module's other hooks are not handed these.
    pub sources: BTreeMap<Collection, Option<Vec<String>>>,
    /// The recognizers a feature module declares in `recognizes`, compiled.
    pub recognizes: Vec<Arc<Rule>>,
    /// The collections a feature module declares in `collections` and builds
    /// in its `records` hook.
    pub collections: Vec<Declared>,
    /// The attributes a feature module declares in `attributes`: notes are
    /// parsed knowing them, and the host evaluates them.
    pub attributes: Vec<Arc<Declaration>>,
    /// The forms a feature module declares in `forms`: notes are parsed
    /// knowing them, and its `define` hook evaluates a definition of one.
    pub forms: Vec<Arc<Form>>,
    pub(crate) imports: Vec<String>,
    /// The lookup kinds a provider module answers (`rate`, `quote`, `forecast`).
    pub provides: Vec<String>,
    /// The declared public API, or `None` for the older rule that every
    /// non-`_` definition is public. See [`Module::public_names`].
    pub(crate) exports: Option<Vec<String>>,
    /// What a library declares in `accepts`: for a function it defines, the
    /// kind of value each argument takes, by name, which completion offers
    /// inside a call to it. Nothing checks a call against it.
    pub accepts: BTreeMap<String, Vec<String>>,
    pub(crate) expressions: Arc<BTreeMap<String, Expr>>,
    pub(crate) hosts: Vec<String>,
    pub(crate) prefix: String,
    pub(crate) properties: Vec<String>,
    pub(crate) cache_key: Option<String>,
    /// Replaced only through [`Module::set_environment`], which forgets the
    /// revision.
    pub(crate) environment: Arc<dyn ModuleEnvironment>,
    /// [`Module::revision`], once asked: it reads only the environment.
    revision: std::sync::OnceLock<String>,
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
    /// Evaluate against `environment` from now on.
    pub(crate) fn set_environment(&mut self, environment: Arc<dyn ModuleEnvironment>) {
        self.environment = environment;
        self.revision = std::sync::OnceLock::new();
    }
    /// A hash of the module's text and of every module it sees.
    pub fn revision(&self) -> String {
        self.revision
            .get_or_init(|| {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                self.environment.document(&self.path).text().hash(&mut hash);
                for module in &self.environment.modules().modules {
                    module.revision().hash(&mut hash);
                }
                format!("{:016x}", hash.finish())
            })
            .clone()
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
    /// trusted the way the Rust adapters are, so the internals of `timer` or
    /// `plan` stay callable from `timers`, `plans` or the prelude while
    /// `exports` keeps them out of notes.
    pub fn member_names(&self) -> Vec<String> {
        self.environment
            .document(&self.path)
            .definitions()
            .iter()
            .map(|d| d.named.name.as_str())
            .filter(|name| *name != "module" && !name.starts_with('_'))
            .map(str::to_owned)
            .collect()
    }
    /// Whether `name` is one of [`Self::public_names`].
    pub fn is_public(&self, name: &str) -> bool {
        match &self.exports {
            Some(exports) => exports.iter().any(|n| n == name),
            None => self.member_names().iter().any(|n| n == name),
        }
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
        // What comes from outside, a fetched reply or cached data, is held
        // to the size any value may have. A feature module is handed the
        // note itself, as large as the note is, and what its call may build
        // grows with that, which the environment measures.
        if self.kind != ModuleKind::Feature {
            for arg in &args {
                values::check_size(arg)?;
            }
        }
        self.environment.clone().call(self, name, args, now)
    }
}

pub fn is_module_path(path: &Path) -> bool {
    common::is_note(path)
        && path
            .parent()
            .is_some_and(|p| p.ends_with("stdlib") || p.ends_with(".xmd/modules"))
}

impl Module {
    /// Parse a module's source and validate its contract.
    ///
    /// A module is a `.xmd` file whose `module :=` record says what it is:
    /// `{api: 1, id, kind, inputs?, imports?, hosts?, path_prefix?, properties?,
    /// enabled?, cache_version?, cache_namespace?, exports?, accepts?,
    /// recognizes?, collections?, attributes?, forms?}`.
    ///
    /// `accepts` (libraries only) is a record from a function the module
    /// defines to the kind names its arguments take, in order:
    /// `{remind: ["Duration", "DateTime"]}`. Completion inside
    /// a call offers the names, built-ins and literals of that kind; nothing
    /// else reads it.
    ///
    /// `recognizes` (feature modules only) declares patterns the host runs
    /// over a note's generic blocks as it parses them, without evaluating
    /// anything; see the `recognizer` record in [`HOOK_RECORDS`](crate::HOOK_RECORDS).
    ///
    /// `forms` (feature modules only) declares definition forms; see the
    /// `form` record in [`HOOK_RECORDS`](crate::HOOK_RECORDS).
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
    pub(crate) fn compile(
        path: PathBuf,
        source: String,
        environment: NewEnvironment,
    ) -> EvalResult<Self> {
        if source.len() > 65_536 {
            return Err("Modules are limited to 64 KiB".into());
        }
        let document = Document::parse(source);
        if let Some(problem) = document.problems().first() {
            return Err(problem.message.clone().into());
        }
        let mut names = BTreeSet::new();
        let mut live = false;
        let mut expressions = BTreeMap::new();
        for def in document.definitions() {
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
        let manifest = Entry::of("module", &config);
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
        let enabled = flag(config.get("enabled"), true).ok_or("enabled must be boolean")?;
        let mut fields = BTreeMap::new();
        let inputs: Vec<Collection> = match config.get("inputs") {
            None => default_inputs(),
            Some(Value::Record(selections)) => {
                for (name, selection) in selections.iter() {
                    fields.insert(name.parse()?, strings(selection)?);
                }
                fields.keys().cloned().collect()
            }
            Some(value) => strings(value)?
                .iter()
                .map(|input| input.parse())
                .collect::<Result<_, String>>()?,
        };
        // Recognizers, collections, attributes and forms are a feature module's.
        let declared = |key: &str| match config.get(key) {
            Some(_) if kind != ModuleKind::Feature => {
                Err(format!("Only feature modules declare {key}"))
            }
            declared => Ok(declared),
        };
        let recognizes = declared("recognizes")?.map_or(Ok(vec![]), |d| rules(&id, d))?;
        let collections = declared("collections")?.map_or(Ok(vec![]), collections)?;
        let attributes = declared("attributes")?.map_or(Ok(vec![]), |d| attributes(&id, d))?;
        let forms = declared("forms")?.map_or(Ok(vec![]), |d| forms(&id, d))?;
        if enabled && kind == ModuleKind::Feature {
            agree(
                !forms.is_empty(),
                names.contains(Hook::Define.as_ref()),
                "define evaluates the forms a module declares; declare them in forms",
                "A module that declares forms evaluates them in a define function",
            )?;
        }
        if enabled {
            agree(
                !collections.is_empty(),
                names.contains(Hook::Records.as_ref()),
                "records builds the collections a module declares; declare them in collections",
                "A module that declares collections builds them in a records function",
            )?;
        }
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
            .any(|p| !document::identifier(p) || matches!(p.as_str(), "url" | "exists"))
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
                        || ![
                            Hook::Actions,
                            Hook::Hovers,
                            Hook::Diagnostics,
                            Hook::Format,
                            Hook::Records,
                            Hook::Symbols,
                            Hook::Completions,
                            Hook::Define,
                        ]
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
        agree(
            !provides.is_empty(),
            kind == ModuleKind::Provider,
            "A provider module provides one or more kinds of lookup, such as rate, quote or forecast",
            "Only provider modules declare provides",
        )?;
        let exports = manifest.list("exports")?;
        let undefined = "which this module does not define";
        if let Some(exports) = &exports {
            if kind != ModuleKind::Library && !exports.is_empty() {
                let hooks = "its hooks are called by the host";
                return Err(format!("A {kind} module has no exports; {hooks}").into());
            }
            let mut seen = BTreeSet::new();
            for name in exports {
                if name == "module" || name.starts_with('_') {
                    let private = "'module' and '_' names are private";
                    return Err(format!("'{name}' cannot be exported; {private}").into());
                }
                if !names.contains(name) {
                    return Err(format!("exports names '{name}', {undefined}").into());
                }
                if !seen.insert(name) {
                    return Err(format!("Duplicate export '{name}'").into());
                }
            }
        }
        let accepts = match config.get("accepts") {
            None => BTreeMap::new(),
            Some(_) if kind != ModuleKind::Library => {
                return Err("Only library modules declare accepts".into());
            }
            Some(Value::Record(entries)) => entries
                .iter()
                .map(|(name, kinds)| {
                    if name == "module" || name.starts_with('_') || !names.contains(name) {
                        return Err(format!("accepts names '{name}', {undefined}").into());
                    }
                    let kinds = strings(kinds)
                        .ok()
                        .filter(|kinds| kinds.iter().all(|k| common::ValueType::is_name(k)))
                        .ok_or_else(|| {
                            let accepts = Entry::of("accepts", entries);
                            accepts.must(name, "be a list of kind names, such as Duration")
                        })?;
                    Ok((name.clone(), kinds))
                })
                .collect::<EvalResult<_>>()?,
            Some(_) => return Err(manifest.must("accepts", "be a record of function names")),
        };
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
            sources: collections
                .iter()
                .flat_map(|declared| declared.from.clone())
                .collect(),
            recognizes,
            collections,
            attributes,
            forms,
            imports: opt_strings("imports")?,
            provides,
            fields,
            hosts,
            prefix,
            properties,
            exports,
            accepts,
            cache_key,
            expressions: Arc::new(expressions),
            environment,
            revision: std::sync::OnceLock::new(),
        })
    }
}

/// Whether a module declares something agrees with whether it has what goes
/// with it: `has_alone` is the problem of having it undeclared, and
/// `declares_alone` the problem of declaring it without having it.
fn agree(declares: bool, has: bool, has_alone: &str, declares_alone: &str) -> EvalResult<()> {
    match (declares, has) {
        (false, true) => Err(has_alone.into()),
        (true, false) => Err(declares_alone.into()),
        _ => Ok(()),
    }
}

/// What a feature module that names no `inputs` reads: sections, tasks,
/// values and links. `tasks` is the bundled `tasks` module's collection, so
/// it holds nothing while no active module declares it.
pub(crate) fn default_inputs() -> Vec<Collection> {
    vec![
        Collection::Sections,
        Collection::Declared("tasks".into()),
        Collection::Values,
        Collection::Links,
    ]
}
