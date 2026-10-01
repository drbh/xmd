//! Definition forms: a definition whose whole expression calls a form a
//! feature module declares, `bakery := maximize(objective)`, with the table
//! under it when the form takes one. The language owns no form; the bundled
//! `plans` module declares `maximize`, `minimize` and `solve`. Reading one is
//! pure text: the call's arguments, the table's cells as the declaration says
//! to read each column, the names its expressions read and the problems with
//! how it is written. What the expressions are worth, and what the form
//! means, is the evaluator's and the module's.
use crate::blocks::cells;
use crate::document::Document;
use crate::inline::{Definition, Named, identifier, name_len};
use crate::tables::{Cell, Table};
use crate::tables_impl::rows;
use crate::tree::{HighlightKind, Problem, Tree};
use common::Span;
use std::sync::Arc;
use syntax::Literal;

/// How the host reads an expression a form is handed, before the module sees
/// it: a module reads values, never note code.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr, strum::VariantNames,
)]
#[strum(serialize_all = "snake_case")]
pub enum Reading {
    /// A linear form over the form's unknowns: `terms · unknowns + constant`.
    Linear,
    /// `a <= b`, `a >= b` or `a == b`, each side a linear form.
    Constraint,
    /// A table cell that names its row: an identifier, unique in the table.
    Name,
}
impl Reading {
    /// Every reading a declaration may name, in the order the reference lists
    /// them.
    pub const NAMES: &'static [&'static str] = <Self as strum::VariantNames>::VARIANTS;
    pub fn declared(name: &str) -> Option<Self> {
        name.parse().ok()
    }
    pub fn as_str(self) -> &'static str {
        self.into()
    }
    /// Whether the cell or argument is an expression in the note's scope.
    pub fn is_expression(self) -> bool {
        self != Self::Name
    }
}

/// One column of the table a form takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub reads: Reading,
    /// What a missing cell's problem suggests writing.
    pub example: String,
}

/// The names a form solves for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unknowns {
    /// The names its expressions read that its note leaves undefined. Each is
    /// a name of the note, which reads as that field of the definition's value.
    Free,
    /// The definition's own name.
    Own,
}

/// A form a feature module declares in its manifest's `forms`, under the
/// name a definition calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    /// The id of the module that declares it, whose `define` hook says what
    /// a definition of it is worth.
    pub module: String,
    /// Each parameter as signature help shows it, `name: what it holds`.
    pub params: Vec<String>,
    /// How each argument is read, one per parameter.
    pub reads: Vec<Reading>,
    /// The table under the definition, or none.
    pub table: Vec<Column>,
    pub unknowns: Unknowns,
    /// What a free unknown is called, and the words its hover adds after
    /// naming the definition that chooses it.
    pub unknown: String,
    pub unknown_doc: String,
    /// What a definition of it is called in the problems of its table.
    pub noun: String,
    /// What a definition of it evaluates to, as signature help says it.
    pub returns: String,
    pub documentation: String,
    pub example: String,
}
impl Form {
    /// The parameter's name, without what it holds.
    fn param(&self, index: usize) -> &str {
        let param = self.params.get(index).map_or("argument", String::as_str);
        param.split(':').next().unwrap_or(param).trim()
    }
    /// `| constraint | expression |`: the header its table has.
    pub fn header(&self) -> String {
        let names: Vec<_> = self.table.iter().map(|c| c.name.as_str()).collect();
        format!("| {} |", names.join(" | "))
    }
}

/// A definition a form lays out: the call's arguments and the table under it.
#[derive(Clone, Debug)]
pub struct Formed {
    pub definition: usize,
    pub form: Arc<Form>,
    /// Each argument as written, trimmed, and where.
    pub arguments: Vec<(String, Span)>,
    /// The table's header line and one past its last row; both the line
    /// after the definition when the form takes no table.
    pub header: usize,
    pub end_line: usize,
    pub columns: Vec<Named>,
    pub separators: Vec<String>,
    /// Each row with a cell for every column and a name where one is read,
    /// as written, trimmed, and where.
    pub rows: Vec<Vec<(String, Span)>>,
    /// Every distinct name its expressions read, first occurrence first: the
    /// workspace decides which are its unknowns.
    pub names: Vec<Named>,
    pub problems: Vec<Problem>,
}
impl Formed {
    /// The byte regions of its expressions: its arguments, then each
    /// expression cell, row by row.
    pub fn regions(&self) -> impl Iterator<Item = Span> + '_ {
        let columns = &self.form.table;
        self.arguments
            .iter()
            .map(|(_, span)| *span)
            .chain(self.rows.iter().flat_map(move |row| {
                row.iter()
                    .zip(columns)
                    .filter(|(_, column)| column.reads.is_expression())
                    .map(|((_, span), _)| *span)
            }))
    }
    /// The name cell of each row, when the table reads one.
    pub fn row_name(&self, row: usize) -> Option<&(String, Span)> {
        let column = self
            .form
            .table
            .iter()
            .position(|c| c.reads == Reading::Name)?;
        self.rows.get(row)?.get(column)
    }
    /// Whether a table lies under the definition.
    pub fn has_table(&self) -> bool {
        !self.form.table.is_empty()
    }
}

