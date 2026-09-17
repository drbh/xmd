use crate::{
    document::{Document, Named},
    resources::{self, Cache},
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
}
impl Workspace {
    pub fn load(roots: Vec<PathBuf>) -> Result<Self, String> {
        let mut result = Self {
            roots,
            documents: BTreeMap::new(),
            cache: BTreeMap::new(),
        };
        for root in &result.roots {
            result.cache.extend(resources::load_cache(root));
            let walker = ignore::WalkBuilder::new(root)
                .hidden(true)
                .follow_links(false)
                .require_git(false)
                .filter_entry(|e| {
                    !e.file_type().is_some_and(|t| t.is_dir())
                        || !matches!(
                            e.file_name().to_str(),
                            Some("target" | "node_modules" | ".git" | ".jot")
                        )
                })
                .build();
            for entry in walker {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().is_some_and(|t| t.is_file())
                    && entry.path().extension().is_some_and(|s| s == "jot")
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
        }
    }
    pub fn root(&self) -> &Path {
        self.roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or(Path::new("."))
    }
    pub fn save_cache(&self) -> Result<(), String> {
        let dir = self.root().join(".jot");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.cache).map_err(|e| e.to_string())?;
        // Atomic replacement avoids partially written cache data after a crash.
        let tmp = dir.join(format!("cache-{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("cache.json")).map_err(|e| e.to_string())
    }
}
