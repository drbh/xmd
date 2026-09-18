use crate::{
    document::{Document, Named},
    resources::Cache,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolKind {
    Definition(usize),
    Task(usize),
    Section(usize),
    Column(usize, usize),
    /// A decision variable: (plan index, index into that plan's names).
    Variable(usize, usize),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Symbol {
    pub path: PathBuf,
    pub kind: SymbolKind,
}
#[derive(Clone, Debug)]
pub struct Workspace {
    pub roots: Vec<PathBuf>,
    pub documents: BTreeMap<PathBuf, Document>,
    pub cache: Cache,
    pub lookups: crate::lookups::Store,
    pub modules: std::sync::Arc<crate::evaluate::modules::ModuleRegistry>,
}
impl Workspace {
    #[cfg(feature = "native")]
    pub fn load(roots: Vec<PathBuf>) -> Result<Self, String> {
        let mut workspace = Self::load_notes(roots)?;
        workspace.reload_modules()?;
        Ok(workspace)
    }
    #[cfg(feature = "native")]
    pub fn reload_modules(&mut self) -> Result<(), String> {
        let modules = crate::evaluate::modules::ModuleRegistry::load(&self.roots)?;
        if !self.modules.same_sources(&modules) {
            self.modules = std::sync::Arc::new(modules);
        }
        for module in &self.modules.modules {
            self.documents.remove(&module.path);
        }
        Ok(())
    }
    pub fn link_features(&self) -> crate::link_features::LinkFeatures<'_> {
        crate::link_features::BUILTINS.with_modules(&self.modules.modules)
    }
    #[cfg(feature = "native")]
    pub(crate) fn load_notes(roots: Vec<PathBuf>) -> Result<Self, String> {
        let mut result = Self::empty_notes(roots);
        for root in &result.roots {
            let walker = ignore::WalkBuilder::new(root)
                .hidden(true)
                .follow_links(false)
                .require_git(false)
                .filter_entry(|e| {
                    !e.file_type().is_some_and(|t| t.is_dir())
                        || !matches!(
                            e.file_name().to_str(),
                            Some("target" | "node_modules" | ".git" | ".wtf")
                        )
                })
                .build();
            for entry in walker {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().is_some_and(|t| t.is_file())
                    && entry.path().extension().is_some_and(|s| s == "wtf")
                    && !crate::modules::is_module_path(entry.path())
                {
                    let text = std::fs::read_to_string(entry.path())
                        .map_err(|e| format!("{}: {e}", entry.path().display()))?;
                    result
                        .documents
                        .insert(entry.path().to_path_buf(), Document::parse(text));
                }
            }
        }
        result.load_imports();
        Ok(result)
    }
    #[cfg(feature = "native")]
    fn empty_notes(roots: Vec<PathBuf>) -> Self {
        let mut result = Self {
            roots,
            documents: BTreeMap::new(),
            cache: BTreeMap::new(),
            lookups: BTreeMap::new(),
            modules: Default::default(),
        };
        for root in &result.roots {
            result.cache.extend(crate::resources::load_cache(root));
            result.lookups.extend(crate::lookups::native::load(root));
        }
        result
    }
    /// File commands read only the requested note and its explicit dependencies.
    #[cfg(feature = "native")]
    pub fn load_file(roots: Vec<PathBuf>, path: &Path) -> Result<Self, String> {
        let mut result = Self::empty_notes(roots);
        result.include_file(path)?;
        result.reload_modules()?;
        Ok(result)
    }
    #[cfg(feature = "native")]
    pub(crate) fn include_file(&mut self, path: &Path) -> Result<(), String> {
        if path.extension().is_none_or(|s| s != "wtf") {
            return Err("Expected a .wtf note".into());
        }
        if !self.documents.contains_key(path) {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            self.documents.insert(path.into(), Document::parse(text));
        }
        self.load_imports();
        Ok(())
    }
    /// Follow explicit imports, including ignored files and files outside roots.
    /// Existing documents win so unsaved editor buffers remain authoritative.
    #[cfg(feature = "native")]
    pub(crate) fn load_imports(&mut self) {
        let mut pending: Vec<_> = self.documents.keys().cloned().collect();
        let mut visited = std::collections::BTreeSet::new();
        while let Some(path) = pending.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            let Some(doc) = self.documents.get(&path) else {
                continue;
            };
            let imports: Vec<_> = doc
                .imports
                .iter()
                .filter_map(|id| super::imports::note_path(&path, id).ok())
                .collect();
            for target in imports {
                if !self.documents.contains_key(&target)
                    && let Ok(text) = std::fs::read_to_string(&target)
                {
                    self.documents.insert(target.clone(), Document::parse(text));
                }
                pending.push(target);
            }
        }
    }
    pub fn resolve(&self, path: &Path, name: &str) -> Result<Symbol, String> {
        let options: Vec<_> = self
            .symbols()
            .into_iter()
            .filter(|s| s.path == path && self.named(s).name == name)
            .collect();
        match options.len() {
            0 => Err(format!("Unknown name '{name}'")),
            1 => Ok(options[0].clone()),
            _ => Err(format!(
                "Ambiguous name '{name}'; use a unique name within this note"
            )),
        }
    }
    pub fn symbols(&self) -> Vec<Symbol> {
        let mut symbols = self.declared();
        for (path, doc) in &self.documents {
            for (p, plan) in doc.plans.iter().enumerate() {
                symbols.extend(
                    self.plan_variables(path, plan)
                        .into_iter()
                        .map(|(i, _)| Symbol {
                            path: path.clone(),
                            kind: SymbolKind::Variable(p, i),
                        }),
                );
            }
        }
        symbols
    }
    /// Names a plan reads that its own note does not declare: decision variables.
    pub fn plan_variables<'a>(
        &self,
        path: &Path,
        plan: &'a crate::plans::Plan,
    ) -> Vec<(usize, &'a Named)> {
        let declared: std::collections::BTreeSet<&str> = self
            .declared()
            .iter()
            .filter(|s| s.path == path)
            .map(|s| self.named(s).name.as_str())
            .collect::<Vec<_>>()
            .into_iter()
            .collect();
        plan.names
            .iter()
            .enumerate()
            .filter(|(_, n)| !declared.contains(n.name.as_str()))
            .collect()
    }
    /// Symbols written down by hand: definitions, named tasks and sections.
    fn declared(&self) -> Vec<Symbol> {
        self.documents
            .iter()
            .flat_map(|(path, doc)| {
                doc.definitions
                    .iter()
                    .enumerate()
                    .map(|(i, _)| SymbolKind::Definition(i))
                    .chain(
                        doc.tasks
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| t.named.is_some())
                            .map(|(i, _)| SymbolKind::Task(i)),
                    )
                    .chain(
                        doc.sections
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| s.named.is_some())
                            .map(|(i, _)| SymbolKind::Section(i)),
                    )
                    .map(|kind| Symbol {
                        path: path.clone(),
                        kind,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    pub fn named<'a>(&'a self, symbol: &Symbol) -> &'a Named {
        let doc = &self.documents[&symbol.path];
        match symbol.kind {
            SymbolKind::Definition(i) => &doc.definitions[i].named,
            SymbolKind::Task(i) => doc.tasks[i].named.as_ref().unwrap(),
            SymbolKind::Section(i) => doc.sections[i].named.as_ref().unwrap(),
            SymbolKind::Column(table, column) => &doc.tables[table].columns[column],
            SymbolKind::Variable(plan, name) => &doc.plans[plan].names[name],
        }
    }
    pub fn root(&self) -> &Path {
        self.roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or(Path::new("."))
    }
    #[cfg(feature = "native")]
    pub fn save_cache(&self) -> Result<(), String> {
        let dir = self.root().join(".wtf");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.cache).map_err(|e| e.to_string())?;
        // Atomic replacement avoids partially written cache data after a crash.
        let tmp = dir.join(format!("cache-{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("cache.json")).map_err(|e| e.to_string())
    }
}
