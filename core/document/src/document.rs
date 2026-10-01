//! What a parsed note is: its text with every line start known, the inline
//! forms the generic layer read (`blocks`), the features the recognizers read
//! (`recognizers`), and the questions asked of the result. `Document::parse`
//! lives with the recognizer registry that drives it.
use crate::edits::utf16;
use crate::inline::{Calculation, Definition, Link, Reference};
use crate::sections::Section;
use crate::tasks::Task;
use crate::tree::{Highlight, Problem, Tree};
use common::{LineIndex, Lines, Span};
use lsp_types::Position;
use std::collections::BTreeSet;

/// What a definition lays out, which decides what evaluates it: a feature
/// (a form a module declares, a table) or the evaluator's own reading of an
/// expression or a literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionKind {
    /// A call to a form a module declares, `maximize(...)` over the table
    /// under it or `solve(...)`: the module says what it is worth.
    Form,
    Table,
    Expression,
    Literal,
}
/// A parsed note. Its layout is this crate's own: other layers read each
/// piece through the accessors `reads!` lists below, and change it only
/// through named methods such as `recognize`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct Document {
    pub(crate) text: String,
    pub(crate) definitions: Vec<Definition>,
    pub(crate) references: Vec<Reference>,
    pub(crate) imports: BTreeSet<String>,
    pub(crate) members: Vec<crate::imports::Member>,
    pub(crate) tasks: Vec<Task>,
    pub(crate) sections: Vec<Section>,
    /// Every line that writes `@key(value)` attributes, in note order.
    pub(crate) attributed: Vec<crate::attributes::Attributed>,
    /// The attributes modules declare that the note was parsed with.
    pub(crate) declarations: Vec<std::sync::Arc<crate::attributes::Declaration>>,
    /// Whether it writes any attribute, so what the modules declare can
    /// change how it reads.
    pub(crate) foreign: bool,
    pub(crate) tables: Vec<crate::tables::Table>,
    /// The definitions that call a form a module declares.
    pub(crate) forms: Vec<crate::forms::Formed>,
    /// The forms modules declare that the note was parsed with.
    pub(crate) forms_declared: Vec<std::sync::Arc<crate::forms::Form>>,
    pub(crate) links: Vec<Link>,
    pub(crate) calculations: Vec<Calculation>,
    pub(crate) highlights: Vec<Highlight>,
    pub(crate) problems: Vec<Problem>,
    /// What the recognizers modules declare found, in note order: filled by
    /// [`Document::recognize`], empty until then.
    pub(crate) recognized: Vec<crate::declared::Match>,
    /// The rules it was recognized with.
    pub(crate) rules: Vec<std::sync::Arc<crate::declared::Rule>>,
    /// How many of `links` the note's own text wrote: the rest are what
    /// declared recognizers link.
    pub(crate) native_links: usize,
    /// Each row's generic block, when a declared recognizer may read it, the
    /// byte its text starts at, and the byte its title ends at: its first
    /// attribute, or a heading's or checklist item's trailing `:name`.
    pub(crate) blocks: Vec<Option<(crate::declared::On, usize, usize)>>,
    /// Where each line of `text` starts, so spans find their line at once.
    lines: LineIndex,
    /// The expression regions that call `sum`, in note order: the only ones
    /// a sum row scope can be found in, kept so a lookup per reference need
    /// not gather every region of the note.
    pub(crate) sums: Vec<Span>,
}
/// A document is its text with every line start known: pass it where a span
/// reads text (`span.range(doc)`) instead of `doc.text()`, which rescans.
impl Lines for Document {
    fn text(&self) -> &str {
        &self.text
    }
    fn line_start(&self, line: usize) -> usize {
        self.lines.start(line)
    }
}

