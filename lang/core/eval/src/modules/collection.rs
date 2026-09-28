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
