//! Hot-reloadable XMD modules: what one is, and how the engine calls into it.
use crate::{
    engine_impl::Value,
    error::{EvalError, EvalResult},
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, Utc};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

mod collection;
mod compile;
mod link;
mod registry;
mod values;

pub use collection::Collection;
pub use registry::ModuleRegistry;
pub(crate) use registry::bundled;
pub use values::{from_json, json, record};
pub(crate) use values::{list, url_value};

/// What a module plugs into. `module.kind` in the source names one of these.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum ModuleKind {
    /// Decorates matching URLs: inlays, hovers, properties and refreshes.
    Link,
    /// Drives editor features over the document catalog.
    Feature,
    /// Plain functions other modules and the engine import by name.
    Library,
}
impl ModuleKind {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// The one hook a module of this kind must supply.
    pub fn required_hook(self) -> Option<Hook> {
        match self {
            Self::Link => Some(Hook::Inlay),
            Self::Feature => Some(Hook::Collect),
            Self::Library => None,
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
    strum::IntoStaticStr,
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
}
impl Hook {
    /// Declaration order is also the order compilation validates them in.
    pub const ALL: &'static [Hook] = <Self as strum::VariantArray>::VARIANTS;
    pub fn name(self) -> &'static str {
        self.into()
    }
    pub fn arity(self) -> usize {
        match self {
            Self::Property | Self::Reduce | Self::Decode => 2,
            _ => 1,
        }
    }
    /// The kind this hook is mandatory for; such hooks are validated first.
    pub fn required_for(self) -> Option<ModuleKind> {
        match self {
            Self::Collect => Some(ModuleKind::Feature),
            Self::Inlay => Some(ModuleKind::Link),
            _ => None,
        }
    }
}

/// Lets `Module::has`/`Module::call` take a typed `Hook` or, for library
/// modules whose exports are user-defined, a plain function name.
pub trait Entry {
    fn entry_name(&self) -> &str;
}
impl Entry for Hook {
    fn entry_name(&self) -> &str {
        self.name()
    }
}
impl Entry for &str {
    fn entry_name(&self) -> &str {
        self
    }
}
impl Entry for String {
    fn entry_name(&self) -> &str {
        self
    }
}

#[derive(Clone, Debug)]
pub struct Module {
    pub id: String,
    pub kind: ModuleKind,
    pub path: PathBuf,
    pub live: bool,
    own_live: bool,
    pub enabled: bool,
    pub inputs: Vec<Collection>,
    pub fields: BTreeMap<Collection, Vec<String>>,
    pub imports: Vec<String>,
    /// The declared public API, or `None` for the older rule that every
    /// non-`_` definition is public. See [`Module::public_names`].
    pub exports: Option<Vec<String>>,
    pub(crate) expressions: Arc<BTreeMap<String, crate::engine_impl::Expr>>,
    hosts: Vec<String>,
    prefix: String,
    properties: Vec<String>,
    cache_key: Option<String>,
    workspace: Arc<Workspace>,
}
impl Module {
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
    pub(crate) fn member_names(&self) -> Vec<String> {
        self.workspace.documents[&self.path]
            .definitions
            .iter()
            .map(|d| d.named.name.as_str())
            .filter(|name| *name != "module" && !name.starts_with('_'))
            .map(str::to_owned)
            .collect()
    }
    pub fn has(&self, entry: impl Entry) -> bool {
        self.workspace
            .resolve(&self.path, entry.entry_name())
            .is_ok()
    }
    pub fn call(
        &self,
        entry: impl Entry,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value> {
        let name = entry.entry_name();
        if !self.enabled {
            return Err(EvalError::ModuleDisabled(self.id.clone()));
        }
        for arg in &args {
            crate::functional_impl::check_size(arg)?;
        }
        let mut engine = crate::engine_impl::Engine::at(&self.workspace, now)
            .pure()
            .module()
            .with_link_features(crate::link_features_impl::LinkFeatures::new(&[]))
            .with_environment(self.workspace.clone())
            .with_expressions(self.expressions.clone());
        let function = engine.named(&self.path, name)?;
        engine
            .call(function, args)
            .map_err(|e| e.in_module(&self.id, name))
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

pub(super) fn epoch() -> DateTime<FixedOffset> {
    DateTime::<Utc>::UNIX_EPOCH.fixed_offset()
}