/// Read-only access to what the parse found: one inlined accessor per piece
/// another layer reads.
macro_rules! reads {
    ($($field:ident: $ty:ty),* $(,)?) => {
        impl Document {
            $(#[inline]
            pub fn $field(&self) -> &$ty {
                &self.$field
            })*
        }
    };
}
reads! {
    text: str,
    definitions: [Definition],
    references: [Reference],
    imports: BTreeSet<String>,
    members: [crate::imports::Member],
    tasks: [Task],
    sections: [Section],
    tables: [crate::tables::Table],
    forms: [crate::forms::Formed],
    links: [Link],
    calculations: [Calculation],
    highlights: [Highlight],
    problems: [Problem],
    recognized: [crate::declared::Match],
    rules: [std::sync::Arc<crate::declared::Rule>],
}

impl Document {
    /// Take what the generic layer and the recognizers read into the tree,
    /// once the parse is done.
    pub(crate) fn adopt(&mut self, tree: Tree) {
        self.native_links = tree.links.len();
        Tree {
            text: self.text,
            lines: self.lines,
            definitions: self.definitions,
            references: self.references,
            imports: self.imports,
            members: self.members,
            links: self.links,
            calculations: self.calculations,
            highlights: self.highlights,
            problems: self.problems,
            heading: _,
            forms: _,
        } = tree;
        self.sums = expression_regions(self)
            .into_iter()
            .filter(|region| region.source(self).contains("sum"))
            .collect();
    }
    pub fn line(&self, row: usize) -> &str {
        self.lines.line(&self.text, row)
    }
    /// How many lines it has, as `str::lines` counts them.
    pub fn line_count(&self) -> usize {
        self.lines.count()
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
    /// The form this definition calls, when it calls one.
    pub fn form_of(&self, definition: usize) -> Option<&crate::forms::Formed> {
        self.forms.iter().find(|f| f.definition == definition)
    }
    /// The forms modules declare that the note was parsed with.
    pub fn forms_declared(&self) -> &[std::sync::Arc<crate::forms::Form>] {
        &self.forms_declared
    }
    /// The form named `name` the note was parsed with.
    pub fn declared_form(&self, name: &str) -> Option<&crate::forms::Form> {
        self.forms_declared
            .iter()
            .find(|f| f.name == name)
            .map(std::sync::Arc::as_ref)
    }
    /// The form or table `definition` lays out rows for, as a grid: its
    /// header line and one past its last row.
    pub fn grid_of(&self, definition: usize) -> Option<(usize, usize)> {
        self.table_of(definition)
            .map(|t| (t.header, t.end_line))
            .or_else(|| {
                self.form_of(definition)
                    .filter(|f| f.has_table())
                    .map(|f| (f.header, f.end_line))
            })
    }
    /// What `definition` is. A form is recognized before a table.
    pub fn definition_kind(&self, definition: usize) -> DefinitionKind {
        let def = &self.definitions[definition];
        if self.form_of(definition).is_some() {
            DefinitionKind::Form
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
    /// The first and last rows a definition spans: through the table it lays
    /// out, when it lays one out, otherwise through its own value.
    pub fn definition_rows(&self, definition: usize) -> (usize, usize) {
        let def = &self.definitions[definition];
        let first = def.named.span.line;
        let last = self
            .grid_of(definition)
            .map_or(def.end.line, |(_, end)| end.saturating_sub(1));
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
/// in-place calculation, the expression cells of a form's table, and the
/// attributes of tasks
/// and of lines with a declared attribute whose value is an expression
/// (`AttributeValue::is_expression`). Shared by
/// rename/refactor scans and by a table's `sum` scope lookup, so both agree on
/// what counts as an expression.
pub fn expression_regions(doc: &Document) -> Vec<Span> {
    doc.definitions
        .iter()
        .filter(|d| d.expression)
        .map(|d| d.value_span)
        .chain(doc.calculations.iter().map(|c| c.span))
        .chain(doc.forms.iter().flat_map(|f| {
            // Its arguments are inside the definition's own value already.
            f.regions().skip(f.arguments.len())
        }))
        .chain(
            doc.claimed_attributes()
                .filter(|(k, _)| doc.attribute_value(k).is_some_and(|v| v.is_expression()))
                .map(|(_, a)| a.value_span),
        )
        .collect()
}
