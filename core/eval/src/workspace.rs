use document::{Document, Named};
use modules::Cache;
use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};
use values::{EvalError, EvalResult};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SymbolKind {
    Definition(usize),
    Task(usize),
    Section(usize),
    Column(usize, usize),
    /// A name a form solves for, which its note leaves undefined: (form
    /// index, index into that form's names).
    Variable(usize, usize),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Symbol {
    pub path: PathBuf,
    pub kind: SymbolKind,
}
/// Hashed by the note's file name rather than its whole path: equal paths
/// have equal file names, and every name a note or module reads is a memo
/// lookup by symbol.
impl std::hash::Hash for Symbol {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.file_name().hash(state);
        self.kind.hash(state);
    }
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
    /// Shared, so every engine of a request holds it without a copy.
    pub(crate) lookups: Arc<values::Store>,
    pub(crate) modules: Arc<modules::ModuleRegistry>,
    /// The recognizers and attributes the active modules declare, which
    /// every note is read with as it is added.
    recognizers: document::recognized::Recognizers,
    /// What calls into a module with this as its environment share.
    pub(crate) calls: crate::module_runtime::CallMemo,
    /// The user's home directory, which `~/` links resolve against. The host
    /// supplies it; a browser has none.
    home: Option<PathBuf>,
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
            .map(|(path, doc)| (path.clone(), Arc::new(names_in(path, doc))))
            .collect();
        let recognizers = modules.recognizers();
        let mut workspace = Self {
            roots,
            documents,
            names,
            cache: BTreeMap::new(),
            lookups: Default::default(),
            modules: Arc::new(modules),
            recognizers: Default::default(),
            calls: Default::default(),
            home: None,
        };
        workspace.recognize_with(recognizers);
        workspace
    }
    /// The directories this workspace was opened on.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
    /// The user's home directory, if the host supplied one.
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }
    /// Resolve `~/` links against `home` from now on.
    pub fn set_home(&mut self, home: Option<PathBuf>) {
        self.home = home;
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
    /// Parse a note's text knowing the attributes and forms the active
    /// modules declare, as [`insert_document`](Self::insert_document) would
    /// read it.
    pub fn parse(&self, text: String) -> Document {
        Document::parse_with(text, &self.recognizers.attributes, &self.recognizers.forms)
    }
    /// The form named `name` an active module declares.
    pub(crate) fn form(&self, name: &str) -> Option<&document::forms::Form> {
        self.recognizers
            .forms
            .iter()
            .find(|form| form.name == name)
            .map(Arc::as_ref)
    }
    /// Add or replace a note, returning the one it replaces.
    pub fn insert_document(&mut self, path: PathBuf, mut document: Document) -> Option<Document> {
        document.recognize(&self.recognizers);
        self.names
            .insert(path.clone(), Arc::new(names_in(&path, &document)));
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
        Arc::make_mut(&mut self.lookups).insert(key, lookup);
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
        Arc::make_mut(&mut self.lookups).extend(lookups);
    }
    /// Activate a freshly compiled module registry.
    pub fn replace_modules(&mut self, modules: Arc<modules::ModuleRegistry>) {
        let recognizers = modules.recognizers();
        self.modules = modules;
        self.recognize_with(recognizers);
    }
    /// Recognize every note with `recognizers`, unless they are the ones it
    /// was recognized with already.
    fn recognize_with(&mut self, recognizers: document::recognized::Recognizers) {
        if recognizers == self.recognizers {
            return;
        }
        for (path, document) in self.documents.iter_mut() {
            document.recognize(&recognizers);
            // A note read with other forms solves for other names.
            self.names
                .insert(path.clone(), Arc::new(names_in(path, document)));
        }
        self.recognizers = recognizers;
    }
    pub fn link_features(&self) -> modules::LinkFeatures<'_> {
        self.modules.link_features()
    }
    pub fn resolve(&self, path: &Path, name: &str) -> EvalResult<Symbol> {
        self.resolve_shared(path, name).map(Arc::unwrap_or_clone)
    }
    /// [`Self::resolve`], sharing the workspace's own copy of the symbol.
    pub(crate) fn resolve_shared(&self, path: &Path, name: &str) -> EvalResult<Arc<Symbol>> {
        match self.candidates(path, name) {
            [] => Err(EvalError::UnknownName { name: name.into() }),
            [symbol] => Ok(symbol.clone()),
            _ => Err(EvalError::AmbiguousName { name: name.into() }),
        }
    }
    /// Every symbol `name` may mean in the note at `path`.
    pub(crate) fn candidates(&self, path: &Path, name: &str) -> &[Arc<Symbol>] {
        self.names
            .get(path)
            .and_then(|names| names.get(name))
            .map_or(&[][..], Vec::as_slice)
    }
    pub fn symbols(&self) -> Vec<Symbol> {
        let declared = self
            .documents
            .iter()
            .flat_map(|(path, doc)| declared_in(doc).map(move |kind| Symbol::new(path, kind)));
        let variables = self
            .documents
            .iter()
            .flat_map(|(path, doc)| variables_in(doc).map(move |kind| Symbol::new(path, kind)));
        declared.chain(variables).collect()
    }
    /// The symbols of the note at `path`, in the order [`Self::symbols`]
    /// lists them.
    pub fn symbols_in(&self, path: &Path) -> Vec<Symbol> {
        self.documents.get(path).map_or(vec![], |doc| {
            declared_in(doc)
                .chain(variables_in(doc))
                .map(|kind| Symbol::new(path, kind))
                .collect()
        })
    }
    /// The names a form solves for that its own note does not declare,
    /// with their index in its names: none unless its unknowns are free.
    pub fn claimed<'a>(
        &self,
        path: &Path,
        formed: &'a document::forms::Formed,
    ) -> Vec<(usize, &'a Named)> {
        self.documents
            .get(path)
            .map_or(vec![], |doc| undeclared(doc, formed))
    }
    pub fn named<'a>(&'a self, symbol: &Symbol) -> &'a Named {
        named(&self.documents[&symbol.path], &symbol.kind)
    }
    pub fn root(&self) -> &Path {
        self.roots.first().map_or(Path::new("."), PathBuf::as_path)
    }
}