/// `name(arguments)` covering the whole source: the name, and each
/// argument's byte range in it, trimmed. The closing parenthesis has to match
/// the opening one, not an inner call's.
pub fn call(source: &str) -> Option<(&str, Vec<(usize, usize)>)> {
    let end = source.trim_end().len();
    let length = name_len(source);
    let name = &source[..length];
    if !identifier(name) {
        return None;
    }
    let open = length + source[length..].len() - source[length..].trim_start().len();
    if source.as_bytes().get(open) != Some(&b'(') || !source[..end].ends_with(')') {
        return None;
    }
    // The closing parenthesis must match the opening one, not an inner call.
    let mut depth = 0;
    for (i, c) in source[open..end].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && open + i + 1 != end {
                    return None;
                }
            }
            _ => {}
        }
    }
    let inner = (open + 1, end - 1);
    let trimmed = |(start, end): (usize, usize)| {
        let raw = &source[start..end];
        let from = start + raw.len() - raw.trim_start().len();
        (from, from + raw.trim().len())
    };
    // The arguments are split where the lexer finds a comma between them,
    // so a comma in `$1,000` or in a string is no separator. Text the lexer
    // cannot read is one argument, which fails as it is read.
    let Ok(tokens) = syntax::lex_with_comments(&source[inner.0..inner.1]) else {
        return Some((name, vec![trimmed(inner)]));
    };
    let mut depth = 0usize;
    let mut start = inner.0;
    let mut arguments = Vec::new();
    for token in tokens {
        match token.kind {
            syntax::Lexeme::Left | syntax::Lexeme::OpenList | syntax::Lexeme::OpenRecord => {
                depth += 1;
            }
            syntax::Lexeme::Right | syntax::Lexeme::CloseList | syntax::Lexeme::CloseRecord => {
                depth = depth.saturating_sub(1);
            }
            syntax::Lexeme::Comma if depth == 0 => {
                arguments.push(trimmed((start, inner.0 + token.start)));
                start = inner.0 + token.end;
            }
            _ => {}
        }
    }
    let last = trimmed((start, inner.1));
    // `f()` has no arguments rather than one empty one.
    if !(arguments.is_empty() && last.0 == last.1) {
        arguments.push(last);
    }
    Some((name, arguments))
}

/// The form `source` calls, of those a note is parsed with.
pub(crate) fn called<'f>(source: &str, forms: &'f [Arc<Form>]) -> Option<&'f Arc<Form>> {
    let (name, _) = call(source)?;
    forms.iter().find(|form| form.name == name)
}

/// A definition on `row` that calls a declared form, and the table under it
/// when the form takes one. Returns how many rows below `row` the table took;
/// a form without a table claims none.
pub(crate) fn recognize(
    tree: &mut Tree,
    doc: &mut Document,
    lines: &[&str],
    row: usize,
) -> Option<usize> {
    let index = tree.opened(row)?;
    let form = called(&tree.definitions[index].source, &doc.forms_declared)?.clone();
    let mut formed = parse(&tree.text, &tree.definitions[index], index, form, lines);
    let end_line = formed.end_line;
    for column in &formed.columns {
        tree.paint(column.span, HighlightKind::Keyword);
    }
    for cells in &formed.rows {
        for ((_, span), column) in cells.iter().zip(&formed.form.table) {
            if column.reads.is_expression() {
                tree.expression(lines[span.line], span.line, span.start, span.end);
            } else {
                tree.paint(*span, HighlightKind::Variable);
            }
        }
    }
    let regions: Vec<Span> = formed.regions().collect();
    for reference in &tree.references {
        let Some(region) = regions.iter().find(|r| r.contains(&*tree, reference.span)) else {
            continue;
        };
        // Column names inside sum(table, ...) belong to the table.
        let offset = region.offset_of(tree, reference.span).unwrap_or(0);
        if reference.property.is_none()
            && syntax::sum_scope_at(region.source(tree), offset).is_none()
            && !formed.names.iter().any(|n| n.name == reference.name)
        {
            formed.names.push(Named {
                name: reference.name.clone(),
                span: reference.span,
            });
        }
    }
    tree.problems.extend(formed.problems.clone());
    let takes_table = formed.has_table();
    doc.forms.push(formed);
    takes_table.then(|| end_line.saturating_sub(row + 1))
}

