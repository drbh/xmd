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
    pub plugins: std::sync::Arc<crate::evaluate::plugins::Plugins>,
}
impl Workspace {
    #[cfg(feature = "native")]
    pub fn load(roots: Vec<PathBuf>) -> Result<Self, String> {
        let mut workspace = Self::load_notes(roots)?;
        workspace.reload_plugins()?;
        Ok(workspace)
    }
    #[cfg(feature = "native")]
    pub fn reload_plugins(&mut self) -> Result<(), String> {
        let plugins = crate::evaluate::plugins::Plugins::load(&self.roots)?;
        if !self.plugins.same_sources(&plugins) {
            self.plugins = std::sync::Arc::new(plugins);
        }
        Ok(())
    }
    pub fn link_features(&self) -> crate::link_features::LinkFeatures<'_> {
        crate::link_features::BUILTINS.with_plugins(&self.plugins.modules)
    }
    #[cfg(feature = "native")]
    pub(crate) fn load_notes(roots: Vec<PathBuf>) -> Result<Self, String> {
        let mut result = Self {
            roots,
            documents: BTreeMap::new(),
            cache: BTreeMap::new(),
            lookups: BTreeMap::new(),
            plugins: Default::default(),
        };
        for root in &result.roots {
            result.cache.extend(crate::resources::load_cache(root));
            result.lookups.extend(crate::lookups::native::load(root));
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
                {
                    let text = std::fs::read_to_string(entry.path())
                        .map_err(|e| format!("{}: {e}", entry.path().display()))?;
                    result
                        .documents
                        .insert(entry.path().to_path_buf(), Document::parse(text));
                }
            }
        }
        Ok(result)
    }
    pub fn resolve(&self, path: &Path, name: &str) -> Result<Symbol, String> {
        let found: Vec<_> = self
            .symbols()
            .into_iter()
            .filter(|s| self.named(s).name == name)
            .collect();
        let local: Vec<_> = found.iter().filter(|s| s.path == path).cloned().collect();
        let options = if local.is_empty() { found } else { local };
        match options.len() {
            0 => Err(format!("Unknown name '{name}'")),
            1 => Ok(options[0].clone()),
            _ => Err(format!(
                "Ambiguous name '{name}'; use a unique name across notes"
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
    /// Names a plan reads that no note declares: its decision variables.
    pub fn plan_variables<'a>(
        &self,
        _path: &Path,
        plan: &'a crate::plans::Plan,
    ) -> Vec<(usize, &'a Named)> {
        let declared: std::collections::BTreeSet<&str> = self
            .declared()
            .iter()
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
