//! Linear plans: `[name] := maximize(expr)` followed by a constraint table.
//! Parsing only: solving one, and the `PlanValue` a note computes, is
//! `evaluate::plans`, one layer up.
use crate::{
    document::{Document, Named, Problem, identifier},
    tables::{Cell, Table},
};
use common::Span;
use syntax::Literal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Maximize,
    Minimize,
}
impl Goal {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Maximize => "maximize",
            Self::Minimize => "minimize",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Constraint {
    pub named: Named,
    pub source: String,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub definition: usize,
    pub goal: Goal,
    pub objective: String,
    pub objective_span: Span,
    pub header: usize,
    pub end_line: usize,
    pub columns: Vec<Named>,
    pub separators: Vec<String>,
    pub constraints: Vec<Constraint>,
    /// Every distinct name read by the objective or a constraint, first
    /// occurrence first. The workspace decides which are decision variables.
    pub names: Vec<Named>,
    pub problems: Vec<Problem>,
}
/// `maximize(...)` or `minimize(...)` wrapping the whole source, with the byte
/// range of the objective inside the parentheses.
pub fn goal(source: &str) -> Option<(Goal, usize, usize)> {
    let source_end = source.trim_end().len();
    for (keyword, goal) in [("maximize", Goal::Maximize), ("minimize", Goal::Minimize)] {
        let Some(rest) = source.strip_prefix(keyword) else {
            continue;
        };
        let open = keyword.len() + (rest.len() - rest.trim_start().len());
        if source.as_bytes().get(open) != Some(&b'(') || !source[..source_end].ends_with(')') {
            return None;
        }
        // The closing paren must match the opening one, not an inner call.
        let mut depth = 0;
        for (i, c) in source[open..source_end].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && open + i + 1 != source_end {
                        return None;
                    }
                }
                _ => {}
            }
        }
        return Some((goal, open + 1, source_end - 1));
    }
    None
}

pub(crate) fn parse(doc: &Document, definition: usize, lines: &[&str]) -> Plan {
    let def = &doc.definitions[definition];
    let (goal, inner_start, inner_end) = goal(&def.source).unwrap();
    let offset = def.expression_span(&doc.text).start;
    let header = def.end.line + 1;
    let objective = &def.source[inner_start..inner_end];
    let objective_start = inner_start + objective.len() - objective.trim_start().len();
    let objective = objective.trim();
    let mut plan = Plan {
        definition,
        goal,
        objective: objective.into(),
        objective_span: Span::new(def.value_span.line, offset, offset).relative(
            &doc.text,
            objective_start,
            objective_start + objective.len(),
        ),
        header,
        end_line: header,
        columns: vec![],
        separators: vec![],
        constraints: vec![],
        names: vec![],
        problems: vec![],
    };
    let mut problem = |span, message: String| plan.problems.push(Problem { span, message });
    if plan.objective.is_empty() {
        problem(
            def.value_span,
            format!("{}() needs an objective expression", goal.keyword()),
        );
    }
    let Some(headers) = lines
        .get(header)
        .and_then(|l| crate::tables::cells(l, header))
    else {
        problem(
            def.value_span,
            "A plan needs a | constraint | expression | table on the next line".into(),
        );
        return plan;
    };
    if headers.len() != 2 || headers.iter().any(|(name, _)| !identifier(name)) {
        problem(
            Span::new(header, 0, lines[header].len()),
            "Plan tables have two columns: | constraint | expression |".into(),
        );
    }
    plan.columns = headers
        .into_iter()
        .map(|(name, span)| Named { name, span })
        .collect();
    plan.end_line = header + 1;
    if let Some(parts) = lines
        .get(header + 1)
        .and_then(|l| crate::tables::cells(l, header + 1))
    {
        plan.end_line = header + 2;
        plan.separators = parts.iter().map(|(s, _)| s.clone()).collect();
        if parts.len() != plan.columns.len()
            || parts.iter().any(|(s, _)| {
                let core = s.trim_matches(':');
                core.len() < 3 || !core.bytes().all(|c| c == b'-')
            })
        {
            problem(
                Span::new(header + 1, 0, lines[header + 1].len()),
                "Table separator must have one --- cell per column".into(),
            );
        }
    } else {
        problem(
            def.value_span,
            "A plan table needs a Markdown separator row after its header".into(),
        );
    }
    while let Some(line) = lines
        .get(plan.end_line)
        .filter(|l| l.trim_start().starts_with('|'))
    {
        let row = plan.end_line;
        plan.end_line += 1;
        let Some(parts) = crate::tables::cells(line, row) else {
            problem(
                Span::new(row, 0, line.len()),
                "Unclosed table row; use outer | delimiters".into(),
            );
            continue;
        };
        if parts.len() != 2 {
            problem(
                Span::new(row, 0, line.len()),
                format!(
                    "Expected a constraint name and an expression, found {} cells",
                    parts.len()
                ),
            );
            continue;
        }
        let (name, name_span) = &parts[0];
        let (source, span) = &parts[1];
        if !identifier(name) {
            problem(*name_span, "Constraint names must be identifiers".into());
            continue;
        }
        if plan.constraints.iter().any(|c| c.named.name == *name) {
            problem(*name_span, format!("Duplicate constraint '{name}'"));
        }
        if source.is_empty() {
            problem(
                *span,
                "Missing constraint expression, e.g. bagels >= 12".into(),
            );
        }
        plan.constraints.push(Constraint {
            named: Named {
                name: name.clone(),
                span: *name_span,
            },
            source: source.clone(),
            span: *span,
        });
    }
    plan
}
/// Byte regions whose references belong to the plan.
pub fn regions(plan: &Plan) -> impl Iterator<Item = Span> + '_ {
    std::iter::once(plan.objective_span).chain(plan.constraints.iter().map(|c| c.span))
}
pub(crate) fn contains(plan: &Plan, span: Span, text: &str) -> bool {
    regions(plan).any(|r| r.contains(text, span))
}
/// The plan's rows as a table, so formatting and format-on-type align them.
pub(crate) fn grid(plan: &Plan) -> Table {
    Table {
        definition: plan.definition,
        header: plan.header,
        end_line: plan.end_line,
        columns: plan.columns.clone(),
        separators: plan.separators.clone(),
        rows: plan
            .constraints
            .iter()
            .map(|c| {
                [(&c.named.name, c.named.span), (&c.source, c.span)]
                    .into_iter()
                    .map(|(source, span)| Cell {
                        source: source.clone(),
                        span,
                        value: Ok(Literal::Text(source.clone())),
                        expression: None,
                    })
                    .collect()
            })
            .collect(),
        types: vec![Some(common::ValueType::Text); 2],
        problems: plan.problems.clone(),
        domains: vec![None; 2],
    }
}
/// `solve(constraint)`: the body of a goal-seek definition.
pub fn seek_body(source: &str) -> Option<&str> {
    let rest = source.strip_prefix("solve")?;
    let rest = rest.trim_start();
    let inner = rest.strip_prefix('(')?.trim_end().strip_suffix(')')?;
    let mut depth = 0;
    for c in inner.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(inner.trim())
}
