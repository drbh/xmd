//! `Collection`: the named sets of workspace records a query or a feature
//! module's `inputs` may bind. The record builders themselves, and the field
//! names they carry, are feature-layer concerns that read this enum back
//! (see `catalog::collect`); this module only names them.

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
    strum::EnumString,
    strum::VariantArray,
)]
#[strum(
    serialize_all = "snake_case",
    parse_err_ty = String,
    parse_err_fn = unknown
)]
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
    /// What the recognizers modules declare found.
    Recognized,
}
impl Collection {
    /// Every collection name, in declaration order, for error messages.
    pub fn names() -> String {
        <Self as strum::VariantArray>::VARIANTS
            .iter()
            .map(|&c| <&str>::from(c))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
fn unknown(value: &str) -> String {
    format!("Unknown input collection: {value}")
}
