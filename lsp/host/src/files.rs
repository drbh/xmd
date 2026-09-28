//! Reading a workspace from disk: its notes, their imports, the activated
//! modules, the link cache and cached lookups.
use eval::Workspace;
use eval::modules::{ModuleRegistry, is_module_path};
use eval::resources::Cache;
use model::Document;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Disk-backed construction and refresh for [`Workspace`]. `Workspace` is
/// `eval`'s, so these are an extension trait rather than inherent methods.
pub trait WorkspaceFiles: Sized {
    fn load(roots: Vec<PathBuf>) -> Result<Self, String>;
    /// Editors retain readable notes and cached data even when an unrelated
    /// file cannot be read. CLI workspace commands still report scan failures.
    fn scan_notes(roots: Vec<PathBuf>) -> (Self, Vec<String>);
    /// File commands read only the requested note and its explicit dependencies.
    fn load_file(roots: Vec<PathBuf>, path: &Path) -> Result<Self, String>;
    /// A note that arrived on stdin: one document at a synthetic path inside
    /// the root, so relative imports and modules resolve like a saved note.
    fn load_source(roots: Vec<PathBuf>, path: &Path, text: String) -> Result<Self, String>;
    fn reload_modules(&mut self) -> Result<(), String>;
    fn include_file(&mut self, path: &Path) -> Result<(), String>;
    /// Follow explicit imports, including ignored files and files outside roots.
    /// Existing documents win so unsaved editor buffers remain authoritative.
    fn load_imports(&mut self);
    fn save_cache(&self) -> Result<(), String>;
}

impl WorkspaceFiles for Workspace {
    fn load(roots: Vec<PathBuf>) -> Result<Self, String> {
        let (mut workspace, errors) = Self::scan_notes(roots);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        workspace.reload_modules()?;
        Ok(workspace)
    }
    fn scan_notes(roots: Vec<PathBuf>) -> (Self, Vec<String>) {
        let mut result = empty(roots);
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
                            Some("target" | "node_modules" | ".git" | ".xmd")
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
                    && !is_module_path(entry.path())
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
    fn load_file(roots: Vec<PathBuf>, path: &Path) -> Result<Self, String> {
        let mut result = empty(roots);
        result.include_file(path)?;
        result.reload_modules()?;
        Ok(result)
    }
    fn load_source(roots: Vec<PathBuf>, path: &Path, text: String) -> Result<Self, String> {
        let mut result = empty(roots);
        result.documents.insert(path.into(), Document::parse(text));
        result.load_imports();
        result.reload_modules()?;
        Ok(result)
    }
    fn reload_modules(&mut self) -> Result<(), String> {
        let modules = load_modules(&self.roots)?;
        if !self.modules.same_sources(&modules) {
            self.modules = std::sync::Arc::new(modules);
        }
        for module in &self.modules.modules {
            self.documents.remove(&module.path);
        }
        Ok(())
    }
    fn include_file(&mut self, path: &Path) -> Result<(), String> {
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
    fn load_imports(&mut self) {
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
    fn save_cache(&self) -> Result<(), String> {
        let dir = self.root().join(".xmd");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(&self.cache).map_err(|e| e.to_string())?;
        // Atomic replacement avoids partially written cache data after a crash.
        let tmp = dir.join(format!("cache-{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("cache.json")).map_err(|e| e.to_string())
    }
}

/// A workspace with no notes yet, holding each root's cache and lookups.
fn empty(roots: Vec<PathBuf>) -> Workspace {
    let mut result = Workspace {
        roots,
        documents: BTreeMap::new(),
        cache: BTreeMap::new(),
        lookups: BTreeMap::new(),
        modules: Default::default(),
    };
    for root in &result.roots {
        result.cache.extend(load_cache(root));
        result.lookups.extend(crate::lookups_impl::load(root));
    }
    result
}

fn load_cache(root: &Path) -> Cache {
    std::fs::read(root.join(".xmd/cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Compile the modules each root's `.xmd/modules.json` activates, over the
/// bundled stdlib.
fn load_modules(roots: &[PathBuf]) -> Result<ModuleRegistry, String> {
    let mut sources = BTreeMap::new();
    for root in roots {
        let manifest = root.join(".xmd/modules.json");
        let text = match std::fs::read_to_string(&manifest) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", manifest.display())),
        };
        if text.len() > 65_536 {
            return Err(format!("{} exceeds 64 KiB", manifest.display()));
        }
        let paths: Vec<String> = serde_json::from_str(&text).map_err(|e| {
            format!(
                "{}: expected an array of module file paths: {e}",
                manifest.display()
            )
        })?;
        if paths.len() + sources.len() > 64 {
            return Err("At most 64 workspace modules may be activated".into());
        }
        for entry in paths {
            let path = model::note_path(&manifest, &entry)
                .map_err(|e| format!("{}: {e}", manifest.display()))?;
            if sources.contains_key(&path) {
                return Err(format!(
                    "{} is listed more than once in module manifests",
                    path.display()
                ));
            }
            let metadata = std::fs::metadata(&path).map_err(|e| {
                format!("{} (listed in {}): {e}", path.display(), manifest.display())
            })?;
            if metadata.len() > 65_536 {
                return Err(format!("{} exceeds 64 KiB", path.display()));
            }
            let source =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            sources.insert(path, source);
        }
    }
    ModuleRegistry::compile(sources)
}
