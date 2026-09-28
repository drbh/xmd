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
