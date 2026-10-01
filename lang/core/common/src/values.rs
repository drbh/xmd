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
    /// The currencies a note can write with a symbol, `$3` for USD.
    const SYMBOLS: [(char, Currency); 4] = [
        ('$', Self::USD),
        ('€', Currency(*b"EUR")),
        ('£', Currency(*b"GBP")),
        ('¥', Currency(*b"JPY")),
    ];
    pub fn from_symbol(symbol: char) -> Option<Self> {
        Self::SYMBOLS
            .iter()
            .find_map(|&(s, currency)| (s == symbol).then_some(currency))
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("???")
    }
    pub fn symbol(&self) -> Option<char> {
        Self::SYMBOLS
            .iter()
            .find_map(|&(s, currency)| (currency == *self).then_some(s))
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
    Ratio,
    Duration,
    Date,
    DateTime,
    Boolean,
    Text,
    Resource,
    Checklist,
    Table,
    /// No value has this kind: it types a table's yes/no decision column.
    Choice,
    /// A record a module gave a kind of its own with `tagged`. The kind is
    /// the text the module chose, which the value names itself
    /// (`HostObject::type_name`); this only says it is one of those.
    Tagged,
}
impl ValueType {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// Whether `name` is a kind a module may give a record with `tagged`:
    /// one that reads as a kind ([`Self::is_name`]) and that no kind of the
    /// language's own has, so `type` never answers ambiguously.
    pub fn taggable(name: &str) -> bool {
        Self::is_name(name)
            && !<Self as strum::VariantArray>::VARIANTS
                .iter()
                .any(|kind| kind.as_str() == name)
    }
    /// Whether `name` reads as a kind's name, the language's own or one a
    /// module chose: an ASCII capital, then ASCII letters, digits or
    /// underscores, at most 64 bytes.
    pub fn is_name(name: &str) -> bool {
        let mut chars = name.chars();
        chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            && name.len() <= 64
    }
    /// The properties every value of this kind has, as `Value::property` reads
    /// them and completion offers them. This is the language's own half:
    /// `Record` and the host kinds are missing on purpose, because their
    /// fields depend on the value rather than its type (a tagged record has
    /// whatever its module built it with), so they answer
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
