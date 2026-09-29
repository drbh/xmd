//! How the evaluator runs the modules the `modules` crate describes: a module's
//! environment is a `Workspace` over its own note, and compiling or calling
//! one evaluates that note with a module engine.
use crate::{engine::Engine, workspace::Workspace};
use chrono::{DateTime, FixedOffset};
use model::Document;
use modules::{Evaluator, Module, ModuleEnvironment, ModuleRegistry};
use std::{
    any::Any,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use values::{EvalResult, Value};

impl ModuleEnvironment for Workspace {
    fn document(&self, path: &Path) -> &Document {
        &self.documents[path]
    }
    fn resolves(&self, path: &Path, name: &str) -> bool {
        self.resolve(path, name).is_ok()
    }
    fn modules(&self) -> &ModuleRegistry {
        &self.modules
    }
    fn with_modules(&self, modules: ModuleRegistry) -> Arc<dyn ModuleEnvironment> {
        let mut workspace = self.clone();
        workspace.modules = Arc::new(modules);
        Arc::new(workspace)
    }
    fn evaluator<'s>(&'s self, path: &'s Path) -> Box<Evaluator<'s>> {
        let mut engine = Engine::for_module(self, DateTime::UNIX_EPOCH.fixed_offset());
        Box::new(move |name| engine.named(path, name))
    }
    fn call(
        self: Arc<Self>,
        module: &Module,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value> {
        let mut engine = Engine::for_module(&self, now)
            .with_environment(self.clone())
            .with_expressions(module.expressions().clone());
        let function = engine.named(&module.path, name)?;
        engine
            .call(function, args)
            .map_err(|e| e.in_module(&module.id, name))
    }
}

/// A module's own note, alone. Its registry is explicitly empty rather than
/// the bundled set: the bundled modules compile through here while the one
/// copy of that set is still being built.
fn environment(path: &Path, document: Document) -> Arc<dyn ModuleEnvironment> {
    Arc::new(Workspace {
        roots: vec![path.parent().unwrap_or(Path::new(".")).into()],
        documents: [(path.to_path_buf(), document)].into(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: Arc::new(ModuleRegistry::default()),
    })
}

/// The workspace a module's code evaluates in.
pub(crate) fn workspace(module: &Module) -> Arc<Workspace> {
    let environment: Arc<dyn Any + Send + Sync> = module.environment().clone();
    environment
        .downcast()
        .expect("every module environment is a workspace")
}

/// The bundled modules, compiled on first use.
pub(crate) fn bundled() -> ModuleRegistry {
    ModuleRegistry::bundled(environment)
}

/// Compiling a registry evaluates each module's note, which only the
/// evaluator can do. `ModuleRegistry` is the module vocabulary's type, so this
/// is an extension trait rather than an inherent method.
pub trait CompileModules: Sized {
    /// Compile a complete replacement before the caller swaps its Arc snapshot.
    fn compile(sources: BTreeMap<PathBuf, String>) -> Result<Self, String>;
}
impl CompileModules for ModuleRegistry {
    fn compile(sources: BTreeMap<PathBuf, String>) -> Result<Self, String> {
        Self::compile_with(sources, environment)
    }
}
