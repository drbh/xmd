//! Recognizers a module declares: a pattern run over one kind of generic
//! block, whose matches become records and whose named groups are painted.
//! They are data, not code: the module registry compiles each one when its
//! module compiles, and the workspace hands the active ones to
//! [`Document::recognize`] whenever a note is parsed, so recognizing a line
//! never evaluates anything. The native feature recognizers are in
//! `recognizers`; these run after them, over what the generic layer found.
//!
//! Most rules find every match in their kind of block. A `line` rule instead
//! classifies whole lines: of one module's line rules, the first that
//! matches claims the line. Line rules give a note structure: a rule `under`
//! another attaches each match to the nearest open match of that rule, and a
//! rule with `until` stays open to such children until a heading, or until a
//! line it cannot hold (a blank, or one none of its children claims).
use crate::document::Document;
use common::{Found, Pattern, Span};
use std::sync::Arc;

/// The most matches one note keeps across every declared recognizer: past
/// it, recognizing stops.
pub(crate) const MAX_MATCHES: usize = 4096;

/// Which generic block a recognizer reads, and from where: a heading's title
/// (after its `#`s), a list item's text (after its marker and any checkbox),
/// a table row (from its first `|`), a line of prose (after its
/// indentation), or, for `line`, a line of any of those kinds whole, from
/// its first column. A pattern's `^` anchors there.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr, strum::VariantNames,
)]
#[strum(serialize_all = "snake_case")]
pub enum On {
    Prose,
    Item,
    Heading,
    Row,
    Line,
}

/// How long a line rule's match stays open to the matches `under` it: until
/// the next heading, or until the first line that is blank or that none of
/// its children claims.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr, strum::VariantNames,
)]
#[strum(serialize_all = "snake_case")]
pub enum Until {
    Heading,
    Break,
}

/// How a captured group is painted: the highlight kinds a module may name,
/// each drawn with the editor's token of that kind.
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
    /// The key of a `Key: value` line.
    Key,
    // The state of a control a line carries, as a tri-state checkbox shows
    // it, and the text of something finished.
    Toggle,
    ToggleOn,
    ToggleMixed,
    Finished,
    // A categorical palette: a module that needs distinguishable hues picks
    // categories, and the theme decides their colors.
    Category1,
    Category2,
    Category3,
    Category4,
    Category5,
    Category6,
    Category7,
    Category8,
    Category9,
    Category10,
}
impl Paint {
    /// The paint a term names, ignoring case: `Heading` names `heading`.
    pub fn named(term: &str) -> Option<Self> {
        term.to_ascii_lowercase().parse().ok()
    }
}

/// How one group is painted: with the paint of the term of the first of
/// `terms` (group names) that has one, else with `paint`; marked as a
/// declaration when `declaration` is set.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Brush {
    pub paint: Option<Paint>,
    pub terms: Vec<String>,
    /// Each term's paint. A term not listed paints as the paint it names.
    pub paints: Vec<(String, Paint)>,
    pub declaration: bool,
}
impl Brush {
    /// The paint `term` has: the one `paints` gives it, else the one it
    /// names.
    pub fn term_paint(&self, term: &str) -> Option<Paint> {
        self.paints
            .iter()
            .find(|(t, _)| t.eq_ignore_ascii_case(term))
            .map(|(_, paint)| *paint)
            .or_else(|| Paint::named(term))
    }
}

/// One entry of a group's terms: the text as declared, and the term it
/// means. A captured text finds its entry ignoring case.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub text: String,
    lower: String,
    pub term: Arc<str>,
}
impl Term {
    pub fn new(text: String, term: &str) -> Self {
        Self {
            lower: text.to_lowercase(),
            text,
            term: term.into(),
        }
    }
}

