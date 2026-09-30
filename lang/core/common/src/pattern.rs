//! Text patterns: the one way the language compiles and runs a regular
//! expression, shared by the `match_pattern` built-in and the recognizers a
//! module declares. Matching runs in time linear in the text (the `regex`
//! crate never backtracks), and a pattern's source and compiled size are
//! bounded, so no pattern a module or note writes can stall a keystroke.
use std::{
    collections::HashMap,
    ops::Range,
    sync::{Arc, Mutex, OnceLock},
};

/// The longest pattern source, in bytes.
pub const MAX_PATTERN: usize = 1024;
/// The most memory a compiled pattern, or its lazy DFA, may take.
const MAX_COMPILED: usize = 1 << 20;
/// The deepest nesting of groups and repetitions.
const MAX_NESTING: u32 = 64;
/// How many compiled patterns [`Pattern::cached`] keeps before starting over.
const CACHED: usize = 128;

/// A compiled pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    regex: regex::Regex,
}

/// One match: its byte range, and each named group that took part, in the
/// order the pattern names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found<'p> {
    pub range: Range<usize>,
    pub groups: Vec<(&'p str, Range<usize>)>,
}

impl Pattern {
    /// Compile `source`, or say why it is not a pattern.
    pub fn new(source: &str) -> Result<Self, String> {
        if source.len() > MAX_PATTERN {
            return Err(format!("Patterns are limited to {MAX_PATTERN} bytes"));
        }
        regex::RegexBuilder::new(source)
            .size_limit(MAX_COMPILED)
            .dfa_size_limit(MAX_COMPILED)
            .nest_limit(MAX_NESTING)
            .build()
            .map(|regex| Self { regex })
            .map_err(|e| match e {
                regex::Error::CompiledTooBig(_) => "Pattern is too large once compiled".into(),
                e => format!("Invalid pattern: {e}"),
            })
    }
    /// The same, compiled once per source: a bounded cache shared by every
    /// caller, so a pattern evaluated on every keystroke is compiled once.
    pub fn cached(source: &str) -> Result<Arc<Self>, String> {
        static CACHE: OnceLock<Mutex<HashMap<String, Arc<Pattern>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(Mutex::default);
        let lock = || cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(pattern) = lock().get(source) {
            return Ok(pattern.clone());
        }
        let pattern = Arc::new(Self::new(source)?);
        let mut cache = lock();
        if cache.len() >= CACHED {
            cache.clear();
        }
        cache.insert(source.to_owned(), pattern.clone());
        Ok(pattern)
    }
    /// The source it was compiled from.
    pub fn source(&self) -> &str {
        self.regex.as_str()
    }
    /// The names of its named groups, in order.
    pub fn group_names(&self) -> impl Iterator<Item = &str> {
        self.regex.capture_names().flatten()
    }
    /// The first match in `text`.
    pub fn first(&self, text: &str) -> Option<Found<'_>> {
        self.regex.captures(text).map(|c| self.found(&c))
    }
    /// Every match in `text` that is not empty, left to right.
    pub fn all<'s>(&'s self, text: &'s str) -> impl Iterator<Item = Found<'s>> + 's {
        self.regex
            .captures_iter(text)
            .filter(|c| !c.get_match().is_empty())
            .map(|c| self.found(&c))
    }
    fn found(&self, captures: &regex::Captures<'_>) -> Found<'_> {
        Found {
            range: captures.get_match().range(),
            groups: self
                .regex
                .capture_names()
                .enumerate()
                .filter_map(|(i, name)| Some((name?, captures.get(i)?.range())))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_named_groups_and_rejects_what_is_too_big() {
        let pattern = Pattern::new(r"(?<name>\w+): (?<number>\d+)(?<note> !)?").unwrap();
        let found = pattern.first("x Ada: 42").unwrap();
        assert_eq!(found.range, 2..9);
        assert_eq!(found.groups, vec![("name", 2..5), ("number", 7..9)]);
        assert_eq!(pattern.all("a: 1 b: 2").count(), 2);
        assert!(
            Pattern::new("(")
                .unwrap_err()
                .starts_with("Invalid pattern")
        );
        assert!(Pattern::new(&"a".repeat(MAX_PATTERN + 1)).is_err());
        assert!(Pattern::new(r"\w{1000}{1000}").is_err());
        assert!(Arc::ptr_eq(
            &Pattern::cached("a+").unwrap(),
            &Pattern::cached("a+").unwrap()
        ));
    }
}
