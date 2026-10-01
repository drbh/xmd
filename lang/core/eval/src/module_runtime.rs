//! How the evaluator runs the modules the `modules` crate describes: a module's
//! environment is a `Workspace` over its own note, and compiling or calling
//! one evaluates that note with a module engine.
use crate::{engine::Engine, memo::Memo, workspace::Workspace};
use chrono::{DateTime, FixedOffset};
use model::Document;
use modules::{Evaluator, Module, ModuleEnvironment, ModuleRegistry};
use std::{
    any::Any,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
        let mut engine = Engine::for_module(self, modules::no_clock());
        Box::new(move |name| engine.named(path, name))
    }
    fn call(
        self: Arc<Self>,
        module: &Module,
        name: &str,
        args: Vec<Value>,
        now: DateTime<FixedOffset>,
    ) -> EvalResult<Value> {
        let mut engine = Engine::for_module_sharing(&self, now, self.calls.at(now))
            .with_environment(self.clone())
            .with_expressions(module.expressions().clone());
        // What the call is handed sets how much it may do with it. Measured
        // once, its parts are known when the values built of them are.
        let cap = values::Size {
            items: usize::MAX,
            bytes: usize::MAX,
        };
        let input = args
            .iter()
            .fold(values::Size { items: 0, bytes: 0 }, |total, arg| {
                let size = values::Size::of(arg, cap);
                values::Size {
                    items: total.items.saturating_add(size.items),
                    bytes: total.bytes.saturating_add(size.bytes),
                }
            });
        engine.budget = crate::engine::Budget::scaled(input);
        let result = engine
            .named(&module.path, name)
            .and_then(|function| engine.call(function, args))
            .map_err(|e| e.in_module(&module.id, name));
        if engine.time_dependent() {
            crate::memo::clock_read();
        }
        result
    }
}

/// The memo a module's calls share while the clock stands still, so its
/// top-level definitions — `fmt := import("format")`, its helpers — evaluate
/// once per clock rather than once per call. Module code is pure but for the
/// clock, and a different clock starts a fresh memo. A clone starts empty: a
/// cloned environment may see other modules.
#[derive(Default)]
pub(crate) struct CallMemo(Mutex<Option<(DateTime<FixedOffset>, Memo)>>);
impl CallMemo {
    fn at(&self, now: DateTime<FixedOffset>) -> Memo {
        let mut slot = self.0.lock().expect("module memo poisoned");
        match &*slot {
            // The offset decides what day it is, so it is part of the clock.
            Some((at, memo)) if *at == now && at.offset() == now.offset() => memo.clone(),
            _ => slot.insert((now, Memo::default())).1.clone(),
        }
    }
}
impl Clone for CallMemo {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl std::fmt::Debug for CallMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CallMemo")
    }
}

/// A module's own note, alone. Its registry is explicitly empty rather than
/// the bundled set: the bundled modules compile through here while the one
/// copy of that set is still being built.
fn environment(path: &Path, document: Document) -> Arc<dyn ModuleEnvironment> {
    Arc::new(Workspace::with(
        vec![path.parent().unwrap_or(Path::new(".")).into()],
        [(path.to_path_buf(), document)].into(),
        ModuleRegistry::default(),
    ))
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
    /// A module that takes a stdlib id is also checked against the stdlib
    /// contract, so a replacement missing a function the engine calls fails
    /// here, with the other module problems, rather than at the call.
    fn compile(sources: BTreeMap<PathBuf, String>) -> Result<Self, String> {
        let registry = Self::compile_with(sources, environment)?;
        let problems = crate::contract::check(&registry);
        if problems.is_empty() {
            return Ok(registry);
        }
        Err(problems
            .iter()
            .map(|(path, message)| format!("{}: {message}", path.display()))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
