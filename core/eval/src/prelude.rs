//! The prelude: the library (`modules::PRELUDE`) whose exports are names in
//! every note and every other module, without an `import`. A name resolves
//! to what the code itself defines first — a call's locals, a host's
//! bindings, the note's own definitions — and to the prelude only when
//! nothing there answers, so a note that defines `total` reads its own, and a
//! new prelude function never changes a note that already uses its name.
//!
//! Its functions are described the way every library export is, by the `//`
//! comment above the definition, so signature help, completion, hover and the
//! functions reference read one description.
use crate::engine::{Engine, Expr};
use crate::workspace::Workspace;
use std::path::Path;
use values::EvalResult;

/// One function the prelude gives every note, as the editor describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreludeFunction {
    pub name: String,
    /// Its parameters, a trailing `?` on one a call may leave out.
    pub params: Vec<String>,
    /// The `//` comment above its definition, lines joined by spaces.
    pub documentation: String,
    /// The kind each argument takes, in order, as the prelude declares it
    /// in `accepts`; empty when it declares none.
    pub accepts: Vec<String>,
}

impl Workspace {
    /// Whether `name` is one the prelude gives a note: it exports it, and
    /// the note at `path` defines nothing by that name.
    pub fn prelude_name(&self, path: &Path, name: &str) -> bool {
        matches!(
            self.resolve(path, name),
            Err(values::EvalError::UnknownName { .. })
        ) && self.modules.prelude().is_some_and(|m| m.is_public(name))
    }
    /// The prelude's names the note at `path` does not define itself: the
    /// ones its calls reach in the prelude.
    pub fn prelude_names(&self, path: &Path) -> Vec<String> {
        self.modules
            .prelude()
            .map(|m| m.public_names())
            .unwrap_or_default()
            .into_iter()
            .filter(|name| {
                matches!(
                    self.resolve(path, name),
                    Err(values::EvalError::UnknownName { .. })
                )
            })
            .collect()
    }
    /// The functions the prelude exports, in the order it lists them.
    pub fn prelude_functions(&self) -> Vec<PreludeFunction> {
        let Some(module) = self.modules.prelude() else {
            return Vec::new();
        };
        let document = module.environment().document(&module.path);
        module
            .public_names()
            .into_iter()
            .filter_map(|name| {
                let definition = document.definitions.iter().find(|d| d.named.name == name)?;
                let Some(Expr::Lambda(params, defaults, _)) =
                    module.expressions().get(&definition.source).map(Expr::bare)
                else {
                    return None;
                };
                let first = params.len() - defaults.len();
                let params = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("{p}{}", if i < first { "" } else { "?" }))
                    .collect();
                let lines: Vec<&str> = document.text.lines().collect();
                let above = &lines[..definition.named.span.line];
                let start = above.iter().rposition(|line| !line.starts_with("//"));
                let comment: Vec<&str> = above[start.map_or(0, |i| i + 1)..]
                    .iter()
                    .map(|line| line[2..].trim())
                    .collect();
                Some(PreludeFunction {
                    accepts: module.accepts.get(&name).cloned().unwrap_or_default(),
                    name,
                    params,
                    documentation: comment.join(" "),
                })
            })
            .collect()
    }
}

impl Engine<'_> {
    /// What the prelude exports as `name`, unless the code at `path` is the
    /// prelude's own; `None` when it exports no such name.
    pub(crate) fn prelude(&mut self, path: &Path, name: &str) -> Option<EvalResult<values::Value>> {
        let module = self.workspace().modules.prelude()?;
        if module.path == path || !module.is_public(name) {
            return None;
        }
        Some(self.in_module(module, |engine| engine.named(&module.path, name)))
    }
}