/// One recognizer, as its module declared it.
#[derive(Clone, Debug)]
pub struct Rule {
    /// The id of the module that declared it.
    pub module: String,
    pub name: String,
    pub on: On,
    pub pattern: Arc<Pattern>,
    /// A line this matches is not the rule's, whatever `pattern` finds: what
    /// a lookahead would say, in a pattern language without one.
    pub unless: Option<Arc<Pattern>>,
    /// The line rule whose open match each match belongs to.
    pub under: Option<String>,
    /// How long a match stays open to the matches under it.
    pub until: Option<Until>,
    /// Each group's table of terms, in declared order.
    pub terms: Vec<(String, Vec<Term>)>,
    /// The brush of each named group that is painted.
    pub tokens: Vec<(String, Brush)>,
    /// Groups that link somewhere: a URL template whose `{}` is the captured
    /// text, form-encoded.
    pub links: Vec<(String, String)>,
    /// Whether it reads a line only up to where its title ends: its first
    /// attribute, or a heading's or checklist item's trailing `:name`.
    pub title: bool,
    /// Whether its matches are records of the `recognized` collection, or
    /// only paint the note.
    pub record: bool,
}
impl Rule {
    /// A rule over `on` with nothing but its pattern.
    pub fn new(module: &str, name: &str, on: On, pattern: Arc<Pattern>) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
            on,
            pattern,
            unless: None,
            under: None,
            until: None,
            terms: Vec::new(),
            tokens: Vec::new(),
            links: Vec::new(),
            title: false,
            record: true,
        }
    }
    /// What it reads of `line`, whose title ends at `title_end`.
    fn reads<'a>(&self, line: &'a str, title_end: usize) -> &'a str {
        if self.title { &line[..title_end] } else { line }
    }
    /// The terms declared for `group`, in order.
    pub fn terms(&self, group: &str) -> &[Term] {
        self.terms
            .iter()
            .find(|(name, _)| name == group)
            .map_or(&[], |(_, terms)| terms)
    }
    /// What `text`, captured by `group`, means.
    fn term(&self, group: &str, text: &str) -> Option<Arc<str>> {
        let terms = self.terms(group);
        if terms.is_empty() {
            return None;
        }
        let lower = text.to_lowercase();
        terms
            .iter()
            .find(|t| t.lower == lower)
            .map(|t| t.term.clone())
    }
}
impl PartialEq for Rule {
    fn eq(&self, other: &Self) -> bool {
        let source = |p: &Option<Arc<Pattern>>| p.as_ref().map(|p| p.source().to_owned());
        self.module == other.module
            && self.name == other.name
            && self.on == other.on
            && self.pattern.source() == other.pattern.source()
            && source(&self.unless) == source(&other.unless)
            && self.under == other.under
            && self.until == other.until
            && self.terms == other.terms
            && self.tokens == other.tokens
            && self.links == other.links
            && self.title == other.title
            && self.record == other.record
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
    /// The index in `Document::recognized` of the match it is under.
    pub parent: Option<usize>,
    /// One past the last line it holds: the next line, unless it stays open
    /// to children, when it is the line that closed it.
    pub end: usize,
}
impl Match {
    /// The group named `name`, when it took part.
    pub fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }
}
#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub span: Span,
    /// What the rule's terms say the captured text means.
    pub term: Option<Arc<str>>,
    pub paint: Option<Paint>,
    pub declaration: bool,
}

/// One module's line rules, in order, and the stack of its matches still
/// open to children.
struct Classifier<'r> {
    rules: Vec<&'r Arc<Rule>>,
    open: Vec<usize>,
}
impl<'r> Classifier<'r> {
    /// Close, at `row`, the lowest open match `closes` and all above it.
    fn close(&mut self, found: &mut [Match], row: usize, closes: Until) {
        let from = self
            .open
            .iter()
            .position(|i| found[*i].rule.until == Some(closes));
        if let Some(from) = from {
            for i in self.open.drain(from..) {
                found[i].end = row;
            }
        }
    }
    /// The rule that claims `line`, with the position in `open` of the match
    /// it would go under.
    fn claim(
        &self,
        found: &[Match],
        line: &str,
        title_end: usize,
    ) -> Option<(&'r Arc<Rule>, Option<usize>, Found<'r>)> {
        self.rules.iter().find_map(|&rule| {
            let parent = match &rule.under {
                Some(under) => Some(
                    self.open
                        .iter()
                        .rposition(|i| found[*i].rule.name == *under)?,
                ),
                None => None,
            };
            let line = rule.reads(line, title_end);
            // Most lines are no rule's: say so before finding the groups.
            if !rule.pattern.is_match(line)
                || rule.unless.as_ref().is_some_and(|p| p.is_match(line))
            {
                return None;
            }
            let hit = rule.pattern.first(line).filter(|h| !h.range.is_empty())?;
            Some((rule, parent, hit))
        })
    }
}

