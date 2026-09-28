//! The module compiler: parse a module's source and validate its contract.
//!
//! A module is a `.wtf` file whose `module :=` record says what it is:
//! `{api: 1, id, kind, inputs?, imports?, hosts?, path_prefix?, properties?,
//! enabled?, cache_version?, cache_namespace?, exports?}`.
//!
//! `exports` is an optional list of text naming a library's public API.
//! `import(id)` from a note returns exactly those members, the reference lists
//! exactly those, and completion offers exactly those. Each name must be a
//! top-level definition that is neither `module` nor `_`-prefixed, and may
//! appear once. A library that declares no `exports` keeps the older rule,
//! every non-`_` definition is public; `exports: []` is a library only the
//! engine and other modules call. Link and feature modules have hooks, not
//! exports, so for them the field must be absent or empty. Another module's
//! `imports:` is not bound by `exports`: module code may reach any non-`_`
//! name of a library it declares (see `Module::member_names`).
use super::{
    Collection, Hook, Module, ModuleKind, ModuleRegistry, epoch,
    values::{strings, text},
};
use crate::{
    engine_impl::{Engine, Lexeme, Value},
    error::{EvalError, EvalResult},
    workspace::Workspace,
};
use model::Document;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use url::Url;

impl Module {
    pub fn compile(path: PathBuf, source: String) -> EvalResult<Self> {
        if source.len() > 65_536 {
            return Err(EvalError::Message("Modules are limited to 64 KiB".into()));
        }
        let document = Document::parse(source);
        if let Some(problem) = document.problems.first() {
            return Err(EvalError::Message(problem.message.clone()));
        }
        let mut names = BTreeSet::new();
        let mut live = false;
        let mut expressions = BTreeMap::new();
        for def in &document.definitions {
            if !names.insert(def.named.name.clone()) {
                return Err(EvalError::Message(format!(
                    "Duplicate definition '{}'",
                    def.named.name
                )));
            }
            if !def.expression {
                return Err(EvalError::Message("Module definitions must use :=".into()));
            }
            expressions.insert(
                def.source.clone(),
                crate::engine_impl::Parser::parse(&def.source)
                    .map_err(|e| format!("{}:{}: {e}", path.display(), def.value_span.line + 1))?,
            );
            live |= crate::engine_impl::lex(&def.source)
                .map_err(EvalError::Parse)?
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
            .module()
            .with_link_features(crate::link_features_impl::LinkFeatures::new(&[]));
        let Value::Record(config) = engine.named(&path, "module")? else {
            return Err(EvalError::Message("module must be a record".into()));
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
        let kind: ModuleKind = text(config.get("kind").ok_or("module.kind is required")?)?
            .parse()
            .map_err(|_| "module.kind must be link, feature, or library")?;
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
            other => other.map(strings).transpose()?.unwrap_or_default(),
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
            .any(|p| !model::identifier(p) || matches!(p.as_str(), "url" | "exists"))
        {
            return Err("Invalid or reserved property name".into());
        }
        // A library's exports are its own names, so only link and feature
        // modules are checked against the hook table: required hook first.
        let required = kind.required_hook();
        for hook in required.into_iter().chain(
            Hook::ALL
                .iter()
                .copied()
                .filter(|_| required.is_some())
                .filter(|h| h.required_for().is_none()),
        ) {
            let name = hook.name();
            let arity = hook.arity();
            if names.contains(name) {
                if !matches!(engine.named(&path,name)?,Value::Function(f) if f.params.len()==arity)
                {
                    return Err(EvalError::Message(format!(
                        "{name} must be a function with {arity} parameters"
                    )));
                }
            } else if Some(hook) == required
                && enabled
                && (kind == ModuleKind::Link
                    || ![Hook::Actions, Hook::Hovers, Hook::Diagnostics, Hook::Format]
                        .iter()
                        .any(|h| names.contains(h.name())))
            {
                return Err(EvalError::Message(format!("Missing {name} function")));
            }
        }
        if names.contains(Hook::Refresh.name()) != names.contains(Hook::Decode.name()) {
            return Err("refresh and decode must be supplied together".into());
        }
        if !properties.is_empty() && !names.contains(Hook::Property.name()) {
            return Err("Declared properties need a property function".into());
        }
        let exports = config
            .get("exports")
            .map(strings)
            .transpose()
            .map_err(|_| EvalError::Message("module.exports must be a list of text".into()))?;
        if let Some(exports) = &exports {
            if kind != ModuleKind::Library && !exports.is_empty() {
                return Err(EvalError::Message(format!(
                    "A {kind} module has no exports; its hooks are called by the host"
                )));
            }
            let mut seen = BTreeSet::new();
            for name in exports {
                if name == "module" || name.starts_with('_') {
                    return Err(EvalError::Message(format!(
                        "'{name}' cannot be exported; 'module' and '_' names are private"
                    )));
                }
                if !names.contains(name) {
                    return Err(EvalError::Message(format!(
                        "exports names '{name}', which this module does not define"
                    )));
                }
                if !seen.insert(name) {
                    return Err(EvalError::Message(format!("Duplicate export '{name}'")));
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
            exports,
            cache_key,
            expressions: Arc::new(expressions),
            workspace,
        })
    }
}
