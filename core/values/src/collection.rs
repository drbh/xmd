//! `Collection`: the named sets of workspace records a query or a feature
//! module's `inputs` may bind. The record builders themselves, and the field
//! names they carry, are feature-layer concerns that read this enum back
//! (see `records::collect`); this module only names them.
//!
//! Most collections are the language's own, built natively from what a note
//! is. A feature module may declare more under `collections` and build them
//! in its `records` hook; those are [`Collection::Declared`], known only by
//! name here, and whether any module declares one is the registry's to say.
use std::sync::Arc;

/// A named set of workspace records. Queries bind these names, and feature
/// modules declare the ones they read in `module.inputs`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Collection {
    Ast,
    /// Every place a note names something: each definition's name and each
    /// reference to a name, bracketed or inside an expression.
    Mentions,
    Links,
    /// Every list item with a checkbox: the language's checklist items.
    Checkboxes,
    /// The records of every declared collection a module marks as entries
    /// (leaf tasks, events, stops): what a query lays on a timeline.
    Entries,
    Values,
    /// The definitions that call a form a module declares, as `values`
    /// records them: what the module that declares the form builds from.
    Forms,
    Tables,
    Rows,
    Resources,
    Diagnostics,
    Notes,
    Sections,
    Calculations,
    References,
    Cells,
    /// What the recognizers modules declare found.
    Recognized,
    /// The lines that write an attribute a module declares, with each such
    /// attribute evaluated.
    Attributed,
    /// A collection a feature module declares and builds, by name.
    Declared(Arc<str>),
}
impl Collection {
    /// The language's own collections, in declaration order.
    pub const NATIVE: [Collection; 18] = [
        Self::Ast,
        Self::Mentions,
        Self::Links,
        Self::Checkboxes,
        Self::Entries,
        Self::Values,
        Self::Forms,
        Self::Tables,
        Self::Rows,
        Self::Resources,
        Self::Diagnostics,
        Self::Notes,
        Self::Sections,
        Self::Calculations,
        Self::References,
        Self::Cells,
        Self::Recognized,
        Self::Attributed,
    ];
    /// The name a query or `inputs` binds.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Declared(name) => name,
            native => native.into(),
        }
    }
    /// The language's own collection called `name`.
    fn native(name: &str) -> Option<Self> {
        Self::NATIVE.into_iter().find(|c| c.as_str() == name)
    }
    /// Every native collection name, in declaration order, for error
    /// messages; the declared ones are the registry's to add.
    pub fn names() -> String {
        Self::NATIVE
            .iter()
            .map(Self::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
    /// Whether a module builds it.
    pub fn is_declared(&self) -> bool {
        matches!(self, Self::Declared(_))
    }
}
impl std::fmt::Display for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// A native name reads as that collection; any other identifier as a
/// declared one, which the registry checks some module declares.
impl std::str::FromStr for Collection {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        if let Some(native) = Self::native(value) {
            return Ok(native);
        }
        let identifier = value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if identifier {
            Ok(Self::Declared(value.into()))
        } else {
            Err(format!("Unknown input collection: {value}"))
        }
    }
}