impl Document {
    /// Run `rules` over the note's blocks, replacing what an earlier set
    /// found. Without rules this only clears. `Document::recognize` (the
    /// parse driver) calls it.
    pub(crate) fn match_declared(&mut self, rules: &[Arc<Rule>]) {
        self.recognized.clear();
        self.links.truncate(self.native_links);
        self.rules = rules.to_vec();
        if rules.is_empty() {
            return;
        }
        let mut classifiers: Vec<Classifier<'_>> = Vec::new();
        for rule in rules.iter().filter(|r| r.on == On::Line) {
            match classifiers
                .iter_mut()
                .find(|c| c.rules[0].module == rule.module)
            {
                Some(classifier) => classifier.rules.push(rule),
                None => classifiers.push(Classifier {
                    rules: vec![rule],
                    open: Vec::new(),
                }),
            }
        }
        let mut found: Vec<Match> = Vec::new();
        'rows: for row in 0..self.blocks.len() {
            let block = self.blocks[row];
            let line = self.line(row);
            let title_end = block.map_or(line.len(), |(_, _, end)| end);
            if let Some((on, from, _)) = block {
                for rule in rules.iter().filter(|r| r.on == on) {
                    let text = rule.reads(line, title_end.max(from));
                    for hit in rule.pattern.all(&text[from..]) {
                        if found.len() == MAX_MATCHES {
                            break 'rows;
                        }
                        found.push(matched(rule, line, row, from, &hit, None));
                    }
                }
            }
            for classifier in &mut classifiers {
                match block {
                    None => {
                        classifier.close(&mut found, row, Until::Break);
                        continue;
                    }
                    Some((On::Heading, ..)) => classifier.close(&mut found, row, Until::Heading),
                    Some(_) => {}
                }
                let Some((rule, parent, hit)) = classifier.claim(&found, line, title_end) else {
                    classifier.close(&mut found, row, Until::Break);
                    continue;
                };
                if found.len() == MAX_MATCHES {
                    break 'rows;
                }
                // A match closes every open one that is not its ancestor.
                for i in classifier.open.drain(parent.map_or(0, |p| p + 1)..) {
                    found[i].end = row;
                }
                let parent = parent.map(|p| classifier.open[p]);
                let rule = rule.clone();
                found.push(matched(&rule, line, row, 0, &hit, parent));
                if rule.until.is_some() {
                    classifier.open.push(found.len() - 1);
                }
            }
        }
        let rows = self.blocks.len();
        for classifier in &classifiers {
            for i in &classifier.open {
                found[*i].end = rows;
            }
        }
        for m in &found {
            for (name, template) in &m.rule.links {
                if let Some(group) = m.group(name) {
                    let text = group.span.source(&*self);
                    let encoded: String =
                        url::form_urlencoded::byte_serialize(text.as_bytes()).collect();
                    self.links.push(crate::blocks::Link {
                        span: group.span,
                        target: template.replace("{}", &encoded),
                    });
                }
            }
        }
        self.recognized = found;
    }
}