/// "a" or "an", for the word that follows.
fn article(word: &str) -> &'static str {
    let vowel = word.starts_with(['a', 'e', 'i', 'o', 'u']);
    if vowel { "an" } else { "a" }
}

/// A count as a word, the way a problem says how many columns there are.
fn count(n: usize) -> String {
    let words = ["one", "two", "three", "four", "five"];
    n.checked_sub(1)
        .and_then(|i| words.get(i))
        .map_or_else(|| n.to_string(), |word| word.to_string())
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn parse(text: &str, def: &Definition, index: usize, form: Arc<Form>, lines: &[&str]) -> Formed {
    let (_, bounds) = call(&def.source).expect("a form is a call");
    let offset = def.expression_span(text).start;
    let source = Span::new(def.value_span.line, offset, offset);
    let header = def.end.line + 1;
    let arguments: Vec<(String, Span)> = bounds
        .iter()
        .map(|&(s, e)| (def.source[s..e].into(), source.relative(text, s, e)))
        .collect();
    let mut formed = Formed {
        definition: index,
        form: form.clone(),
        arguments,
        header,
        end_line: header,
        columns: vec![],
        separators: vec![],
        rows: vec![],
        names: vec![],
        problems: vec![],
    };
    let mut problem = |span, message: String| formed.problems.push(Problem { span, message });
    let (at, takes) = (def.value_span, form.reads.len());
    let (name, noun) = (&form.name, &form.noun);
    for missing in formed.arguments.len()..takes {
        let param = form.param(missing);
        let message = format!("{name}() needs {} {param} expression", article(param));
        problem(at, message);
    }
    if formed.arguments.len() > takes {
        let plural = if takes == 1 { "" } else { "s" };
        problem(at, format!("{name}() takes {takes} argument{plural}"));
    }
    if form.table.is_empty() {
        return formed;
    }
    let width = form.table.len();
    let Some(headers) = lines.get(header).and_then(|l| cells(l, header)) else {
        let message = format!("A {noun} needs a {} table on the next line", form.header());
        problem(at, message);
        return formed;
    };
    if headers.len() != width || headers.iter().any(|(name, _)| !identifier(name)) {
        let (title, layout) = (capitalized(&form.noun), form.header());
        let message = format!("{title} tables have {} columns: {layout}", count(width));
        problem(Span::new(header, 0, lines[header].len()), message);
    }
    formed.columns = headers
        .into_iter()
        .map(|(name, span)| Named { name, span })
        .collect();
    let message = format!("A {noun} table needs a Markdown separator row after its header");
    let missing = (at, message);
    let n = formed.columns.len();
    let (separators, end_line, grid) = rows(lines, header, n, true, missing, &mut problem);
    (formed.separators, formed.end_line) = (separators, end_line);
    let named = form.table.iter().position(|c| c.reads == Reading::Name);
    let expected = {
        let described: Vec<String> = form
            .table
            .iter()
            .map(|c| {
                let named = c.reads == Reading::Name;
                let name = if named { " name" } else { "" };
                format!("{} {}{name}", article(&c.name), c.name)
            })
            .collect();
        match described.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => String::new(),
        }
    };
    for (row, line, parts) in grid {
        let Some(parts) = parts else {
            let message = "Unclosed table row; use outer | delimiters";
            problem(Span::new(row, 0, line.len()), message.into());
            continue;
        };
        if parts.len() != width {
            let message = format!("Expected {expected}, found {} cells", parts.len());
            problem(Span::new(row, 0, line.len()), message);
            continue;
        }
        if let Some(column) = named {
            let (name, span) = &parts[column];
            let what = &form.table[column].name;
            if !identifier(name) {
                let message = format!("{} names must be identifiers", capitalized(what));
                problem(*span, message);
                continue;
            }
            if formed.rows.iter().any(|cells| cells[column].0 == *name) {
                problem(*span, format!("Duplicate {what} '{name}'"));
            }
        }
        for ((source, span), column) in parts.iter().zip(&form.table) {
            if column.reads.is_expression() && source.is_empty() {
                let mut message = match named {
                    Some(n) => format!("Missing {} {}", form.table[n].name, column.name),
                    None => format!("Missing {}", column.name),
                };
                if !column.example.is_empty() {
                    message.push_str(&format!(", e.g. {}", column.example));
                }
                problem(*span, message);
            }
        }
        formed.rows.push(parts);
    }
    formed
}

