//! Recognizers a module declares: a pattern run over one kind of generic
//! block, whose matches become records and whose named groups are painted.
//! They are data, not code: the module registry compiles each one when its
//! module compiles, and the workspace hands the active ones to
//! [`Document::recognize`] whenever a note is parsed, so recognizing a line
//! never evaluates anything. The native feature recognizers are in
//! `recognizers`; these run after them, over what the generic layer found.
use crate::document::Document;
use common::{Pattern, Span};
use std::sync::Arc;

/// The most matches one note keeps across every declared recognizer: past
/// it, recognizing stops.
pub const MAX_MATCHES: usize = 4096;

/// Which generic block a recognizer reads, and from where: a heading's title
/// (after its `#`s), a list item's text (after its marker and any checkbox),
/// a table row (from its first `|`) or a line of prose (after its
/// indentation). A pattern's `^` anchors there.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr, strum::VariantNames,
)]
#[strum(serialize_all = "snake_case")]
pub enum On {
    Prose,
    Item,
    Heading,
    Row,
}

/// How a captured group is painted: the highlight kinds a module may name,
/// each drawn with the editor's existing token of that kind.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    strum::EnumString,
    strum::IntoStaticStr,
    strum::VariantNames,
    strum::VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum Paint {
    Keyword,
    Number,
    String,
    Variable,
    Heading,
    Function,
    Property,
    Decorator,
    Operator,
    Comment,
    Punctuation,
    Money,
    Date,
    Time,
    Duration,
    Boolean,
    Link,
    Code,
    Place,
}

/// One recognizer, as its module declared it.
#[derive(Clone, Debug)]
pub struct Rule {
    /// The id of the module that declared it.
    pub module: String,
    pub name: String,
    pub on: On,
    pub pattern: Arc<Pattern>,
    /// The paint of each named group that has one.
    pub tokens: Vec<(String, Paint)>,
}
impl PartialEq for Rule {
    fn eq(&self, other: &Self) -> bool {
        self.module == other.module
            && self.name == other.name
            && self.on == other.on
            && self.pattern.source() == other.pattern.source()
            && self.tokens == other.tokens
    }
}

/// One match of a rule in a note.
#[derive(Clone, Debug)]
pub struct Match {
    pub rule: Arc<Rule>,
    /// The whole match, on one line.
    pub span: Span,
    /// Each named group that took part, in the order the pattern names them.
    pub groups: Vec<Group>,
}
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub span: Span,
    pub paint: Option<Paint>,
}

impl Document {
    /// Run `rules` over the note's blocks, replacing what an earlier set
    /// found. Without rules this only clears.
    pub fn recognize(&mut self, rules: &[Arc<Rule>]) {
        self.recognized.clear();
        if rules.is_empty() {
            return;
        }
        let mut found = Vec::new();
        'rows: for (row, block) in self.blocks.iter().enumerate() {
            let Some((on, from)) = *block else { continue };
            let line = self.line(row);
            for rule in rules.iter().filter(|r| r.on == on) {
                for hit in rule.pattern.all(&line[from..]) {
                    if found.len() == MAX_MATCHES {
                        break 'rows;
                    }
                    let at = |range: &std::ops::Range<usize>| {
                        Span::new(row, from + range.start, from + range.end)
                    };
                    found.push(Match {
                        rule: rule.clone(),
                        span: at(&hit.range),
                        groups: hit
                            .groups
                            .iter()
                            .map(|(name, range)| Group {
                                name: (*name).to_owned(),
                                span: at(range),
                                paint: rule
                                    .tokens
                                    .iter()
                                    .find(|(group, _)| group == name)
                                    .map(|(_, paint)| *paint),
                            })
                            .collect(),
                    });
                }
            }
        }
        self.recognized = found;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(on: On, pattern: &str) -> Arc<Rule> {
        Arc::new(Rule {
            module: "m".into(),
            name: "r".into(),
            on,
            pattern: Arc::new(Pattern::new(pattern).unwrap()),
            tokens: vec![("x".into(), Paint::Number)],
        })
    }

    /// Each rule reads only its kind of block, from where that block's text
    /// starts, and never a fence, a comment or a continued expression.
    #[test]
    fn rules_read_their_own_blocks() {
        let text = "## ^ title\n- [ ] ^ item\n^ prose\n| ^ | 1 |\n\nt := table\n| a |\n| 2 |\n\n```\n^ fenced\n```\n<!-- ^ -->\nx := (1 +\n  2)\n";
        let mut doc = Document::parse(text.into());
        let found = |doc: &mut Document, on, pattern| {
            doc.recognize(&[rule(on, pattern)]);
            doc.recognized
                .iter()
                .map(|m| (m.span.line, m.span.start, m.groups.len()))
                .collect::<Vec<_>>()
        };
        assert_eq!(found(&mut doc, On::Heading, r"^\^"), [(0, 3, 0)]);
        assert_eq!(found(&mut doc, On::Item, r"^\^"), [(1, 6, 0)]);
        assert_eq!(found(&mut doc, On::Prose, r"^(\^|2)"), [(2, 0, 0)]);
        assert_eq!(
            found(&mut doc, On::Row, r"(?<x>\d)"),
            [(3, 6, 1), (7, 2, 1)]
        );
        assert_eq!(doc.recognized[0].groups[0].paint, Some(Paint::Number));
        doc.recognize(&[]);
        assert!(doc.recognized.is_empty());
    }
}