/// A match of `rule` on `row`, whose pattern read `line` from byte `from`:
/// each group with its term and paint.
fn matched(
    rule: &Arc<Rule>,
    line: &str,
    row: usize,
    from: usize,
    hit: &Found<'_>,
    parent: Option<usize>,
) -> Match {
    let at = |range: &std::ops::Range<usize>| Span::new(row, from + range.start, from + range.end);
    let mut groups: Vec<Group> = hit
        .groups
        .iter()
        .map(|(name, range)| Group {
            name: (*name).to_owned(),
            span: at(range),
            term: rule.term(name, &line[from + range.start..from + range.end]),
            paint: None,
            declaration: false,
        })
        .collect();
    for i in 0..groups.len() {
        let Some((_, brush)) = rule.tokens.iter().find(|(g, _)| *g == groups[i].name) else {
            continue;
        };
        let by_term = brush.terms.iter().find_map(|name| {
            let term = groups.iter().find(|g| g.name == *name)?.term.as_deref()?;
            brush.term_paint(term)
        });
        groups[i].paint = by_term.or(brush.paint);
        groups[i].declaration = brush.declaration;
    }
    Match {
        rule: rule.clone(),
        span: at(&hit.range),
        groups,
        parent,
        end: row + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(on: On, pattern: &str) -> Arc<Rule> {
        let mut rule = Rule::new("m", "r", on, Arc::new(Pattern::new(pattern).unwrap()));
        rule.tokens = vec![(
            "x".into(),
            Brush {
                paint: Some(Paint::Number),
                ..Brush::default()
            },
        )];
        Arc::new(rule)
    }

    /// What a note is recognized with when modules declare only `rules`.
    fn rules(rules: Vec<Arc<Rule>>) -> crate::recognizers::Recognizers {
        crate::recognizers::Recognizers {
            rules,
            ..Default::default()
        }
    }

    /// Each rule reads only its kind of block, from where that block's text
    /// starts, and never a fence, a comment or a continued expression.
    #[test]
    fn rules_read_their_own_blocks() {
        let text = "## ^ title\n- [ ] ^ item\n^ prose\n| ^ | 1 |\n\nt := table\n| a |\n| 2 |\n\n```\n^ fenced\n```\n<!-- ^ -->\nx := (1 +\n  2)\n";
        let mut doc = Document::parse(text.into());
        let found = |doc: &mut Document, on, pattern| {
            doc.recognize(&rules(vec![rule(on, pattern)]));
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
        assert_eq!(
            found(&mut doc, On::Line, r"^\S+ \^"),
            [(0, 0, 0), (3, 0, 0)]
        );
        doc.recognize(&rules(vec![]));
        assert!(doc.recognized.is_empty());
    }

    /// Line rules give lines structure: a day holds its stops until a
    /// heading, a stop holds the lines right under it, and the first rule
    /// that matches claims a line. Terms name what a group means and paint.
    #[test]
    fn line_rules_nest_and_close() {
        let pattern = |p: &str| Arc::new(Pattern::new(p).unwrap());
        let mut day = Rule::new("m", "day", On::Line, pattern(r"^# D"));
        day.until = Some(Until::Heading);
        let mut stop = Rule::new("m", "stop", On::Line, pattern(r"^(?<t>[0-9]+) (?<k>\w+)"));
        stop.under = Some("day".into());
        stop.until = Some(Until::Break);
        stop.terms = vec![("k".into(), vec![Term::new("Fly".into(), "Depart")])];
        stop.tokens = vec![(
            "k".into(),
            Brush {
                terms: vec!["k".into()],
                paints: vec![("Depart".into(), Paint::Category1)],
                paint: Some(Paint::Heading),
                declaration: true,
            },
        )];
        let mut note = Rule::new("m", "note", On::Line, pattern(r"\S.*"));
        note.under = Some("stop".into());
        note.unless = Some(pattern(r"^\|"));
        let declared = rules([day, stop, note].into_iter().map(Arc::new).collect());
        let text = "1 fly\n# D\n1 fly\n  a\n| b |\nc\n2 walk\n\nd\n# E\n3 fly\n";
        let mut doc = Document::parse(text.into());
        doc.recognize(&declared);
        let seen: Vec<_> = doc
            .recognized
            .iter()
            .map(|m| (m.rule.name.as_str(), m.span.line, m.parent, m.end))
            .collect();
        assert_eq!(
            seen,
            [
                ("day", 1, None, 9),
                ("stop", 2, Some(0), 4),
                ("note", 3, Some(1), 4),
                ("stop", 6, Some(0), 7),
            ]
        );
        assert_eq!(Paint::named("Category10"), Some(Paint::Category10));
        let fly = &doc.recognized[1].groups[1];
        assert_eq!(fly.term.as_deref(), Some("Depart"));
        assert_eq!((fly.paint, fly.declaration), (Some(Paint::Category1), true));
        let walk = &doc.recognized[3].groups[1];
        assert_eq!(
            (walk.term.clone(), walk.paint),
            (None, Some(Paint::Heading))
        );
    }
}
