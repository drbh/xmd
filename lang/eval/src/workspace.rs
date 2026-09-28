use crate::resources_impl::Cache;
use model::{Document, Named};
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
impl Symbol {
    pub fn new(path: impl Into<PathBuf>, kind: SymbolKind) -> Self {
        Self {
            path: path.into(),
            kind,
        }
    }
    /// Another symbol in the same note.
    pub fn sibling(&self, kind: SymbolKind) -> Self {
        Self::new(self.path.clone(), kind)
    }
}
#[derive(Clone, Debug)]
pub struct Workspace {
    pub roots: Vec<PathBuf>,
    pub documents: BTreeMap<PathBuf, Document>,
    pub cache: Cache,
    pub lookups: crate::lookups_impl::Store,
    pub modules: std::sync::Arc<crate::modules_impl::ModuleRegistry>,
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
        let modules = crate::modules_impl::ModuleRegistry::load(&self.roots)?;
        if !self.modules.same_sources(&modules) {
            self.modules = std::sync::Arc::new(modules);
        }
        for module in &self.modules.modules {
            self.documents.remove(&module.path);
        }
        Ok(())
    }
    pub fn link_features(&self) -> crate::link_features_impl::LinkFeatures<'_> {
        crate::link_features_impl::BUILTINS.with_modules(&self.modules.modules)
    }
    #[cfg(feature = "native")]
    pub(crate) fn load_notes(roots: Vec<PathBuf>) -> Result<Self, String> {
        let (workspace, errors) = Self::scan_notes(roots);
        if errors.is_empty() {
            Ok(workspace)
        } else {
            Err(errors.join("\n"))
        }
    }
    /// Editors retain readable notes and cached data even when an unrelated
    /// file cannot be read. CLI workspace commands still report scan failures.
    #[cfg(feature = "native")]
    pub fn scan_notes(roots: Vec<PathBuf>) -> (Self, Vec<String>) {
        let mut result = Self::empty_notes(roots);
        let mut errors = Vec::new();
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
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        errors.push(error.to_string());
                        continue;
                    }
                };
                if entry.file_type().is_some_and(|t| t.is_file())
                    && common::is_note(entry.path())
                    && !crate::modules_impl::is_module_path(entry.path())
                {
                    let text = match std::fs::read_to_string(entry.path()) {
                        Ok(text) => text,
                        Err(error) => {
                            errors.push(format!("{}: {error}", entry.path().display()));
                            continue;
                        }
                    };
                    result
                        .documents
                        .insert(entry.path().to_path_buf(), Document::parse(text));
                }
            }
        }
        result.load_imports();
        (result, errors)
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
            result.cache.extend(crate::resources_impl::load_cache(root));
            result
                .lookups
                .extend(crate::lookups_impl::native::load(root));
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
    /// A note that arrived on stdin: one document at a synthetic path inside
    /// the root, so relative imports and modules resolve like a saved note.
    #[cfg(feature = "native")]
    pub fn load_source(roots: Vec<PathBuf>, path: &Path, text: String) -> Result<Self, String> {
        let mut result = Self::empty_notes(roots);
        result.documents.insert(path.into(), Document::parse(text));
        result.load_imports();
        result.reload_modules()?;
        Ok(result)
    }
    #[cfg(feature = "native")]
    pub fn include_file(&mut self, path: &Path) -> Result<(), String> {
        if !common::is_note(path) {
            return Err(format!("Expected a .{} note", common::EXTENSION));
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
    pub fn load_imports(&mut self) {
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
                .filter_map(|id| model::note_path(&path, id).ok())
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
    pub fn resolve(&self, path: &Path, name: &str) -> crate::error::EvalResult<Symbol> {
        let options: Vec<_> = self
            .documents
            .get(path)
            .into_iter()
            .flat_map(|doc| Self::declared_in(path, doc).chain(self.variables_in(path, doc)))
            .filter(|s| self.named(s).name == name)
            .collect();
        match options.len() {
            0 => Err(crate::error::EvalError::UnknownName { name: name.into() }),
            1 => Ok(options[0].clone()),
            _ => Err(crate::error::EvalError::AmbiguousName { name: name.into() }),
        }
    }
    pub fn symbols(&self) -> Vec<Symbol> {
        let mut symbols: Vec<_> = self
            .documents
            .iter()
            .flat_map(|(path, doc)| Self::declared_in(path, doc))
            .collect();
        for (path, doc) in &self.documents {
            symbols.extend(self.variables_in(path, doc));
        }
        symbols
    }
    fn variables_in<'a>(
        &'a self,
        path: &'a Path,
        doc: &'a Document,
    ) -> impl Iterator<Item = Symbol> + 'a {
        doc.plans.iter().enumerate().flat_map(move |(p, plan)| {
            self.plan_variables(path, plan)
                .into_iter()
                .map(move |(i, _)| Symbol::new(path, SymbolKind::Variable(p, i)))
        })
    }
    /// Names a plan reads that its own note does not declare: decision variables.
    pub fn plan_variables<'a>(
        &self,
        path: &Path,
        plan: &'a model::plans::Plan,
    ) -> Vec<(usize, &'a Named)> {
        let declared: std::collections::BTreeSet<&str> = self
            .documents
            .get(path)
            .into_iter()
            .flat_map(|doc| Self::declared_in(path, doc))
            .map(|s| self.named(&s).name.as_str())
            .collect();
        plan.names
            .iter()
            .enumerate()
            .filter(|(_, n)| !declared.contains(n.name.as_str()))
            .collect()
    }
    /// Symbols written down by hand: definitions, named tasks and sections.
    fn declared_in<'a>(path: &'a Path, doc: &'a Document) -> impl Iterator<Item = Symbol> + 'a {
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
            .map(move |kind| Symbol::new(path, kind))
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
