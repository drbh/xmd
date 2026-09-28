//! Who a built-in is for. A note only ever sees the `Note` tier; the
//! `Toolkit` is the list and text plumbing a note may reach for once it needs
//! it; `Module` names the primitives a .wtf module is written with, which
//! outside a module are not names at all.
//!
//! This is the one source of truth for the tier: `describe` in
//! `features::signature` restates every other fact about a built-in, but
//! reads its tier from [`tier`] rather than keeping its own copy.
use super::Builtin;

#[derive(Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum Tier {
    Note,
    Toolkit,
    Module,
}
impl Tier {
    /// Declaration order is the order the reference and the docs read them in.
    pub const ALL: &'static [Tier] = <Self as strum::VariantArray>::VARIANTS;
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// The tier this built-in belongs to; the engine, completion and the
/// reference all read it from here.
pub const fn tier(builtin: Builtin) -> Tier {
    use Builtin::*;
    match builtin {
        Import => Tier::Note,
        SolveLinear => Tier::Module,
        Object => Tier::Toolkit,
        ParseDate => Tier::Module,
        ParseDatetime => Tier::Module,
        Entries => Tier::Toolkit,
        Number => Tier::Toolkit,
        Source => Tier::Toolkit,
        MakeDate => Tier::Module,
        DurationParts => Tier::Module,
        DateParts => Tier::Module,
        AtTime => Tier::Module,
        ParseTime => Tier::Module,
        ParseDuration => Tier::Module,
        PadStart => Tier::Toolkit,
        PadEnd => Tier::Toolkit,
        Slice => Tier::Toolkit,
        Concat => Tier::Toolkit,
        Trim => Tier::Toolkit,
        Type => Tier::Toolkit,
        Floor => Tier::Toolkit,
        Round => Tier::Toolkit,
        Repeat => Tier::Toolkit,
        FormatDate => Tier::Module,
        Error => Tier::Module,
        If => Tier::Note,
        Coalesce => Tier::Note,
        Map => Tier::Toolkit,
        Filter => Tier::Toolkit,
        SortBy => Tier::Toolkit,
        GroupBy => Tier::Toolkit,
        Eval => Tier::Module,
        Fold => Tier::Toolkit,
        Get => Tier::Toolkit,
        Length => Tier::Toolkit,
        Text => Tier::Toolkit,
        Debug => Tier::Note,
        Sparkline => Tier::Note,
        Contains => Tier::Toolkit,
        StartsWith => Tier::Toolkit,
        EndsWith => Tier::Toolkit,
        Split => Tier::Toolkit,
        Join => Tier::Toolkit,
        Lower => Tier::Toolkit,
        Upper => Tier::Toolkit,
        Replace => Tier::Toolkit,
        Sum => Tier::Note,
        Countdown => Tier::Note,
        Stopwatch => Tier::Note,
        Maximize => Tier::Note,
        Solve => Tier::Note,
        Minimize => Tier::Note,
        Today => Tier::Note,
        Now => Tier::Note,
        Rate => Tier::Note,
        To => Tier::Note,
        Forecast => Tier::Note,
        ForecastRange => Tier::Note,
        Quote => Tier::Note,
        Date => Tier::Note,
        Effort => Tier::Note,
        Total => Tier::Note,
        Completed => Tier::Note,
        Remaining => Tier::Note,
    }
}
