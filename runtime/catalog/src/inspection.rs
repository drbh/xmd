//! Read-only syntax and dependency views over the same model used by the editor.
use crate::{Record, SourceRef, value as q};
use analysis::hierarchy;
use lang::common::Span;
use lang::eval::engine::{Expr, Parser, Value, value_json};
use lang::eval::{Symbol, ToValue, Workspace};
use lang::model::{Attribute, Document, TaskState};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, path::Path};

struct Syntax<'a> {
    ws: &'a Workspace,
    path: &'a Path,
    doc: &'a Document,
    nodes: Vec<Json>,
}
impl Syntax<'_> {
    fn add(
        &mut self,
        kind: &str,
        span: Span,
        parent: impl Into<Option<usize>>,
        extra: Json,
    ) -> usize {
        let parent = parent.into();
        let index = self.nodes.len();
        let id = format!("{}#ast:{index}", lang::common::file_url(self.path).unwrap());
        let mut node = json!({
            "id":id, "kind":kind, "name":null,
            "parent":parent.map(|p| self.nodes[p]["id"].clone()), "children":[],
            "text":span.source(&self.doc.text),
            "source":value_json(&SourceRef::new(self.ws, self.path, span).to_value()),
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
    /// `@name(value)` attributes, with the value's syntax when it parses as an
    /// expression. Attributes may use domain syntax (e.g. @every(day)); that
    /// stays verbatim.
    fn attributes(&mut self, attributes: &BTreeMap<String, Attribute>, parent: usize) {
        for (name, attr) in attributes {
            let child = self.add(
                "attribute",
                attr.span,
                parent,
                json!({"name":name,"value":attr.value}),
            );
            if let Ok(expr) = Parser::parse(&attr.value) {
                self.expr(
                    &expr,
                    attr.value_span,
                    Some(attr.value_span),
                    child,
                    "expression",
                );
            }
        }
    }
    fn expression(&mut self, source: &str, span: Span, parent: usize) {
        let offset = span.source(&self.doc.text).find(source).unwrap_or(0);
        let span = span.relative(&self.doc.text, offset, offset + source.len());
        match Parser::parse(source) {
            Ok(expr) => self.expr(&expr, span, Some(span), parent, "expression"),
            Err(message) => {
                self.add("parse_error", span, parent, json!({"message":message}));
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
            Expr::Value(value) => (
                "literal",
                json!({"value":value_json(&q::query_value(Value::from(value.clone())))}),
            ),
            Expr::Code(code) => ("literal", json!({"value":value_json(&Value::Code(*code))})),
            Expr::Name(name) | Expr::Param { name, .. } => ("name", json!({"name":name})),
            Expr::Call(name, _) => ("call", json!({"name":name})),
            // A built-in call is still a call node named by its spelling.
            Expr::Builtin(builtin, _) => ("call", json!({"name":builtin.as_str()})),
            Expr::Unary(op, _) => ("unary", json!({"operator":op.as_str()})),
            Expr::Binary(op, _, _) => ("binary", json!({"operator":op.as_str()})),
            Expr::Property(_, key) => ("property", json!({"name":key})),
            Expr::List(_) => ("list", json!({})),
            Expr::Record(_) => ("record", json!({})),
            Expr::Lambda(params, _, _) => ("lambda", json!({"parameters":params})),
            Expr::Apply(_, _) => ("apply", json!({})),
            Expr::Spanned(..) => unreachable!(),
        };
        let node = self.add(kind, span.unwrap_or(base), parent, data);
        self.nodes[node]["role"] = json!(role);
        match expr {
            Expr::Unary(_, e) | Expr::Property(e, _) => self.expr(e, base, None, node, "operand"),
            Expr::Lambda(params, defaults, e) => {
                // A default is the child named after the parameter it fills.
                let first = params.len() - defaults.len();
                for (param, default) in params[first..].iter().zip(defaults) {
                    self.expr(default, base, None, node, param);
                }
                self.expr(e, base, None, node, "body")
            }
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
    let doc = &ws.documents()[path];
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
        syntax.add("line", Span::new(line, 0, text.len()), root, json!({}));
    }
    let mut sections = Vec::new();
    for section in &doc.sections {
        let parent = enclosing(doc, &sections, section.line, section.level).unwrap_or(root);
        sections.push(syntax.add("section", syntax.block(section.line, section.end_line), parent,
            json!({"name":section.named.as_ref().map(|n| &n.name),"title":section.title,"level":section.level})));
    }
    let section_at = |line| enclosing(doc, &sections, line, usize::MAX).unwrap_or(root);
    let mut definitions = Vec::new();
    for (i, def) in doc.definitions.iter().enumerate() {
        let end = doc.grid_of(i).map(|(_, end)| end);
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
            section_at(def.named.span.line),
            json!({"name":def.named.name,"computed":def.expression}),
        );
        definitions.push(node);
        if end.is_none() {
            if def.expression {
                syntax.expression(&def.source, def.value_span, node);
            } else {
                syntax.add("literal", def.value_span, node, json!({}));
            }
        }
    }
    let mut tasks = Vec::new();
    for task in &doc.tasks {
        let parent = task
            .parent
            .map(|i| tasks[i])
            .unwrap_or_else(|| section_at(task.line));
        let node = syntax.add("task", syntax.block(task.line, task.line + 1), parent,
            json!({"name":task.named.as_ref().map(|n| &n.name),"title":task.title,"checked":task.state == TaskState::Done,"state":task.state.as_str()}));
        tasks.push(node);
        syntax.add(
            "checkbox",
            task.checkbox,
            node,
            json!({"checked":task.state == TaskState::Done,"state":task.state.as_str()}),
        );
        syntax.attributes(&task.attributes, node);
    }
    // Any other line whose attributes a module declares: what it means is
    // the module's to say.
    for line in doc.claimed().filter(|a| !a.checkbox) {
        let node = syntax.add(
            "attributed",
            syntax.block(line.line, line.line + 1),
            section_at(line.line),
            json!({"title":line.title}),
        );
        syntax.attributes(&line.attributes, node);
    }
    for table in &doc.tables {
        let node = syntax.add(
            "table",
            syntax.block(table.header, table.end_line),
            definitions[table.definition],
            json!({}),
        );
        for (i, column) in table.columns.iter().enumerate() {
            syntax.add("column", column.span, node, json!({"name":column.name,"index":i,"type":table.types[i].map(|t| t.as_str()),"domain":table.domains[i].map(|d| d.value_type().as_str())}));
        }
        for (i, row) in table.rows.iter().enumerate() {
            let Some(first) = row.first() else {
                continue;
            };
            let row_node = syntax.add(
                "row",
                syntax.block(first.span.line, first.span.line + 1),
                node,
                json!({"index":i}),
            );
            for (j, cell) in row.iter().enumerate() {
                let child = syntax.add(
                    "cell",
                    cell.span,
                    row_node,
                    json!({"column":j,"name":table.columns.get(j).map(|c| &c.name)}),
                );
                if let Some((source, span)) = &cell.expression {
                    syntax.expression(source, *span, child);
                }
            }
        }
    }
    // A form that takes a table is a node named as the form calls its
    // definitions (a plan), with its arguments named by their parameters,
    // its columns, and each expression of a row named by the column that
    // names the row (a constraint).
    for formed in doc.forms.iter().filter(|f| f.has_table()) {
        let form = &formed.form;
        let node = syntax.add(
            &form.noun,
            syntax.block(
                doc.definitions[formed.definition].named.span.line,
                formed.end_line,
            ),
            definitions[formed.definition],
            json!({"form":form.name}),
        );
        for (i, (source, span)) in formed.arguments.iter().enumerate() {
            let param = form.params.get(i).map_or("argument", String::as_str);
            let param = param.split(':').next().unwrap_or(param).trim();
            let argument = syntax.add(param, *span, node, json!({}));
            syntax.expression(source, *span, argument);
        }
        for column in &formed.columns {
            syntax.add("column", column.span, node, json!({"name":column.name}));
        }
        let names = form
            .table
            .iter()
            .find(|c| c.reads == lang::eval::forms::Reading::Name);
        for (row, cells) in formed.rows.iter().enumerate() {
            let name = formed.row_name(row).map(|(name, _)| name);
            for ((source, span), column) in cells.iter().zip(&form.table) {
                if !column.reads.is_expression() {
                    continue;
                }
                let child = syntax.add(
                    &names.unwrap_or(column).name,
                    *span,
                    node,
                    json!({"name":name}),
                );
                syntax.expression(source, *span, child);
            }
        }
    }
    // What a module's recognizers found, under the match each is `under`:
    // what it means is the module's, so a node names only the recognizer
    // and the text of each group.
    let mut matches = Vec::with_capacity(doc.recognized.len());
    for found in &doc.recognized {
        // A match that only paints is no node; nothing is under it.
        if !found.rule.record {
            matches.push(section_at(found.span.line));
            continue;
        }
        let span = if found.rule.until.is_some() {
            syntax.block(found.span.line, found.end)
        } else {
            found.span
        };
        let parent = found
            .parent
            .and_then(|p| matches.get(p).copied())
            .unwrap_or_else(|| section_at(found.span.line));
        let groups: serde_json::Map<String, Json> = found
            .groups
            .iter()
            .map(|g| (g.name.clone(), json!(g.span.source(doc))))
            .collect();
        matches.push(syntax.add(
            "recognized",
            span,
            parent,
            json!({"name":found.rule.name,"module":found.rule.module,"groups":groups}),
        ));
    }
    for calculation in &doc.calculations {
        let node = syntax.add(
            "calculation",
            calculation.span,
            section_at(calculation.span.line),
            json!({"bracketed":calculation.bracketed}),
        );
        syntax.expression(&calculation.source, calculation.span, node);
    }
    for reference in &doc.references {
        syntax.add("reference", reference.full_span(), section_at(reference.span.line), json!({"name":reference.name,"property":reference.property,"bracketed":reference.bracket}));
    }
    for link in &doc.links {
        syntax.add(
            "link",
            link.span,
            section_at(link.span.line),
            json!({"target":link.target}),
        );
    }
    for problem in doc
        .problems
        .iter()
        .chain(doc.tables.iter().flat_map(|t| &t.problems))
        .chain(doc.forms.iter().flat_map(|f| &f.problems))
    {
        syntax.add(
            "problem",
            problem.span,
            root,
            json!({"message":problem.message}),
        );
    }
    syntax
        .nodes
        .into_iter()
        .map(|node| {
            let Value::Record(fields) = q::from_json(node) else {
                unreachable!()
            };
            Record::typed(path, std::sync::Arc::unwrap_or_clone(fields).into_inner())
        })
        .collect()
}

/// The node of the innermost section, among those already added, that
/// contains `line` and is shallower than `level`.
fn enclosing(doc: &Document, sections: &[usize], line: usize, level: usize) -> Option<usize> {
    doc.sections
        .iter()
        .zip(sections)
        .rev()
        .find(|(s, _)| s.level < level && s.line < line && line < s.end_line)
        .map(|(_, node)| *node)
}

fn node_id(symbol: &Symbol) -> String {
    hierarchy::encode(symbol).to_string()
}

/// Edges point from a reader to its dependency. File-scoped graphs include external
/// endpoints, marked as such, without expanding the rest of those documents.
pub(crate) fn graph(ws: &Workspace, only: Option<&Path>) -> Value {
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
            edges.push(json!({"from":node_id(symbol),"to":node_id(&target),"reads":spans.into_iter().map(|span| value_json(&SourceRef::new(ws, &symbol.path, span).to_value())).collect::<Vec<_>>()}));
        }
    }
    symbols.extend(external);
    let nodes: Vec<_> = symbols.iter().map(|symbol| {
        let encoded = hierarchy::encode(symbol);
        json!({"id":node_id(symbol),"name":hierarchy::label(ws, symbol),"kind":encoded["kind"],"symbol":encoded,
            "external":only.is_some_and(|p| p != symbol.path),
            "source":value_json(&SourceRef::new(ws, &symbol.path, hierarchy::selection(&ws.documents()[&symbol.path], symbol)).to_value())})
    }).collect();
    q::from_json(json!({"schemaVersion":1,"nodes":nodes,"edges":edges}))
}
