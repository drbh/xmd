//! `Collection`: the named sets of workspace records a query or a feature
//! module's `inputs` may bind. The record builders themselves, and the field
//! names they carry, are feature-layer concerns that read this enum back
//! (see `features::catalog::CollectionFields`); this module only names them.

/// A named set of workspace records. Queries bind these names, and feature
/// modules declare the ones they read in `module.inputs`.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::IntoStaticStr,
    strum::Display,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum Collection {
    Ast,
    Days,
    Timers,
    Links,
    Tasks,
    Events,
    Stops,
    Entries,
    Values,
    Plans,
    Decisions,
    Tables,
    Rows,
    Resources,
    Diagnostics,
    Notes,
    Sections,
    Calculations,
    References,
    Cells,
}
impl Collection {
    /// Declaration order is the order the query API lists them in its errors.
    pub const ALL: &'static [Collection] = <Self as strum::VariantArray>::VARIANTS;
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// Who reaches this collection. A note writes a query over the first
    /// kind; the rest describe a note's own structure, which is what a .wtf
    /// module reads and no note needs to know about.
    pub fn tier(self) -> &'static str {
        match self {
            Self::Tasks
            | Self::Events
            | Self::Entries
            | Self::Values
            | Self::Tables
            | Self::Rows
            | Self::Plans
            | Self::Resources
            | Self::Diagnostics
            | Self::Notes
            | Self::Sections
            | Self::Links => "query",
            Self::Ast
            | Self::Cells
            | Self::Calculations
            | Self::References
            | Self::Decisions
            | Self::Timers
            | Self::Days
            | Self::Stops => "module",
        }
    }
    /// One line about what a query finds here, for the reference.
    pub fn documentation(self) -> &'static str {
        match self {
            Self::Ast => "Every syntax node in a note: its kind, text and parent.",
            Self::Days => "One record per itinerary day, with its date and cached forecast.",
            Self::Timers => "Every countdown and stopwatch, at each definition and reference.",
            Self::Links => "Every link written in the prose of a note.",
            Self::Tasks => "Every task, with its dates, estimate, tags and what blocks it.",
            Self::Events => "Appointments: the lines carrying an @at time.",
            Self::Stops => "Itinerary stops, placed on the day they belong to.",
            Self::Entries => "Tasks, appointments and stops together, ready for one timeline.",
            Self::Values => "Every named value, with its expression and evaluated result.",
            Self::Plans => "Linear plans, with the solution the solver found.",
            Self::Decisions => "Each decision a plan made, at the table cell it belongs in.",
            Self::Tables => "Every table definition, with its columns and rows.",
            Self::Rows => "One record per table row, with its cells by column name.",
            Self::Resources => "Links, files, places and GitHub items, with cached metadata.",
            Self::Diagnostics => "The problems a note has, as rows: the ci check in the README.",
            Self::Notes => "One record per note in the workspace, carrying its whole text.",
            Self::Sections => "Every heading, with the span of lines it covers.",
            Self::Calculations => "Every calculation written in prose, with its value.",
            Self::References => "Every bracketed reference, with the value it resolved to.",
            Self::Cells => "Every table cell, with its expression and value.",
        }
    }
    /// A query a reader can run against a note of their own.
    pub fn try_snippet(self) -> &'static str {
        match self {
            Self::Ast => "ast | where kind == \"definition\" | select {name, text}",
            Self::Days => "days | select {title, line}",
            Self::Timers => "timers | where definition | select {name, value}",
            Self::Links => "links | select {url}",
            Self::Tasks => "tasks | where !done",
            Self::Events => "events | sort at | select {title, at}",
            Self::Stops => "stops | select {title, at}",
            Self::Entries => "entries | where due != null | sort due",
            Self::Values => "values | select {name, display}",
            Self::Plans => "plans | select {name, solution}",
            Self::Decisions => "decisions | select {plan, title, value}",
            Self::Tables => "tables | select {name, display}",
            Self::Rows => "rows | where cells.price > $4 | select {cells}",
            Self::Resources => "resources | select {target, metadata}",
            Self::Diagnostics => "diagnostics | where severity == \"error\"",
            Self::Notes => "notes | select {title, line}",
            Self::Sections => "sections | where level == 2 | select {title}",
            Self::Calculations => "calculations | select {expression, display}",
            Self::References => "references | select {name, display}",
            Self::Cells => "cells | where computed | select {expression, display}",
        }
    }
    /// Every collection name, in declaration order, for error messages.
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
impl std::str::FromStr for Collection {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| format!("Unknown input collection: {value}"))
    }
}
