//! The one list of built-ins. Each row names a variant, the spelling a note
//! writes, who may call it and whether it is a special form, so evaluation,
//! the parser's reserved names and the editor's signature table all read the
//! same facts and a new built-in cannot be half-added.

/// Who a built-in is for. A note only ever sees the `Note` tier; the `Toolkit`
/// is the list and text plumbing a note may reach for once it needs it;
/// `Module` names the primitives a .x.md module is written with, which outside
/// a module are not names at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr, strum::VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum Tier {
    Note,
    Toolkit,
    Module,
}
impl Tier {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// Declare the built-ins once. A row is `Variant => "spelling", Tier` and ends
/// in `special` when the built-in is a special form: it decides for itself
/// whether and how to evaluate its arguments (the branches of `if`, the module
/// an `import` names, the row expression a `sum` walks, the lookups that record
/// what they wanted). Every other built-in takes evaluated values.
macro_rules! builtins {
    (@special special) => { true };
    (@special) => { false };
    ($($variant:ident => $name:literal, $tier:ident $(, $special:ident)?;)*) => {
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
            /// Who may call it; the engine, completion and the reference all
            /// read it from here.
            pub const fn tier(self) -> Tier {
                match self {
                    $(Builtin::$variant => Tier::$tier,)*
                }
            }
            /// Whether it evaluates its own arguments.
            pub const fn is_special_form(self) -> bool {
                match self {
                    $(Builtin::$variant => builtins!(@special $($special)?),)*
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
    Import => "import", Note, special;
    SolveLinear => "solve_linear", Module;
    Object => "object", Toolkit;
    ParseDate => "parse_date", Module;
    ParseDatetime => "parse_datetime", Module;
    Entries => "entries", Toolkit;
    Number => "number", Toolkit;
    Source => "source", Toolkit;
    MakeDate => "make_date", Module;
    DurationParts => "duration_parts", Module;
    Merge3 => "merge3", Module;
    UrlEncode => "url_encode", Module;
    DateParts => "date_parts", Module;
    AtTime => "at_time", Module;
    ParseTime => "parse_time", Module;
    ParseDuration => "parse_duration", Module;
    PadStart => "pad_start", Toolkit;
    PadEnd => "pad_end", Toolkit;
    Slice => "slice", Toolkit;
    Concat => "concat", Toolkit;
    Trim => "trim", Toolkit;
    Type => "type", Toolkit;
    Floor => "floor", Toolkit;
    Round => "round", Toolkit;
    Repeat => "repeat", Toolkit;
    FormatDate => "format_date", Module;
    Error => "error", Module;
    If => "if", Note, special;
    Let => "let", Note, special;
    Match => "match", Note, special;
    Coalesce => "coalesce", Note, special;
    Map => "map", Toolkit;
    Filter => "filter", Toolkit;
    SortBy => "sort_by", Toolkit;
    GroupBy => "group_by", Toolkit;
    Eval => "eval", Module, special;
    Fold => "fold", Toolkit;
    Get => "get", Toolkit;
    Length => "length", Toolkit;
    Text => "text", Toolkit;
    Debug => "debug", Note;
    Sparkline => "sparkline", Note;
    Contains => "contains", Toolkit;
    StartsWith => "starts_with", Toolkit;
    EndsWith => "ends_with", Toolkit;
    Split => "split", Toolkit;
    Join => "join", Toolkit;
    Lower => "lower", Toolkit;
    Upper => "upper", Toolkit;
    Replace => "replace", Toolkit;
    Sum => "sum", Note, special;
    Countdown => "countdown", Note, special;
    Stopwatch => "stopwatch", Note, special;
    Maximize => "maximize", Note, special;
    Solve => "solve", Note, special;
    Minimize => "minimize", Note, special;
    Today => "today", Note, special;
    Now => "now", Note, special;
    Rate => "rate", Note, special;
    To => "to", Note, special;
    Forecast => "forecast", Note, special;
    ForecastRange => "forecast_range", Note, special;
    Quote => "quote", Note, special;
    Date => "date", Note, special;
    Effort => "effort", Note, special;
    Total => "total", Note, special;
    Completed => "completed", Note, special;
    Remaining => "remaining", Note, special;
}

impl std::fmt::Display for Builtin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
