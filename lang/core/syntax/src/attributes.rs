//! What an attribute's value can hold. The language owns no attribute: a
//! feature module declares each one it owns (`attributes` in its manifest)
//! with the value it holds, by one of the names [`AttributeValue::declared`]
//! reads, and the parser's highlighting, the diagnostics, completion and the
//! evaluation of the value all go by that.

use chrono::NaiveDate;

/// What an attribute's value holds, which decides how it is highlighted, how
/// it is checked and whether it is an expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeValue {
    /// A date or timestamp: relative text such as `tomorrow` or `next friday`,
    /// or an expression that evaluates to one (`2026-09-18`, `launch - 2d`).
    When,
    /// A calendar date written `YYYY-MM-DD` and nothing else: read verbatim,
    /// never evaluated, as an editor action stamps it.
    Date,
    /// An expression that evaluates to a nonnegative duration.
    Duration,
    /// Comma-separated conditions, each a Boolean or a checklist; those not
    /// yet met are what the line waits on.
    Dependencies,
    /// The bare name of a definition whose own call made a tagged record
    /// of one of the kinds the declaration lists (`{tagged: [kinds]}` in a
    /// manifest), written as an expression.
    Tagged,
    /// An expression the engine evaluates, to a value of any kind.
    Expression,
    /// Text, never evaluated.
    Text,
}
impl AttributeValue {
    /// Whether the value is an expression the engine evaluates: it is lexed
    /// and highlighted as one, and rename and extract refactors look inside it.
    /// A `When` may instead be relative text, which callers check first.
    pub const fn is_expression(self) -> bool {
        matches!(
            self,
            Self::When | Self::Duration | Self::Dependencies | Self::Tagged | Self::Expression
        )
    }
    /// The names a manifest gives the values written as text, in the order
    /// the reference lists them. `Tagged` is written as a record instead,
    /// `{tagged: [kinds]}`, since it names the kinds it takes.
    pub const NAMES: [&'static str; 6] = [
        "when",
        "date",
        "duration",
        "dependencies",
        "expression",
        "text",
    ];
    /// The value a module declares an attribute to hold, by the name its
    /// manifest gives it.
    pub fn declared(name: &str) -> Option<Self> {
        match name {
            "when" => Some(Self::When),
            "date" => Some(Self::Date),
            "duration" => Some(Self::Duration),
            "dependencies" => Some(Self::Dependencies),
            "expression" => Some(Self::Expression),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
    /// Whether the value is a date, which highlighting paints as one when it
    /// reads as a date without evaluating anything.
    pub const fn is_date(self) -> bool {
        matches!(self, Self::When | Self::Date)
    }
}

/// The date a `Date` attribute holds, when its text is one.
pub fn stamp(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_is_declared() {
        for name in AttributeValue::NAMES {
            assert!(AttributeValue::declared(name).is_some(), "{name}");
        }
        assert_eq!(AttributeValue::declared("money"), None);
    }
}
