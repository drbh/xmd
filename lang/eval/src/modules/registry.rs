//! The registry: the compiled set of modules, linked and ready to call.
use super::Module;
use crate::{
    engine_impl::Value,
    error::{EvalError, EvalResult},
};
use chrono::{DateTime, FixedOffset};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
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
impl ModuleRegistry {
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
    ) -> EvalResult<Value> {
        self.active()
            .find(|m| m.id == id)
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
            return Err("At most 64 modules may be loaded per source layer".into());
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
        let mut sources = BTreeMap::new();
        for root in roots {
            let manifest = root.join(".wtf/modules.json");
            let text = match std::fs::read_to_string(&manifest) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("{}: {e}", manifest.display())),
            };
            if text.len() > 65_536 {
                return Err(format!("{} exceeds 64 KiB", manifest.display()));
            }
            let paths: Vec<String> = serde_json::from_str(&text).map_err(|e| {
                format!(
                    "{}: expected an array of module file paths: {e}",
                    manifest.display()
                )
            })?;
            if paths.len() + sources.len() > 64 {
                return Err("At most 64 workspace modules may be activated".into());
            }
            for entry in paths {
                let path = model::note_path(&manifest, &entry)
                    .map_err(|e| format!("{}: {e}", manifest.display()))?;
                if sources.contains_key(&path) {
                    return Err(format!(
                        "{} is listed more than once in module manifests",
                        path.display()
                    ));
                }
                let metadata = std::fs::metadata(&path).map_err(|e| {
                    format!("{} (listed in {}): {e}", path.display(), manifest.display())
                })?;
                if metadata.len() > 65_536 {
                    return Err(format!("{} exceeds 64 KiB", path.display()));
                }
                let source = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                sources.insert(path, source);
            }
        }
        Self::compile(sources)
    }
}

/// Bundled modules use exactly the same compiler and adapters as workspace modules.
pub(crate) fn bundled() -> &'static [Module] {
    static MODULES: std::sync::OnceLock<Vec<Module>> = std::sync::OnceLock::new();
    MODULES.get_or_init(|| {
        let modules = [
            ("agenda", include_str!("../../../stdlib/agenda.wtf")),
            (
                "definitions",
                include_str!("../../../stdlib/definitions.wtf"),
            ),
            ("tasks", include_str!("../../../stdlib/tasks.wtf")),
            ("references", include_str!("../../../stdlib/references.wtf")),
            ("links", include_str!("../../../stdlib/links.wtf")),
            (
                "itinerary_core",
                include_str!("../../../stdlib/itinerary_core.wtf"),
            ),
            ("itinerary", include_str!("../../../stdlib/itinerary.wtf")),
            ("timers", include_str!("../../../stdlib/timers.wtf")),
            ("plans", include_str!("../../../stdlib/plans.wtf")),
            ("plan", include_str!("../../../stdlib/plan.wtf")),
            ("timer", include_str!("../../../stdlib/timer.wtf")),
            ("format", include_str!("../../../stdlib/format.wtf")),
            ("units", include_str!("../../../stdlib/units.wtf")),
            ("github", include_str!("../../../stdlib/github.wtf")),
            ("rss", include_str!("../../../stdlib/rss.wtf")),
            (
                "table_cells",
                include_str!("../../../stdlib/table_cells.wtf"),
            ),
            ("checklists", include_str!("../../../stdlib/checklists.wtf")),
            (
                "calculations",
                include_str!("../../../stdlib/calculations.wtf"),
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
