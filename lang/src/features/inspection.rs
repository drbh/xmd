//! Read-only syntax and dependency views over the same model used by the editor.
use crate::{
    catalog::{self, QueryValue as Q, Record},
    document::{Document, Span},
    engine::{Expr, Parser},
    hierarchy,
    workspace::{Symbol, Workspace},
};
use serde_json::{Value, json};
use std::path::Path;

struct Syntax<'a> {
    ws: &'a Workspace,
    path: &'a Path,
    doc: &'a Document,
    nodes: Vec<Value>,
}
impl Syntax<'_> {
    fn add(&mut self, kind: &str, span: Span, parent: Option<usize>, extra: Value) -> usize {
        let index = self.nodes.len();
        let id = format!("{}#ast:{index}", crate::paths::file_url(self.path).unwrap());
        let mut node = json!({
            "id":id, "kind":kind, "name":null,
            "parent":parent.map(|p| self.nodes[p]["id"].clone()), "children":[],
            "text":span.source(&self.doc.text),
            "source":catalog::source(self.ws, self.path, span),
        });
        node.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.nodes.push(node);
        if let Some(parent) = parent {
            self.nodes[parent]["children"]
                .as_array_mut()
                .unwrap()
                .push(json!(id));
        }
        index
    }
    fn block(&self, line: usize, end: usize) -> Span {
        let length = self
            .doc
            .text
            .split_inclusive('\n')
            .skip(line)
            .take(end.saturating_sub(line))
            .map(str::len)
            .sum();
        Span::new(line, 0, length)
    }
    fn expression(&mut self, source: &str, span: Span, parent: usize) {
        let offset = span.source(&self.doc.text).find(source).unwrap_or(0);
        let span = span.relative(&self.doc.text, offset, offset + source.len());
        match Parser::parse(source) {
            Ok(expr) => self.expr(&expr, span, Some(span), parent, "expression"),
            Err(message) => {
                self.add(
                    "parse_error",
                    span,
                    Some(parent),
                    json!({"message":message}),
                );
            }
        }
    }
    fn expr(&mut self, expr: &Expr, base: Span, span: Option<Span>, parent: usize, role: &str) {
        if let Expr::Spanned(start, end, expr) = expr {
            self.expr(
                expr,
                base,
                Some(base.relative(&self.doc.text, *start, *end)),
                parent,
                role,
            );
            return;
        }
        let (kind, data) = match expr {
            Expr::Value(value) => ("literal", json!({"value":Q::from_value(value.clone())})),
            Expr::Name(name) => ("name", json!({"name":name})),
            Expr::Call(name, _) => ("call", json!({"name":name})),
            // A built-in call is still a call node named by its spelling.
            Expr::Builtin(builtin, _) => ("call", json!({"name":builtin.as_str()})),
            Expr::Unary(op, _) => ("unary", json!({"operator":op.as_str()})),
            Expr::Binary(op, _, _) => ("binary", json!({"operator":op.as_str()})),
            Expr::Property(_, key) => ("property", json!({"name":key})),
            Expr::List(_) => ("list", json!({})),
            Expr::Record(_) => ("record", json!({})),
            Expr::Lambda(params, _) => ("lambda", json!({"parameters":params})),
            Expr::Apply(_, _) => ("apply", json!({})),
            Expr::Spanned(..) => unreachable!(),
        };
        let node = self.add(kind, span.unwrap_or(base), Some(parent), data);
        self.nodes[node]["role"] = json!(role);
        match expr {
            Expr::Unary(_, e) | Expr::Property(e, _) => self.expr(e, base, None, node, "operand"),
            Expr::Lambda(_, e) => self.expr(e, base, None, node, "body"),
            Expr::Binary(_, a, b) => {
                self.expr(a, base, None, node, "left");
                self.expr(b, base, None, node, "right");
            }
            Expr::Call(_, items) | Expr::Builtin(_, items) | Expr::List(items) => {
                for (i, e) in items.iter().enumerate() {
                    self.expr(e, base, None, node, &i.to_string());
                }
            }
            Expr::Record(fields) => {
                for (key, e) in fields {
                    self.expr(e, base, None, node, key);
                }
            }
            Expr::Apply(f, args) => {
                self.expr(f, base, None, node, "function");
                for (i, e) in args.iter().enumerate() {
                    self.expr(e, base, None, node, &i.to_string());
                }
            }
            _ => (),
        }
    }
}