/// The form's rows as a table, so typing a pipe finds them as it finds a table.
pub(crate) fn grid(formed: &Formed) -> Table {
    Table {
        definition: formed.definition,
        header: formed.header,
        end_line: formed.end_line,
        columns: formed.columns.clone(),
        separators: formed.separators.clone(),
        rows: formed
            .rows
            .iter()
            .map(|cells| {
                cells
                    .iter()
                    .map(|(source, span)| Cell {
                        source: source.clone(),
                        span: *span,
                        value: Ok(Literal::Text(source.clone())),
                        expression: None,
                    })
                    .collect()
            })
            .collect(),
        types: vec![Some(common::ValueType::Text); formed.columns.len()],
        problems: formed.problems.clone(),
        domains: vec![None; formed.columns.len()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_names_its_arguments() {
        let (name, args) = call("maximize($3 * x + f(y, z))").unwrap();
        assert_eq!(name, "maximize");
        assert_eq!(args, vec![(9, 25)]);
        let source = "solve(a, \"b, c\", [1, 2], $1,000 >= x)";
        let (_, args) = call(source).unwrap();
        let args: Vec<_> = args.iter().map(|&(s, e)| &source[s..e]).collect();
        assert_eq!(args, ["a", "\"b, c\"", "[1, 2]", "$1,000 >= x"]);
        assert_eq!(call("f()").unwrap().1, vec![]);
        assert!(call("f(x) + g(y)").is_none());
        assert!(call("3 * f(x)").is_none());
        assert!(call("f(x").is_none());
    }

    fn maximize() -> Arc<Form> {
        Arc::new(Form {
            name: "maximize".into(),
            module: "plans".into(),
            params: vec!["objective: linear expression".into()],
            reads: vec![Reading::Linear],
            table: vec![
                Column {
                    name: "constraint".into(),
                    reads: Reading::Name,
                    example: String::new(),
                },
                Column {
                    name: "expression".into(),
                    reads: Reading::Constraint,
                    example: "bagels >= 12".into(),
                },
            ],
            unknowns: Unknowns::Free,
            unknown: "decision variable".into(),
            unknown_doc: String::new(),
            noun: "plan".into(),
            returns: "Plan".into(),
            documentation: String::new(),
            example: String::new(),
        })
    }

    #[test]
    fn a_declared_form_takes_its_table_and_reads_its_names() {
        let text = "stock := 4\n\
                    p := maximize($3 * bagels + sum(gear, value) + other.x)\n\
                    | constraint | expression |\n\
                    | --- | --- |\n\
                    | flour | bagels <= stock |\n\
                    | 9bad | x >= 1 |\n\
                    | flour | |\n\
                    | wide | a | b |\n\
                    after";
        let doc = Document::parse_with(text.into(), &[], &[maximize()]);
        let [formed] = doc.forms.as_slice() else {
            panic!("one form")
        };
        assert_eq!((formed.header, formed.end_line), (2, 8));
        let names: Vec<_> = formed.names.iter().map(|n| n.name.as_str()).collect();
        // A sum's columns are its table's; the workspace decides which of
        // the rest the note leaves undefined.
        assert_eq!(names, ["bagels", "gear", "other", "stock"]);
        let rows: Vec<_> = (0..formed.rows.len())
            .map(|row| formed.row_name(row).unwrap().0.as_str())
            .collect();
        assert_eq!(rows, ["flour", "flour"]);
        let problems: Vec<_> = formed.problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(
            problems,
            [
                "Constraint names must be identifiers",
                "Duplicate constraint 'flour'",
                "Missing constraint expression, e.g. bagels >= 12",
                "Expected a constraint name and an expression, found 3 cells",
            ]
        );
        // The form's name is no reference, and its rows are not prose.
        assert!(!doc.references.iter().any(|r| r.name == "maximize"));
        // Without the form, it is a call and the table is prose.
        let plain = Document::parse(text.into());
        assert!(plain.forms.is_empty());
        assert!(plain.references.iter().any(|r| r.name == "maximize"));
    }

    #[test]
    fn a_form_without_its_table_says_so() {
        let doc = Document::parse_with("p := maximize()\n".into(), &[], &[maximize()]);
        let problems: Vec<_> = doc.forms[0]
            .problems
            .iter()
            .map(|p| p.message.as_str())
            .collect();
        assert_eq!(
            problems,
            [
                "maximize() needs an objective expression",
                "A plan needs a | constraint | expression | table on the next line",
            ]
        );
    }
}