/// A note's symbols by name, in the order [`Workspace::symbols`] lists them.
type Names = BTreeMap<String, Vec<Arc<Symbol>>>;

fn names_in(path: &Path, doc: &Document) -> Names {
    let mut names = Names::new();
    for kind in declared_in(doc).chain(variables_in(doc)) {
        names
            .entry(named(doc, &kind).name.clone())
            .or_default()
            .push(Arc::new(Symbol::new(path, kind)));
    }
    names
}
fn variables_in(doc: &Document) -> impl Iterator<Item = SymbolKind> + '_ {
    doc.forms().iter().enumerate().flat_map(move |(f, formed)| {
        undeclared(doc, formed)
            .into_iter()
            .map(move |(i, _)| SymbolKind::Variable(f, i))
    })
}
/// The names `formed` reads that `doc` does not declare, when the form
/// solves for those.
fn undeclared<'a>(doc: &Document, formed: &'a document::forms::Formed) -> Vec<(usize, &'a Named)> {
    if formed.form.unknowns != document::forms::Unknowns::Free {
        return vec![];
    }
    let declared: std::collections::BTreeSet<&str> = declared_in(doc)
        .map(|kind| named(doc, &kind).name.as_str())
        .collect();
    formed
        .names
        .iter()
        .enumerate()
        .filter(|(_, n)| !declared.contains(n.name.as_str()))
        .collect()
}
/// Symbols written down by hand: definitions, named tasks and sections.
fn declared_in(doc: &Document) -> impl Iterator<Item = SymbolKind> + '_ {
    doc.definitions()
        .iter()
        .enumerate()
        .map(|(i, _)| SymbolKind::Definition(i))
        .chain(
            doc.tasks()
                .iter()
                .enumerate()
                .filter(|(_, t)| t.named.is_some())
                .map(|(i, _)| SymbolKind::Task(i)),
        )
        .chain(
            doc.sections()
                .iter()
                .enumerate()
                .filter(|(_, s)| s.named.is_some())
                .map(|(i, _)| SymbolKind::Section(i)),
        )
}
fn named<'a>(doc: &'a Document, kind: &SymbolKind) -> &'a Named {
    match *kind {
        SymbolKind::Definition(i) => &doc.definitions()[i].named,
        SymbolKind::Task(i) => doc.tasks()[i].named.as_ref().unwrap(),
        SymbolKind::Section(i) => doc.sections()[i].named.as_ref().unwrap(),
        SymbolKind::Column(table, column) => &doc.tables()[table].columns[column],
        SymbolKind::Variable(form, name) => &doc.forms()[form].names[name],
    }
}