/// A flat, linked syntax tree. Raw line nodes retain prose, comments and whitespace
/// that the semantic parser deliberately leaves alone. No expressions are evaluated.
pub(crate) fn ast(ws: &Workspace, path: &Path) -> Vec<Record> {
    let doc = &ws.documents[path];
    let mut syntax = Syntax {
        ws,
        path,
        doc,
        nodes: vec![],
    };
    let root = syntax.add(
        "document",
        Span::new(0, 0, doc.text.len()),
        None,
        json!({"schemaVersion":1}),
    );
    for (line, text) in doc.text.split_inclusive('\n').enumerate() {
        syntax.add(
            "line",
            Span::new(line, 0, text.len()),
            Some(root),
            json!({}),
        );
    }
    let mut sections = Vec::new();
    for (i, section) in doc.sections.iter().enumerate() {
        let parent = doc.sections[..i]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, s)| s.level < section.level && section.line < s.end_line)
            .map(|(i, _)| sections[i])
            .unwrap_or(root);
        sections.push(syntax.add("section", syntax.block(section.line, section.end_line), Some(parent),
            json!({"name":section.named.as_ref().map(|n| &n.name),"title":section.title,"level":section.level})));
    }
    let section_at = |line| {
        doc.sections
            .iter()
            .enumerate()
            .rev()
            .find(|(_, s)| s.line < line && line < s.end_line)
            .map(|(i, _)| sections[i])
            .unwrap_or(root)
    };
    let mut definitions = Vec::new();
    for (i, def) in doc.definitions.iter().enumerate() {
        let table = doc.tables.iter().find(|t| t.definition == i);
        let plan = doc.plans.iter().find(|p| p.definition == i);
        let end = table
            .map(|t| t.end_line)
            .or_else(|| plan.map(|p| p.end_line));
        let span = if let Some(end) = end {
            syntax.block(def.named.span.line, end)
        } else {
            let mut start = def.named.span.start.min(def.value_span.start);
            if doc.line(def.named.span.line)[..start].ends_with('[') {
                start -= 1;
            }
            Span::new(
                def.named.span.line,
                start,
                def.end.end.max(def.value_span.end),
            )
        };
        let node = syntax.add(
            "definition",
            span,
            Some(section_at(def.named.span.line)),
            json!({"name":def.named.name,"computed":def.expression}),
        );
        definitions.push(node);
        if table.is_none() && plan.is_none() {
            if def.expression {
                syntax.expression(&def.source, def.value_span, node);
            } else {
                syntax.add("literal", def.value_span, Some(node), json!({}));
            }
        }
    }
    let mut tasks = Vec::new();
    for task in &doc.tasks {
        let parent = task
            .parent
            .map(|i| tasks[i])
            .unwrap_or_else(|| section_at(task.line));
        let node = syntax.add("task", syntax.block(task.line, task.line + 1), Some(parent),
            json!({"name":task.named.as_ref().map(|n| &n.name),"title":task.title,"checked":task.checked,"tags":task.tags}));
        tasks.push(node);
        syntax.add(
            "checkbox",
            task.checkbox,
            Some(node),
            json!({"checked":task.checked}),
        );
        for (name, attr) in &task.attributes {
            let child = syntax.add(
                "attribute",
                attr.span,
                Some(node),
                json!({"name":name,"value":attr.value}),
            );
            // Attributes may use domain syntax (e.g. @every(day)); retain it verbatim.
            if let Ok(expr) = Parser::parse(&attr.value) {
                syntax.expr(
                    &expr,
                    attr.value_span,
                    Some(attr.value_span),
                    child,
                    "expression",
                );
            }
        }
    }
    for event in &doc.events {
        let node = syntax.add(
            "event",
            syntax.block(event.line, event.line + 1),
            Some(section_at(event.line)),
            json!({"title":event.title}),
        );
        for (name, attr) in &event.attributes {
            let child = syntax.add(
                "attribute",
                attr.span,
                Some(node),
                json!({"name":name,"value":attr.value}),
            );
            if let Ok(expr) = Parser::parse(&attr.value) {
                syntax.expr(
                    &expr,
                    attr.value_span,
                    Some(attr.value_span),
                    child,
                    "expression",
                );
            }
        }
    }
    for table in &doc.tables {
        let node = syntax.add(
            "table",
            syntax.block(table.header, table.end_line),
            Some(definitions[table.definition]),
            json!({}),
        );
        for (i, column) in table.columns.iter().enumerate() {
            syntax.add("column", column.span, Some(node), json!({"name":column.name,"index":i,"type":table.types[i].map(|t| t.as_str()),"domain":table.domains[i].map(|d| d.value_type().as_str())}));
        }
        for (i, row) in table.rows.iter().enumerate() {
            let Some(first) = row.first() else {
                continue;
            };
            let row_node = syntax.add(
                "row",
                syntax.block(first.span.line, first.span.line + 1),
                Some(node),
                json!({"index":i}),
            );
            for (j, cell) in row.iter().enumerate() {
                let child = syntax.add(
                    "cell",
                    cell.span,
                    Some(row_node),
                    json!({"column":j,"name":table.columns.get(j).map(|c| &c.name)}),
                );
                if let Some((source, span)) = &cell.expression {
                    syntax.expression(source, *span, child);
                }
            }
        }
    }
    for plan in &doc.plans {
        let node = syntax.add(
            "plan",
            syntax.block(
                doc.definitions[plan.definition].named.span.line,
                plan.end_line,
            ),
            Some(definitions[plan.definition]),
            json!({"goal":plan.goal.keyword()}),
        );
        let goal = syntax.add("objective", plan.objective_span, Some(node), json!({}));
        syntax.expression(&plan.objective, plan.objective_span, goal);
        for column in &plan.columns {
            syntax.add(
                "column",
                column.span,
                Some(node),
                json!({"name":column.name}),
            );
        }
        for constraint in &plan.constraints {
            let child = syntax.add(
                "constraint",
                constraint.span,
                Some(node),
                json!({"name":constraint.named.name}),
            );
            syntax.expression(&constraint.source, constraint.span, child);
        }
    }
    for day in &doc.days {
        let node = syntax.add(
            "day",
            syntax.block(day.line, day.end_line),
            Some(section_at(day.line)),
            json!({"year":day.year,"month":day.month,"day":day.day}),
        );
        for stop in &day.stops {
            let child = syntax.add("stop", syntax.block(stop.line, stop.end_line), Some(node), json!({"title":stop.title,"time":stop.time.to_string(),"marker":stop.marker_span.map(|s| s.source(&doc.text))}));
            for detail in &stop.details {
                syntax.add(
                    "detail",
                    syntax.block(detail.line, detail.line + 1),
                    Some(child),
                    json!({"name":detail.key,"value":detail.value}),
                );
            }
        }
    }
    for calculation in &doc.calculations {
        let node = syntax.add(
            "calculation",
            calculation.span,
            Some(section_at(calculation.span.line)),
            json!({"bracketed":calculation.bracketed}),
        );
        syntax.expression(&calculation.source, calculation.span, node);
    }
    for reference in &doc.references {
        syntax.add("reference", Span::new(reference.span.line, reference.span.start, reference.end()), Some(section_at(reference.span.line)), json!({"name":reference.name,"property":reference.property,"bracketed":reference.bracket}));
    }
    for link in &doc.links {
        syntax.add(
            "link",
            link.span,
            Some(section_at(link.span.line)),
            json!({"target":link.target}),
        );
    }
    for problem in doc
        .problems
        .iter()
        .chain(doc.tables.iter().flat_map(|t| &t.problems))
        .chain(doc.plans.iter().flat_map(|p| &p.problems))
    {
        syntax.add(
            "problem",
            problem.span,
            Some(root),
            json!({"message":problem.message}),
        );
    }
    syntax
        .nodes
        .into_iter()
        .map(|node| {
            let Q::Object(fields) = Q::from_json(node) else {
                unreachable!()
            };
            Record::projected(path.into(), fields)
        })
        .collect()
}

