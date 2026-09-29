//! Reading a workspace from disk: its notes, their imports, the activated
//! modules, the link cache and cached lookups.
use crate::io::{at, read_json_or_default, write_json_atomic};
use lang::eval::Workspace;
use lang::eval::modules::{CompileModules, ModuleRegistry, is_module_path};
use lang::eval::resources::Cache;
use lang::model::Document;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Disk-backed construction and refresh for [`Workspace`]. `Workspace` is
/// `eval`'s, so these are an extension trait rather than inherent methods.
pub trait WorkspaceFiles: Sized {
    fn load(roots: Vec<PathBuf>) -> Result<Self, String>;
    /// An editor's rescan: the notes on disk over the modules it already has.
    /// Unchanged module sources keep that `Arc`, which is how a refresh in
    /// flight tells whether the modules changed under it.
    fn rescan(roots: Vec<PathBuf>, modules: Arc<ModuleRegistry>) -> (Self, Vec<String>);
    /// File commands read only the requested note and its explicit dependencies.
    fn load_file(roots: Vec<PathBuf>, path: &Path) -> Result<Self, String>;
    /// A note that arrived on stdin: one document at a synthetic path inside
    /// the root, so relative imports and modules resolve like a saved note.
    fn load_source(roots: Vec<PathBuf>, path: &Path, text: String) -> Result<Self, String>;
    fn include_file(&mut self, path: &Path) -> Result<(), String>;
    /// Follow explicit imports, including ignored files and files outside roots.
    /// Existing documents win so unsaved editor buffers remain authoritative.
    fn load_imports(&mut self);
}

impl WorkspaceFiles for Workspace {
    fn load(roots: Vec<PathBuf>) -> Result<Self, String> {
        let (mut workspace, errors) = scan_notes(roots);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        reload_modules(&mut workspace)?;
        Ok(workspace)
    }
    fn rescan(roots: Vec<PathBuf>, modules: Arc<ModuleRegistry>) -> (Self, Vec<String>) {
        let (mut workspace, mut errors) = scan_notes(roots);
        workspace.replace_modules(modules);
        if let Err(error) = reload_modules(&mut workspace) {
            errors.push(error);
        }
        (workspace, errors)
    }
    fn load_file(roots: Vec<PathBuf>, path: &Path) -> Result<Self, String> {
        let mut result = empty(roots);
        result.include_file(path)?;
        reload_modules(&mut result)?;
        Ok(result)
    }
    fn load_source(roots: Vec<PathBuf>, path: &Path, text: String) -> Result<Self, String> {
        let mut result = empty(roots);
        result.insert_document(path.into(), Document::parse(text));
        result.load_imports();
        reload_modules(&mut result)?;
        Ok(result)
    }
    fn include_file(&mut self, path: &Path) -> Result<(), String> {
        if !lang::common::is_note(path) {
            return Err(format!("Expected a .{} note", lang::common::EXTENSION));
        }
        if !self.documents().contains_key(path) {
            let text = std::fs::read_to_string(path).map_err(at(path))?;
            self.insert_document(path.into(), Document::parse(text));
        }
        self.load_imports();
        Ok(())
    }
    fn load_imports(&mut self) {
        let mut pending: Vec<_> = self.documents().keys().cloned().collect();
        let mut visited = std::collections::BTreeSet::new();
        while let Some(path) = pending.pop() {
            if !visited.insert(path.clone()) {
                continue;
            }
            let Some(doc) = self.documents().get(&path) else {
                continue;
            };
            let imports: Vec<_> = doc
                .imports
                .iter()
                .filter_map(|id| lang::model::note_path(&path, id).ok())
                .collect();
            for target in imports {
                if !self.documents().contains_key(&target)
                    && let Ok(text) = std::fs::read_to_string(&target)
                {
                    self.insert_document(target.clone(), Document::parse(text));
                }
                pending.push(target);
            }
        }
    }
}

/// Save a workspace's link cache under its root. It takes only what it
/// writes, so an editor can save off its lock without cloning the workspace.
pub fn save_cache(root: &Path, cache: &Cache) -> Result<(), String> {
    write_json_atomic(&root.join(".xmd/cache.json"), cache)
}

/// Notes on disk, as the language services read imports through them.
pub struct DiskFiles;

impl catalog::NoteFiles for DiskFiles {
    fn load_imports(&self, workspace: &mut Workspace) {
        WorkspaceFiles::load_imports(workspace);
    }
    fn include_file(&self, workspace: &mut Workspace, path: &Path) -> Result<(), String> {
        WorkspaceFiles::include_file(workspace, path)
    }
}

/// Editors retain readable notes and cached data even when an unrelated
/// file cannot be read. CLI workspace commands still report scan failures.
fn scan_notes(roots: Vec<PathBuf>) -> (Workspace, Vec<String>) {
    let mut result = empty(roots);
    let mut errors = Vec::new();
    for root in result.roots().to_vec() {
        let walker = ignore::WalkBuilder::new(&root)
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
                && lang::common::is_note(entry.path())
                && !is_module_path(entry.path())
            {
                let text = match std::fs::read_to_string(entry.path()) {
                    Ok(text) => text,
                    Err(error) => {
                        errors.push(at(entry.path())(error));
                        continue;
                    }
                };
                result.insert_document(entry.path().to_path_buf(), Document::parse(text));
            }
        }
    }
    result.load_imports();
    (result, errors)
}

/// Activate the modules on disk, and drop module files from the notes.
fn reload_modules(workspace: &mut Workspace) -> Result<(), String> {
    let modules = load_modules(workspace.roots())?;
    if !workspace.modules().same_sources(&modules) {
        workspace.replace_modules(Arc::new(modules));
    }
    for module in workspace.modules().clone().iter() {
        workspace.remove_document(&module.path);
    }
    Ok(())
}

/// A workspace with no notes yet, holding each root's cache and lookups.
fn empty(roots: Vec<PathBuf>) -> Workspace {
    let mut result = Workspace::new(roots);
    for root in result.roots().to_vec() {
        result.extend_caches(
            read_json_or_default(&root.join(".xmd/cache.json")),
            crate::lookups::load(&root),
        );
    }
    result
}

/// Compile the modules the personal `modules.json` in the config directory
/// and each root's `.xmd/modules.json` activate, over the bundled stdlib.
pub(crate) fn load_modules(roots: &[PathBuf]) -> Result<ModuleRegistry, String> {
    let personal = crate::io::config_dir().map(|dir| dir.join("modules.json"));
    let manifests = personal
        .into_iter()
        .chain(roots.iter().map(|root| root.join(".xmd/modules.json")));
    let mut sources = BTreeMap::new();
    for manifest in manifests {
        let text = match std::fs::read_to_string(&manifest) {
            Ok(v) => v,
            // A root can be a single note (an editor opening one file), and
            // then its `.xmd` is simply not there.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(e) => return Err(at(&manifest)(e)),
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
            let path = lang::model::note_path(&manifest, &entry).map_err(at(&manifest))?;
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
            let source = std::fs::read_to_string(&path).map_err(at(&path))?;
            sources.insert(path, source);
        }
    }
    ModuleRegistry::compile(sources)
}
