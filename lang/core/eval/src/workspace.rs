use model::{Document, Named};
use modules::Cache;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
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
    /// Each note's symbols by name, kept with the notes so that resolving a
    /// name is a lookup rather than a scan of the note.
    names: BTreeMap<PathBuf, Arc<Names>>,
    pub(crate) cache: Cache,
    pub(crate) lookups: values::Store,
    pub(crate) modules: Arc<modules::ModuleRegistry>,
    /// What calls into a module with this as its environment share.
    pub(crate) calls: crate::module_runtime::CallMemo,
}
impl Workspace {
    /// A workspace over `roots` with no notes, caches or modules of its own
    /// yet; the bundled modules are active.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self::with(roots, BTreeMap::new(), crate::module_runtime::bundled())
    }
    /// A workspace over `roots` holding `documents`, with `modules` active and
    /// no caches yet.
    pub(crate) fn with(
        roots: Vec<PathBuf>,
        documents: BTreeMap<PathBuf, Document>,
        modules: modules::ModuleRegistry,
    ) -> Self {
        let names = documents
            .iter()
            .map(|(path, doc)| (path.clone(), Arc::new(names_in(doc))))
            .collect();
        Self {
            roots,
            documents,
            names,
            cache: BTreeMap::new(),
            lookups: BTreeMap::new(),
            modules: Arc::new(modules),
            calls: Default::default(),
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
    pub fn lookups(&self) -> &values::Store {
        &self.lookups
    }
    /// The active modules: the workspace's own, then the bundled ones.
    pub fn modules(&self) -> &Arc<modules::ModuleRegistry> {
        &self.modules
    }
    /// Add or replace a note, returning the one it replaces.
    pub fn insert_document(&mut self, path: PathBuf, document: Document) -> Option<Document> {
        self.names
            .insert(path.clone(), Arc::new(names_in(&document)));
        self.documents.insert(path, document)
    }
    /// Drop a note, returning it.
    pub fn remove_document(&mut self, path: &Path) -> Option<Document> {
        self.names.remove(path);
        self.documents.remove(path)
    }
    /// Keep only the notes `keep` accepts.
    pub fn retain_documents(&mut self, mut keep: impl FnMut(&Path) -> bool) {
        self.documents.retain(|path, _| keep(path));
        let documents = &self.documents;
        self.names.retain(|path, _| documents.contains_key(path));
    }
    /// Record a link's freshly fetched status.
    pub fn store_link_status(&mut self, target: String, status: modules::Metadata) {
        self.cache.insert(target, status);
    }
    /// Record a lookup's freshly fetched value.
    pub fn store_lookup(&mut self, key: String, lookup: values::Lookup) {
        self.lookups.insert(key, lookup);
    }
    /// Take the cached link statuses and lookups another snapshot of this
    /// workspace refreshed, such as one refreshed off the editor's lock.
    pub fn adopt_caches(&mut self, refreshed: &Workspace) {
        self.cache = refreshed.cache.clone();
        self.lookups = refreshed.lookups.clone();
    }
    /// Add cached link statuses and lookups read from disk.
    pub fn extend_caches(&mut self, cache: Cache, lookups: values::Store) {
        self.cache.extend(cache);
        self.lookups.extend(lookups);
    }
    /// Activate a freshly compiled module registry.
    pub fn replace_modules(&mut self, modules: Arc<modules::ModuleRegistry>) {
        self.modules = modules;
    }
    pub fn link_features(&self) -> modules::LinkFeatures<'_> {
        self.modules.link_features()
    }
    pub fn resolve(&self, path: &Path, name: &str) -> values::EvalResult<Symbol> {
        let options = self
            .names
            .get(path)
            .and_then(|names| names.get(name))
            .map_or(&[][..], Vec::as_slice);
        match options {
            [] => Err(values::EvalError::UnknownName { name: name.into() }),
            [kind] => Ok(Symbol::new(path, kind.clone())),
            _ => Err(values::EvalError::AmbiguousName { name: name.into() }),
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
        variables_in(doc).map(move |kind| Symbol::new(path, kind))
    }
    /// Names a plan reads that its own note does not declare: decision variables.
    pub fn plan_variables<'a>(
        &self,
        path: &Path,
        plan: &'a model::plans::Plan,
    ) -> Vec<(usize, &'a Named)> {
        match self.documents.get(path) {
            Some(doc) => undeclared(doc, plan),
            None => plan.names.iter().enumerate().collect(),
        }
    }
    /// Symbols written down by hand: definitions, named tasks and sections.
    fn declared_in<'a>(path: &'a Path, doc: &'a Document) -> impl Iterator<Item = Symbol> + 'a {
        declared_in(doc).map(move |kind| Symbol::new(path, kind))
    }
    pub fn named<'a>(&'a self, symbol: &Symbol) -> &'a Named {
        named(&self.documents[&symbol.path], &symbol.kind)
    }
    pub fn root(&self) -> &Path {
        self.roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or(Path::new("."))
    }
}

/// A note's symbols by name, in the order [`Workspace::symbols`] lists them.
type Names = BTreeMap<String, Vec<SymbolKind>>;

fn names_in(doc: &Document) -> Names {
    let mut names = Names::new();
    for kind in declared_in(doc).chain(variables_in(doc)) {
        names
            .entry(named(doc, &kind).name.clone())
            .or_default()
            .push(kind);
    }
    names
}
fn variables_in(doc: &Document) -> impl Iterator<Item = SymbolKind> + '_ {
    doc.plans.iter().enumerate().flat_map(move |(p, plan)| {
        undeclared(doc, plan)
            .into_iter()
            .map(move |(i, _)| SymbolKind::Variable(p, i))
    })
}
/// The names `plan` reads that `doc` does not declare.
fn undeclared<'a>(doc: &Document, plan: &'a model::plans::Plan) -> Vec<(usize, &'a Named)> {
    let declared: std::collections::BTreeSet<&str> = declared_in(doc)
        .map(|kind| named(doc, &kind).name.as_str())
        .collect();
    plan.names
        .iter()
        .enumerate()
        .filter(|(_, n)| !declared.contains(n.name.as_str()))
        .collect()
}
fn declared_in(doc: &Document) -> impl Iterator<Item = SymbolKind> + '_ {
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
}
fn named<'a>(doc: &'a Document, kind: &SymbolKind) -> &'a Named {
    match *kind {
        SymbolKind::Definition(i) => &doc.definitions[i].named,
        SymbolKind::Task(i) => doc.tasks[i].named.as_ref().unwrap(),
        SymbolKind::Section(i) => doc.sections[i].named.as_ref().unwrap(),
        SymbolKind::Column(table, column) => &doc.tables[table].columns[column],
        SymbolKind::Variable(plan, name) => &doc.plans[plan].names[name],
    }
}