fn node_id(symbol: &Symbol) -> String {
    hierarchy::encode(symbol).to_string()
}

/// Edges point from a reader to its dependency. File-scoped graphs include external
/// endpoints, marked as such, without expanding the rest of those documents.
pub(crate) fn graph(ws: &Workspace, only: Option<&Path>) -> Q {
    let mut symbols: Vec<_> = hierarchy::nodes(ws)
        .into_iter()
        .filter(|n| only.is_none_or(|p| n.path == p))
        .collect();
    let mut edges = Vec::new();
    let mut external = Vec::new();
    for symbol in &symbols {
        for (target, spans) in hierarchy::dependencies(ws, symbol) {
            if !symbols.contains(&target) && !external.contains(&target) {
                external.push(target.clone());
            }
            edges.push(json!({"from":node_id(symbol),"to":node_id(&target),"reads":spans.into_iter().map(|span| catalog::source(ws, &symbol.path, span)).collect::<Vec<_>>()}));
        }
    }
    symbols.extend(external);
    let nodes: Vec<_> = symbols.iter().map(|symbol| {
        let encoded = hierarchy::encode(symbol);
        json!({"id":node_id(symbol),"name":hierarchy::label(ws, symbol),"kind":encoded["kind"],"symbol":encoded,
            "external":only.is_some_and(|p| p != symbol.path),
            "source":catalog::source(ws, &symbol.path, hierarchy::selection(&ws.documents[&symbol.path], symbol))})
    }).collect();
    Q::from_json(json!({"schemaVersion":1,"nodes":nodes,"edges":edges}))
}
