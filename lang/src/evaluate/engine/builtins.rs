//! The one list of built-in function names. Evaluation, the parser's notion of
//! a reserved name and the editor's signature table all name the same variants,
//! so a new built-in cannot be half-added.

/// Declare the built-ins once: the enum, the spelling each variant answers to,
/// and the order `Builtin::ALL` (and therefore completion) walks them in.
macro_rules! builtins {
    ($($variant:ident => $name:literal,)*) => {
        /// A built-in function, named rather than spelled out at every call site.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Builtin {
            $($variant,)*
        }
        impl Builtin {
            /// Every built-in, in the order the editor presents them.
            pub const ALL: &'static [Builtin] = &[$(Builtin::$variant,)*];
            /// The spelling a note writes.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Builtin::$variant => $name,)*
                }
            }
        }
        impl std::str::FromStr for Builtin {
            type Err = ();
            fn from_str(name: &str) -> Result<Self, Self::Err> {
                match name {
                    $($name => Ok(Builtin::$variant),)*
                    _ => Err(()),
                }
            }
        }
    };
}

builtins! {
    Import => "import",
    SolveLinear => "solve_linear",
    Object => "object",
    ParseDate => "parse_date",
    ParseDatetime => "parse_datetime",
    Entries => "entries",
    Number => "number",
    Source => "source",
    MakeDate => "make_date",
    DurationParts => "duration_parts",
    DateParts => "date_parts",
    AtTime => "at_time",
    ParseTime => "parse_time",
    ParseDuration => "parse_duration",
    PadStart => "pad_start",
    PadEnd => "pad_end",
    Slice => "slice",
    Concat => "concat",
    Trim => "trim",
    Type => "type",
    Floor => "floor",
    Round => "round",
    Repeat => "repeat",
    FormatDate => "format_date",
    Error => "error",
    If => "if",
    Coalesce => "coalesce",
    Map => "map",
    Filter => "filter",
    SortBy => "sort_by",
    GroupBy => "group_by",
    Eval => "eval",
    Fold => "fold",
    Get => "get",
    Length => "length",
    Text => "text",
    Debug => "debug",
    Sparkline => "sparkline",
    Contains => "contains",
    StartsWith => "starts_with",
    EndsWith => "ends_with",
    Split => "split",
    Join => "join",
    Lower => "lower",
    Upper => "upper",
    Replace => "replace",
    Sum => "sum",
    Countdown => "countdown",
    Stopwatch => "stopwatch",
    Maximize => "maximize",
    Solve => "solve",
    Minimize => "minimize",
    Today => "today",
    Now => "now",
    Rate => "rate",
    To => "to",
    Forecast => "forecast",
    ForecastRange => "forecast_range",
    Quote => "quote",
    Date => "date",
    Effort => "effort",
    Total => "total",
    Completed => "completed",
    Remaining => "remaining",
}

impl Builtin {
    /// A special form decides for itself whether and how to evaluate its
    /// arguments: the branches of `if`, the module an `import` names, the row
    /// expression a `sum` walks, the lookups that record what they wanted.
    /// Every other built-in takes evaluated values and is answered by the
    /// functional table.
    pub fn is_special_form(self) -> bool {
        matches!(
            self,
            Builtin::Import
                | Builtin::If
                | Builtin::Coalesce
                | Builtin::Sum
                | Builtin::Eval
                | Builtin::Now
                | Builtin::Today
                | Builtin::Stopwatch
                | Builtin::Countdown
                | Builtin::Rate
                | Builtin::To
                | Builtin::Forecast
                | Builtin::ForecastRange
                | Builtin::Quote
                | Builtin::Date
                | Builtin::Total
                | Builtin::Completed
                | Builtin::Remaining
                | Builtin::Effort
                | Builtin::Maximize
                | Builtin::Minimize
                | Builtin::Solve
        )
    }
}

impl std::fmt::Display for Builtin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
