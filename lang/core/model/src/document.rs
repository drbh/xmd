//! What a parsed note is: its text with every line start known, the inline
//! forms the generic layer read (`blocks`), the features the recognizers read
//! (`recognizers`), and the questions asked of the result. `Document::parse`
//! lives with the recognizer registry that drives it.
use crate::blocks::{Calculation, Definition, Highlight, Link, Problem, Reference, Tree};
use crate::edits::utf16;
use crate::events::Event;
use crate::sections::Section;
use crate::tasks::Task;
use common::{LineIndex, Lines, Span};
use lsp_types::Position;
use std::collections::BTreeSet;

/// What a definition lays out, which decides what evaluates it: a feature
/// (a plan, a goal seek, a table) or the evaluator's own reading of an
/// expression or a literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionKind {
    /// `maximize`/`minimize` over a constraint table.
    Plan,
    /// `solve(constraint)`: the definition's own name is the unknown.
    GoalSeek,
    Table,
    Expression,
    Literal,
}
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Document {
    pub text: String,
    pub definitions: Vec<Definition>,
    pub references: Vec<Reference>,
    pub imports: BTreeSet<String>,
    pub members: Vec<crate::imports::Member>,
    pub tasks: Vec<Task>,
    pub sections: Vec<Section>,
    pub events: Vec<Event>,
    pub tables: Vec<crate::tables::Table>,
    pub plans: Vec<crate::plans::Plan>,
    pub days: Vec<crate::itinerary::Day>,
    pub links: Vec<Link>,
    pub calculations: Vec<Calculation>,
    pub highlights: Vec<Highlight>,
    pub problems: Vec<Problem>,
    /// What the recognizers modules declare found, in note order: filled by
    /// [`Document::recognize`], empty until then.
    pub recognized: Vec<crate::declared::Match>,
    /// Each row's generic block, when a declared recognizer may read it, and
    /// the byte its text starts at.
    pub(crate) blocks: Vec<Option<(crate::declared::On, usize)>>,
    /// Where each line of `text` starts, so spans find their line at once.
    lines: LineIndex,
    /// The expression regions that call `sum`: the only ones a sum row scope
    /// can be found in, kept so a lookup per reference need not gather every
    /// region of the note.
    sums: Vec<Span>,
}
/// A document is its text with every line start known: pass it where a span
/// reads text (`span.range(doc)`) instead of `&doc.text`, which rescans.
impl Lines for Document {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_start(&self, line: usize) -> usize {
        self.lines.start(line)
    }
}

impl Document {
    /// Take what the generic layer and the recognizers read into the tree,
    /// once the parse is done.
    pub(crate) fn adopt(&mut self, tree: Tree) {
        let Tree {
            text,
            lines,
            definitions,
            references,
            imports,
            members,
            links,
            calculations,
            highlights,
            problems,
            heading: _,
        } = tree;
        self.text = text;
        self.lines = lines;
        self.definitions = definitions;
        self.references = references;
        self.imports = imports;
        self.members = members;
        self.links = links;
        self.calculations = calculations;
        self.highlights = highlights;
        self.problems = problems;
        self.sums = expression_regions(self)
            .into_iter()
            .filter(|region| region.source(self).contains("sum"))
            .collect();
    }
    /// The expression regions that call `sum`, in note order.
    pub(crate) fn sum_regions(&self) -> &[Span] {
        &self.sums
    }
    pub fn line(&self, row: usize) -> &str {
        self.lines.line(&self.text, row)
    }
    /// Just past a bracketed reference's closing `]`.
    pub fn reference_close(&self, reference: &Reference) -> usize {
        let end = reference.end();
        end + self.line(reference.span.line)[end..].find(']').unwrap_or(0) + 1
    }
    /// A whole line, without its line break.
    pub fn line_span(&self, row: usize) -> Span {
        Span::new(row, 0, self.line(row).len())
    }
    pub fn line_end(&self, row: usize) -> Position {
        Position::new(row as u32, utf16(self.line(row), self.line(row).len()))
    }
    /// The plan this definition solves, when it is one.
    pub fn plan_of(&self, definition: usize) -> Option<&crate::plans::Plan> {
        self.plans.iter().find(|p| p.definition == definition)
    }
    /// What `definition` is. A plan is recognized before a goal seek, and a
    /// goal seek before a table.
    pub fn definition_kind(&self, definition: usize) -> DefinitionKind {
        let def = &self.definitions[definition];
        if self.plan_of(definition).is_some() {
            DefinitionKind::Plan
        } else if def.expression && crate::plans_impl::seek_body(&def.source).is_some() {
            DefinitionKind::GoalSeek
        } else if self.table_of(definition).is_some() {
            DefinitionKind::Table
        } else if def.expression {
            DefinitionKind::Expression
        } else {
            DefinitionKind::Literal
        }
    }
    /// The table this definition lays out, when it is one.
    pub fn table_of(&self, definition: usize) -> Option<&crate::tables::Table> {
        self.tables.iter().find(|t| t.definition == definition)
    }
    /// The first and last rows a definition spans: through its table or plan
    /// when it lays one out, otherwise through its own value.
    pub fn definition_rows(&self, definition: usize) -> (usize, usize) {
        let def = &self.definitions[definition];
        let first = def.named.span.line;
        let last = self
            .table_of(definition)
            .map(|t| t.end_line)
            .or_else(|| self.plan_of(definition).map(|p| p.end_line))
            .map_or(def.end.line, |end| end.saturating_sub(1));
        (first, last.max(first))
    }
    /// The leaf tasks under a section heading: what its checklist counts.
    pub fn section_tasks(&self, section: usize) -> impl Iterator<Item = usize> + '_ {
        let section = &self.sections[section];
        self.tasks
            .iter()
            .enumerate()
            .filter(move |(i, t)| {
                t.line > section.line
                    && t.line < section.end_line
                    && !self.tasks.iter().any(|child| child.parent == Some(*i))
            })
            .map(|(i, _)| i)
    }
}

/// Every byte range in a note that holds an expression: named definitions, an
/// in-place calculation, a plan's constraints, and the task attributes whose
/// value is an expression (`AttributeValue::is_expression`). Shared by
/// rename/refactor scans and by a table's `sum` scope lookup, so both agree on
/// what counts as an expression.
pub fn expression_regions(doc: &Document) -> Vec<Span> {
    doc.definitions
        .iter()
        .filter(|d| d.expression)
        .map(|d| d.value_span)
        .chain(doc.calculations.iter().map(|c| c.span))
        .chain(
            doc.plans
                .iter()
                .flat_map(|p| p.constraints.iter().map(|c| c.span)),
        )
        .chain(doc.tasks.iter().flat_map(|t| {
            t.attributes
                .iter()
                .filter(|(k, _)| {
                    k.parse::<syntax::AttributeKey>()
                        .is_ok_and(|k| k.value().is_expression())
                })
                .map(|(_, a)| a.value_span)
        }))
        .collect()
}
