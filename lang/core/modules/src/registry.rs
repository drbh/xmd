//! The registry: the compiled set of modules, linked and ready to call.
use crate::link_features::LinkFeatures;
use crate::module::{Module, ModuleKind, NewEnvironment};
use chrono::{DateTime, FixedOffset};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use values::{EvalError, EvalResult, Value};

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

/// Where the bundled modules live: no file on disk is under this root.
const BUNDLED_ROOT: &str = "/__xmd_stdlib__";

#[derive(Clone, Debug, Default)]
pub struct ModuleRegistry {
    pub(crate) modules: Vec<Module>,
}
impl ModuleRegistry {
    /// The bundled modules alone, compiled on first use.
    pub fn bundled(environment: NewEnvironment) -> Self {
        Self {
            modules: bundled(environment).to_vec(),
        }
    }
    /// Every module, disabled ones included, in manifest order.
    pub fn iter(&self) -> impl Iterator<Item = &Module> {
        self.modules.iter()
    }
    pub fn active(&self) -> impl Iterator<Item = &Module> {
        self.modules.iter().filter(|m| m.enabled)
    }
    /// Whether every module is a bundled one: the workspace brings none of
    /// its own, so none replaces a bundled id.
    pub fn only_bundled(&self) -> bool {
        self.modules
            .iter()
            .all(|m| m.path.starts_with(BUNDLED_ROOT))
    }
    /// The active module with this id.
    pub fn get(&self, id: &str) -> Option<&Module> {
        self.active().find(|m| m.id == id)
    }
    /// The active modules of one kind, in manifest order.
    pub fn of_kind(&self, kind: ModuleKind) -> impl Iterator<Item = &Module> {
        self.active().filter(move |m| m.kind == kind)
    }
    /// Resolve every call against this immutable workspace snapshot.
    pub fn call(
        &self,
        id: &str,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value> {
        self.get(id)
            .ok_or_else(|| EvalError::ModuleUnavailable(id.into()))?
            .call(name, args, now)
            // The caller asked for this hook by name, so its own attribution
            // would only repeat what the call site already says.
            .map_err(|e| match e {
                EvalError::Module {
                    id: at,
                    hook,
                    source,
                } if at == id && hook == name => *source,
                other => other,
            })
    }
    /// The link modules among them, as a request consults them.
    pub fn link_features(&self) -> LinkFeatures<'_> {
        LinkFeatures::new(&self.modules)
    }
    pub fn same_sources(&self, other: &Self) -> bool {
        self.modules.len() == other.modules.len()
            && self.modules.iter().zip(&other.modules).all(|(a, b)| {
                a.path == b.path
                    && a.environment.document(&a.path).text == b.environment.document(&b.path).text
            })
    }
    /// Compile a complete replacement before the caller swaps its Arc snapshot.
    pub fn compile_with(
        sources: BTreeMap<PathBuf, String>,
        environment: NewEnvironment,
    ) -> Result<Self, String> {
        if sources.len() > 64 {
            return Err("At most 64 modules may be loaded per source layer".into());
        }
        let mut modules = Vec::new();
        let mut ids = BTreeSet::new();
        for (path, source) in sources {
            let module = Module::compile(path.clone(), source, environment)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if !ids.insert(module.id.clone()) {
                return Err(format!("Duplicate module id '{}'", module.id));
            }
            modules.push(module);
        }
        modules.extend(
            bundled(environment)
                .iter()
                .filter(|m| !ids.contains(&m.id))
                .cloned(),
        );
        Ok(Self {
            modules: link(modules)?,
        })
    }
}

/// Each bundled module's directory, id and source, read from
/// `lang/<directory>/<id>.<extension>`.
macro_rules! bundle {
    ($dir:literal: $($id:ident),* $(,)?) => {
        [$((
            $dir,
            stringify!($id),
            include_str!(concat!(
                "../../../",
                $dir,
                "/",
                stringify!($id),
                ".",
                common::library_extension!()
            )),
        )),*]
    };
}

/// Bundled modules use exactly the same compiler and adapters as workspace modules.
fn bundled(environment: NewEnvironment) -> &'static [Module] {
    static MODULES: std::sync::OnceLock<Vec<Module>> = std::sync::OnceLock::new();
    MODULES.get_or_init(|| {
        // The standard library, then bundled plugins: the integrations with
        // outside services, kept apart from the language's own library.
        let modules = bundle!["stdlib":
            agenda,
            definitions,
            tasks,
            references,
            links,
            itinerary_core,
            itinerary,
            timers,
            plans,
            plan,
            timer,
            format,
            task,
            resource,
            today,
            units,
            table_cells,
            checklists,
            calculations,
        ]
        .into_iter()
        .chain(bundle!["plugins": github, rss, frankfurter, yahoo_finance, open_meteo, sync])
        .map(|(dir, id, source)| {
            Module::compile(
                common::library_file(&format!("{BUNDLED_ROOT}/{dir}/{id}")).into(),
                source.into(),
                environment,
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
        module.environment = module.environment.with_modules(ModuleRegistry {
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
