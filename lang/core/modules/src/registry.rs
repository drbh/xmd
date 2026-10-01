//! The registry: the compiled set of modules, linked and ready to call.
use crate::link_features::LinkFeatures;
use crate::module::{Declared, Module, ModuleKind, NewEnvironment};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Where the bundled modules live: no file on disk is under this root.
const BUNDLED_ROOT: &str = "/__xmd_stdlib__";

/// The id of the prelude: the library whose exports are names in every note
/// and every other module without an `import`, below the names they define
/// themselves. A workspace module with this id replaces it, as with any
/// bundled id.
pub(crate) const PRELUDE: &str = "prelude";

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
    pub(crate) fn active(&self) -> impl Iterator<Item = &Module> {
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
    /// The active prelude library, whose exports every note and module sees.
    pub fn prelude(&self) -> Option<&Module> {
        self.get(PRELUDE).filter(|m| m.kind == ModuleKind::Library)
    }
    /// The recognizers, attributes and forms the active modules declare, in
    /// manifest order: what a note is read with as it is parsed.
    pub fn recognizers(&self) -> model::recognized::Recognizers {
        model::recognized::Recognizers {
            rules: self
                .active()
                .flat_map(|m| m.recognizes.iter().cloned())
                .collect(),
            attributes: self
                .active()
                .flat_map(|m| m.attributes.iter().cloned())
                .collect(),
            forms: self
                .active()
                .flat_map(|m| m.forms.iter().cloned())
                .collect(),
        }
    }
    /// The active module that declares the collection `name`, with its
    /// declaration.
    pub fn declaring(&self, name: &str) -> Option<(&Module, &Declared)> {
        self.active()
            .find_map(|m| Some((m, m.collections.iter().find(|c| *c.name == *name)?)))
    }
    /// Every collection the active modules declare, in manifest order.
    pub fn declared(&self) -> impl Iterator<Item = (&Module, &Declared)> {
        self.active()
            .flat_map(|m| m.collections.iter().map(move |c| (m, c)))
    }
    /// The active modules of one kind, in manifest order.
    pub fn of_kind(&self, kind: ModuleKind) -> impl Iterator<Item = &Module> {
        self.active().filter(move |m| m.kind == kind)
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
            prelude,
            agenda,
            definitions,
            tasks,
            appointments,
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
            tables,
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
        let mut module = sources
            .get(id)
            .ok_or_else(|| format!("Unknown module import '{id}'"))?
            .clone();
        stack.push(id.into());
        let mut dependencies = module
            .imports
            .iter()
            .map(|id| resolve(id, sources, ready, stack))
            .collect::<Result<Vec<_>, _>>()?;
        // A module reads the clock through what it imports; the prelude every
        // module sees only lends it its functions (one reads the clock
        // only when a note calls it), so it makes no module live.
        module.live = module.own_live || dependencies.iter().any(|m| m.live);
        // Every module sees the prelude, but for the prelude itself and the
        // modules it is built from, which `link` resolves before any other.
        if !module.imports.iter().any(|i| i == PRELUDE)
            && let Some(prelude) = ready.get(PRELUDE)
        {
            dependencies.push(prelude.clone());
        }
        stack.pop();
        let environment = module.environment().with_modules(ModuleRegistry {
            modules: dependencies,
        });
        module.set_environment(environment);
        ready.insert(id.into(), module.clone());
        Ok(module)
    }
    collections(&modules)?;
    let order: Vec<_> = modules.iter().map(|m| m.id.clone()).collect();
    let sources: BTreeMap<String, Module> =
        modules.into_iter().map(|m| (m.id.clone(), m)).collect();
    let mut ready = BTreeMap::new();
    if sources.contains_key(PRELUDE) {
        resolve(PRELUDE, &sources, &mut ready, &mut vec![])?;
    }
    order
        .into_iter()
        .map(|id| resolve(&id, &sources, &mut ready, &mut vec![]))
        .collect()
}

/// A collection, an attribute or a form is declared by one active module,
/// and every collection an active module's `inputs` names is native or
/// declared by one.
fn collections(modules: &[Module]) -> Result<(), String> {
    let active = || modules.iter().filter(|m| m.enabled);
    let mut attributes: BTreeMap<&str, &str> = BTreeMap::new();
    for module in active() {
        for attribute in &module.attributes {
            if let Some(first) = attributes.insert(&attribute.key, &module.id) {
                return Err(format!(
                    "{} and {first} both declare the attribute @{}",
                    module.id, attribute.key
                ));
            }
        }
    }
    // A form is called the way a function is, so it is named by no function
    // a note already calls by name.
    let prelude: Vec<String> = active()
        .find(|m| m.id == PRELUDE && m.kind == ModuleKind::Library)
        .map(Module::public_names)
        .unwrap_or_default();
    let mut forms: BTreeMap<&str, &str> = BTreeMap::new();
    for module in active() {
        for form in &module.forms {
            if let Some(first) = forms.insert(&form.name, &module.id) {
                return Err(format!(
                    "{} and {first} both declare the form {}",
                    module.id, form.name
                ));
            }
            if prelude.contains(&form.name) {
                return Err(format!(
                    "{} declares the form {}, which the prelude exports",
                    module.id, form.name
                ));
            }
        }
    }
    let mut declared: BTreeMap<&str, &str> = BTreeMap::new();
    for module in active() {
        for collection in &module.collections {
            if let Some(first) = declared.insert(&collection.name, &module.id) {
                return Err(format!(
                    "{} and {first} both declare the collection '{}'",
                    module.id, collection.name
                ));
            }
        }
    }
    for module in active() {
        // The default inputs read the tasks collection only when it is there.
        let inputs = (module.inputs != crate::module::default_inputs())
            .then_some(module.inputs.iter())
            .into_iter()
            .flatten();
        for input in inputs.chain(module.sources.keys()) {
            if input.is_declared() && !declared.contains_key(input.as_str()) {
                return Err(format!(
                    "{}: Unknown input collection: {input}",
                    module.path.display()
                ));
            }
        }
    }
    Ok(())
}
