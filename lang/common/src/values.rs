//! Currencies, codes and value kinds: scalar vocabulary shared by every
//! layer, independent of how a note is parsed or a value is evaluated.
/// An ISO 4217 code such as USD or EUR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency([u8; 3]);
impl Currency {
    pub const USD: Currency = Currency(*b"USD");
    pub fn parse(code: &str) -> Option<Self> {
        let bytes = code.as_bytes();
        (bytes.len() == 3 && bytes.iter().all(u8::is_ascii_uppercase))
            .then(|| Currency([bytes[0], bytes[1], bytes[2]]))
    }
    pub fn from_symbol(symbol: char) -> Option<Self> {
        Some(match symbol {
            '$' => Self::USD,
            '€' => Currency(*b"EUR"),
            '£' => Currency(*b"GBP"),
            '¥' => Currency(*b"JPY"),
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("???")
    }
    pub fn symbol(&self) -> Option<char> {
        match self.as_str() {
            "USD" => Some('$'),
            "EUR" => Some('€'),
            "GBP" => Some('£'),
            "JPY" => Some('¥'),
            _ => None,
        }
    }
}
impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// A 3–5 letter uppercase name is a code literal (USD, EUR, NVDA), never a
/// reference to a note value.
pub fn is_code(name: &str) -> bool {
    (name.len() == 1 || (3..=5).contains(&name.len()))
        && name.bytes().all(|b| b.is_ascii_uppercase())
}
/// A code a note writes bare: a currency (USD), a ticker (NVDA), a temperature
/// unit (F). The shape is decided once, where the note is parsed, instead of
/// being read back out of a string at every lookup.
///
/// A code is still *text* to a note: it displays as its letters, `type` calls
/// it Text, and it compares and concatenates with text, because that is what
/// notes and the stdlib already rely on. What it adds is that the evaluator
/// can tell a code from a string that happens to be uppercase.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code {
    letters: [u8; 5],
    length: u8,
}
impl Code {
    pub fn parse(name: &str) -> Option<Self> {
        is_code(name).then(|| {
            let mut letters = [0; 5];
            letters[..name.len()].copy_from_slice(name.as_bytes());
            Code {
                letters,
                length: name.len() as u8,
            }
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.letters[..self.length as usize]).unwrap_or("???")
    }
    /// The ISO 4217 currency this code names, if it names one.
    pub fn currency(self) -> Option<Currency> {
        Currency::parse(self.as_str())
    }
}
impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Code({})", self.as_str())
    }
}
/// The kind of a value, named exactly as a note or query sees it.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    strum::IntoStaticStr,
    strum::Display,
    strum::VariantArray,
)]
pub enum ValueType {
    Null,
    List,
    Record,
    Function,
    Namespace,
    Number,
    Count,
    Money,
    Forecast,
    Ratio,
    Duration,
    Date,
    DateTime,
    Boolean,
    Text,
    Resource,
    Checklist,
    Countdown,
    Stopwatch,
    Table,
    Plan,
    /// No value has this kind: it types a table's yes/no decision column.
    Choice,
}
impl ValueType {
    /// Every kind, in declaration order: the reference walks this list.
    pub const ALL: &'static [ValueType] = <Self as strum::VariantArray>::VARIANTS;
    /// One line about the kind, for the reference. Exhaustive on purpose: a
    /// new kind does not compile until it says what it is.
    pub fn documentation(self) -> &'static str {
        match self {
            Self::Null => "No value: a missing lookup, an absent field, or the literal null.",
            Self::List => "An ordered list of values, written [a, b, c].",
            Self::Record => "Named fields, written {key: value}; read them with a dot.",
            Self::Function => "A pure function, written fn(a, b) => expression.",
            Self::Namespace => {
                "Another note or module, imported by name; its members resolve lazily."
            }
            Self::Number => "A plain number, with , as an optional thousands separator.",
            Self::Count => "A whole number of things, as length and the checklist calls return it.",
            Self::Money => "An amount in one currency; two currencies never add up silently.",
            Self::Forecast => {
                "A cached day of weather, with .high, .low, .summary and .rain. Seasonal outlooks include estimated temperatures and precipitation chance, labeled as estimates."
            }
            Self::Ratio => "A percentage, written 8.875%; it scales anything it multiplies.",
            Self::Duration => "A span of whole seconds, written 30s, 20m, 2h, 14d or 2w.",
            Self::Date => "A calendar date, written 2026-11-20; dates subtract to a duration.",
            Self::DateTime => "A timestamp with a UTC offset, written 2026-11-20T09:30-06:00.",
            Self::Boolean => "true or false, as comparisons and named tasks produce it.",
            Self::Text => "Text in double quotes; an uppercase code such as USD is text too.",
            Self::Resource => {
                "A link, file, place or GitHub item, with cached metadata and an Open lens."
            }
            Self::Checklist => {
                "A named heading: the tasks beneath it, counted by total and completed."
            }
            Self::Countdown => {
                "A countdown timer started from a duration; controls persist its state."
            }
            Self::Stopwatch => "A stopwatch that counts up; elapsed time survives a closed editor.",
            Self::Table => "A Markdown table with typed columns, read by sum and the row calls.",
            Self::Plan => "A solved linear plan: its decisions and its constraint slack.",
            Self::Choice => "No value has this kind: it types a table's yes/no decision column.",
        }
    }
    /// A complete note that produces a value of this kind.
    pub fn try_snippet(self) -> &'static str {
        match self {
            Self::Null => "missing := null\nname := coalesce(missing, \"friend\")",
            Self::List => "prices := [$3, $4.50]\ntotal := sum(prices)",
            Self::Record => "trip := {city: \"Oaxaca\", nights: 3}\nwhere := trip.city",
            Self::Function => "double := fn(n) => n * 2\nfour := double(2)",
            Self::Namespace => {
                "units := import(\"units\")\nmiles := units.convert(100, \"km\", \"mi\")"
            }
            Self::Number => "12:units\n$8.50:unit_price\nsubtotal := units * unit_price",
            Self::Count => "letters := length(\"Oaxaca\")",
            Self::Money => "price := $12.50\ncents := price.amount",
            Self::Forecast => "landing := forecast(\"Oaxaca\", 2026-11-20)",
            Self::Ratio => "share := 8.875%\ntax := $100 * share",
            Self::Duration => "slot := 90m\nminutes := slot.seconds / 60",
            Self::Date => "2026-11-20:departure\ndays_left := departure - today()",
            Self::DateTime => "landing := 2026-11-20T09:30-06:00\nhour := landing + 90m",
            Self::Boolean => "$500:budget\n$420:spent\nbudget_ok := spent <= budget",
            Self::Text => "\"Oaxaca\":city\ngreeting := \"Hello from \" + city",
            Self::Resource => "https://example.com/docs:docs\n\n- [ ] Read the [docs]",
            Self::Checklist => {
                "## Launch :launch\n\n- [x] Draft the announcement\n- [ ] Ship it\n\ndone := completed(launch)"
            }
            Self::Countdown => "focus := countdown(25m)\nleft := focus.remaining",
            Self::Stopwatch => "work := stopwatch()\nspent := work.elapsed",
            Self::Table => {
                "groceries := table\n| item  | quantity | price |\n| ----- | -------- | ----- |\n| apple | 2        | $3.30 |\n| pear  | 4        | $4.30 |\n\ntotal := sum(groceries, quantity * price)"
            }
            Self::Plan => {
                "bakery := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression                        |\n| ---------- | --------------------------------- |\n| flour      | 12 * bagels + 6.5 * doughnuts <= 400 |\n\nbake := bakery.bagels"
            }
            Self::Choice => {
                "gear := table\n| item  | weight | value | take? |\n| ----- | ------ | ----- | ----- |\n| tent  | 3      | 9     |       |\n| stove | 1      | 4     |       |\n\npack := maximize(sum(gear, value * take))\n| constraint | expression                    |\n| ---------- | ----------------------------- |\n| weight     | sum(gear, weight * take) <= 3 |"
            }
        }
    }
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// The properties every value of this kind has, as `Value::property` reads
    /// them and completion offers them. This is the language's own half:
    /// `Record` and the host kinds are missing on purpose, because their
    /// fields depend on the value rather than its type (a countdown has
    /// `remaining`, a record has whatever it was built with), so they answer
    /// for themselves — see `engine::host`.
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Money => &["amount", "currency", "type"],
            Self::Duration => &["seconds", "type"],
            Self::Date | Self::DateTime | Self::Ratio => &["value", "type"],
            _ => &[],
        }
    }
}
