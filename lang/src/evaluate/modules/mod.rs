//! Hot-reloadable WTF modules: what one is, and how the engine calls into it.
use crate::{
    catalog::Collection,
    engine::Value,
    error::{EvalError, EvalResult},
    workspace::Workspace,
};
use chrono::{DateTime, FixedOffset, Utc};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

mod compile;
mod link;
mod registry;
mod values;

pub use registry::{ModuleRegistry, bundled};
pub(crate) use values::list;
pub use values::{from_json, json, record, url_value};

/// What a module plugs into. `module.kind` in the source names one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
        match self {
            Self::Link => "link",
            Self::Feature => "feature",
            Self::Library => "library",
        }
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
impl std::str::FromStr for ModuleKind {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "link" => Ok(Self::Link),
            "feature" => Ok(Self::Feature),
            "library" => Ok(Self::Library),
            _ => Err("module.kind must be link, feature, or library".into()),
        }
    }
}
impl std::fmt::Display for ModuleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The fixed entry points a link or feature module may define. Library exports
/// are user-chosen names and stay text; these are the contract the hosts call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    pub const ALL: &'static [Hook] = &[
        Hook::Collect,
        Hook::Inlay,
        Hook::Hover,
        Hook::Property,
        Hook::Refresh,
        Hook::Decode,
        Hook::Matches,
        Hook::PropertyNames,
        Hook::TimeDependent,
        Hook::Actions,
        Hook::Reduce,
        Hook::Hovers,
        Hook::Diagnostics,
        Hook::Format,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Collect => "collect",
            Self::Inlay => "inlay",
            Self::Hover => "hover",
            Self::Property => "property",
            Self::Refresh => "refresh",
            Self::Decode => "decode",
            Self::Matches => "matches",
            Self::PropertyNames => "property_names",
            Self::TimeDependent => "time_dependent",
            Self::Actions => "actions",
            Self::Reduce => "reduce",
            Self::Hovers => "hovers",
            Self::Diagnostics => "diagnostics",
            Self::Format => "format",
        }
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
impl std::fmt::Display for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
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
    pub(crate) expressions: Arc<BTreeMap<String, crate::engine::Expr>>,
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
            crate::evaluate::functional::check_size(arg)?;
        }
        let mut engine = crate::engine::Engine::at(&self.workspace, now)
            .pure()
            .with_link_features(crate::link_features::LinkFeatures::new(&[]))
            .with_environment(self.workspace.clone())
            .with_expressions(self.expressions.clone());
        let function = engine.named(&self.path, name)?;
        engine
            .call(function, args)
            .map_err(|e| e.in_module(&self.id, name))
    }
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

pub(super) fn epoch() -> DateTime<FixedOffset> {
    DateTime::<Utc>::UNIX_EPOCH.fixed_offset()
}
