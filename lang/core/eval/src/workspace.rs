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
/// Every note a request can reach, with the cached data notes read and the
/// modules that extend them. Its state changes only through the named
/// operations below; outside `eval` it is otherwise read-only.
pub struct Workspace {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) documents: BTreeMap<PathBuf, Document>,
    pub(crate) cache: Cache,
    pub(crate) lookups: crate::lookups_impl::Store,
    pub(crate) modules: std::sync::Arc<crate::modules_impl::ModuleRegistry>,
}
impl Workspace {
    /// A workspace over `roots` with no notes, caches or modules of its own
    /// yet; the bundled modules are active.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            documents: BTreeMap::new(),
            cache: BTreeMap::new(),
            lookups: BTreeMap::new(),
            modules: Default::default(),
        }
    }
    /// The directories this workspace was opened on.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
    /// Every loaded note, by path.
    pub fn documents(&self) -> &BTreeMap<PathBuf, Document> {
        &self.documents
    }
    /// Cached link statuses, by resource target.
    pub fn cache(&self) -> &Cache {
        &self.cache
    }
    /// Cached lookup values, by lookup key.
    pub fn lookups(&self) -> &BTreeMap<String, crate::lookups_impl::Lookup> {
        &self.lookups
    }
    /// The active modules: the workspace's own, then the bundled ones.
    pub fn modules(&self) -> &std::sync::Arc<crate::modules_impl::ModuleRegistry> {
        &self.modules
    }
    /// Add or replace a note, returning the one it replaces.
    pub fn insert_document(&mut self, path: PathBuf, document: Document) -> Option<Document> {
        self.documents.insert(path, document)
    }
    /// Drop a note, returning it.
    pub fn remove_document(&mut self, path: &Path) -> Option<Document> {
        self.documents.remove(path)
    }
    /// Keep only the notes `keep` accepts.
    pub fn retain_documents(&mut self, mut keep: impl FnMut(&Path) -> bool) {
        self.documents.retain(|path, _| keep(path));
    }
    /// Record a link's freshly fetched status.
    pub fn store_link_status(&mut self, target: String, status: crate::resources_impl::Metadata) {
        self.cache.insert(target, status);
    }
    /// Record a lookup's freshly fetched value.
    pub fn store_lookup(&mut self, key: String, lookup: crate::lookups_impl::Lookup) {
        self.lookups.insert(key, lookup);
    }
    /// Take the cached link statuses and lookups another snapshot of this
    /// workspace refreshed, such as one refreshed off the editor's lock.
    pub fn adopt_caches(&mut self, refreshed: &Workspace) {
        self.cache = refreshed.cache.clone();
        self.lookups = refreshed.lookups.clone();
    }
    /// Add cached link statuses and lookups read from disk.
    pub fn extend_caches(
        &mut self,
        cache: Cache,
        lookups: BTreeMap<String, crate::lookups_impl::Lookup>,
    ) {
        self.cache.extend(cache);
        self.lookups.extend(lookups);
    }
    /// Activate a freshly compiled module registry.
    pub fn replace_modules(
        &mut self,
        modules: std::sync::Arc<crate::modules_impl::ModuleRegistry>,
    ) {
        self.modules = modules;
    }
    pub fn link_features(&self) -> crate::link_features_impl::LinkFeatures<'_> {
        crate::link_features_impl::BUILTINS.with_modules(&self.modules.modules)
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
}
