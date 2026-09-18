use chrono::{DateTime, FixedOffset};
use std::{collections::BTreeMap, path::Path};
use tower_lsp::lsp_types::*;
use wtf::{
    actions, diagnostics,
    document::Document,
    engine::{Engine, Value},
    hierarchy, intelligence, plans, presentation, symbols, tables, typing,
    workspace::{Symbol, SymbolKind, Workspace},
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
    }
}
fn point(ws: &Workspace, row: usize, needle: &str) -> Position {
    let line = ws.documents[path()].line(row);
    let end = line.find(needle).unwrap() + needle.len();
    Position::new(row as u32, line[..end].encode_utf16().count() as u32)
}
fn def(i: usize) -> Symbol {
    Symbol {
        path: path().into(),
        kind: SymbolKind::Definition(i),
    }
}
fn plan(ws: &Workspace, name: &str) -> std::sync::Arc<plans::PlanValue> {
    match Engine::at(ws, now()).named(path(), name) {
        Ok(Value::Plan(p)) => p,
        other => panic!("{other:?}"),
    }
}
fn messages(ws: &Workspace) -> Vec<String> {
    diagnostics::collect(ws, path(), now().date_naive(), now(), false)
        .into_iter()
        .map(|d| d.message)
        .collect()
}

const BAKERY: &str = "\
[400]:flour_stock
[bakery] := maximize($3 * bagels + $1.25 * doughnuts)
| constraint   | expression                                   |
| ------------ | -------------------------------------------- |
| flour        | 12 * bagels + 6.5 * doughnuts <= flour_stock |
| milk         | bagels + 0.5 * doughnuts <= 200              |
| bagel_min    | bagels >= 12                                 |
| doughnut_min | doughnuts >= 14                              |
Bake [bakery.bagels] bagels for [bakery].
";

#[test]
fn a_plan_solves_reactively_with_note_values_as_constants() {
    let ws = note(BAKERY);
    let solved = plan(&ws, "bakery");
    assert_eq!(
        solved.objective,
        Value::Money(94.75, wtf::engine::Currency::USD)
    );
    assert_eq!(
        solved.variables,
        [
            ("bagels".to_string(), Value::Number(25.75)),
            ("doughnuts".to_string(), Value::Number(14.0))
        ]
    );
    let flour = &solved.constraints[0];
    assert!(flour.binding);
    assert_eq!(
        (flour.lhs.clone(), flour.rhs.clone()),
        (Value::Number(400.0), Value::Number(400.0))
    );
    assert_eq!(solved.constraints[1].slack, Value::Number(167.25));
    assert_eq!(messages(&ws), Vec::<String>::new());
    let mut engine = Engine::at(&ws, now());
    assert_eq!(
        engine.eval(path(), "bakery.bagels").unwrap(),
        Value::Number(25.75)
    );
    assert_eq!(
        engine.eval(path(), "bakery.flour").unwrap(),
        Value::Number(0.0)
    );
    assert_eq!(
        engine.eval(path(), "bagels * 2").unwrap(),
        Value::Number(51.5)
    );
    // Less flour in the note, less profit: the plan re-solves from the edit.
    let less = note(&BAKERY.replace("[400]:flour_stock", "[300]:flour_stock"));
    assert_eq!(
        plan(&less, "bakery").objective,
        Value::Money(69.75, wtf::engine::Currency::USD)
    );
}

#[test]
fn units_are_checked_and_nonlinear_terms_are_rejected() {
    let cost = note(
        "[budget] := minimize(30m * calls + 1h * visits)\n| constraint | expression |\n| --- | --- |\n| coverage | calls + 3 * visits >= 10 |\n",
    );
    assert_eq!(plan(&cost, "budget").objective, Value::Duration(200 * 60));
    let mixed = note(
        "[p] := maximize($3 * x + 2h * y)\n| constraint | expression |\n| --- | --- |\n| c | x <= 1 |\n",
    );
    assert!(
        messages(&mixed)
            .iter()
            .any(|m| m == "Cannot add Money and Duration"),
        "{:?}",
        messages(&mixed)
    );
    let quadratic = note(
        "[p] := maximize(x * y)\n| constraint | expression |\n| --- | --- |\n| c | x <= 1 |\n| d | y <= 1 |\n",
    );
    assert!(
        messages(&quadratic)
            .iter()
            .any(|m| m.contains("stay linear")),
        "{:?}",
        messages(&quadratic)
    );
    let compare =
        note("[p] := maximize(x)\n| constraint | expression |\n| --- | --- |\n| c | x < 1 |\n");
    assert!(
        messages(&compare)
            .iter()
            .any(|m| m == "Constraints use <=, >=, or ==, not <"),
        "{:?}",
        messages(&compare)
    );
    let table = note("[p] := maximize(x)\n| constraint |\n| --- |\n| c |\n");
    assert!(
        messages(&table).iter().any(|m| m.contains("two columns")),
        "{:?}",
        messages(&table)
    );
}

#[test]
fn infeasible_and_unbounded_plans_report_on_the_objective() {
    let unbounded = note(
        "[p] := maximize(x)\n| constraint | expression |\n| ---------- | ---------- |\n| floor      | x >= 1     |\n",
    );
    let issues = diagnostics::collect(&unbounded, path(), now().date_naive(), now(), false);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].message.contains("without limit"));
    assert_eq!(issues[0].range.start, Position::new(0, 16));
    let infeasible = note(
        "[p] := minimize(x)\n| constraint | expression |\n| ---------- | ---------- |\n| low        | x <= 1     |\n| high       | x >= 2     |\n",
    );
    let issues = messages(&infeasible);
    assert_eq!(
        issues,
        ["No values satisfy every constraint; relax one of them"]
    );
}

#[test]
fn decision_variables_are_symbols_with_hover_rename_and_completion() {
    let ws = note(BAKERY);
    let (symbol, _) = intelligence::symbol_at(&ws, path(), point(&ws, 4, "12 * bag")).unwrap();
    assert_eq!(symbol.kind, SymbolKind::Variable(0, 0));
    assert_eq!(ws.named(&symbol).name, "bagels");
    // The declaration is the first occurrence, in the objective.
    assert_eq!(ws.named(&symbol).span.line, 1);
    let hover = intelligence::hover(&ws, &symbol, now());
    assert!(hover.starts_with("**bagels · Number**\n\n25.75"), "{hover}");
    assert!(hover.contains("Decision variable of [bakery]"), "{hover}");
    let plan_hover = intelligence::hover(&ws, &def(1), now());
    assert!(
        plan_hover.contains("Variables: bagels = 25.75, doughnuts = 14"),
        "{plan_hover}"
    );
    assert!(
        plan_hover.contains("- flour: `██████████ 100%` 400 ≤ 400 · binding"),
        "{plan_hover}"
    );
    assert!(
        plan_hover.contains("- milk: `██░░░░░░░░ 16%` 32.75 ≤ 200 · slack 167.25"),
        "{plan_hover}"
    );
    let completions =
        intelligence::completions(&ws, path(), point(&ws, 8, "[bakery."), now(), false);
    let labels: Vec<_> = completions.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "objective",
            "bagels",
            "doughnuts",
            "flour",
            "milk",
            "bagel_min",
            "doughnut_min"
        ]
    );
    // A note value with the same name turns the variable into a constant.
    let shadowed = note(&format!("{BAKERY}[20]:doughnuts\n"));
    assert_eq!(plan(&shadowed, "bakery").variables.len(), 1);
    assert!(
        shadowed
            .symbols()
            .iter()
            .all(|s| s.kind != SymbolKind::Variable(0, 1))
    );
}

#[test]
fn inlays_symbols_and_formatting_cover_the_constraint_table() {
    let ws = note(BAKERY);
    let hints = presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(20, 0)),
    );
    let label = |line: u32| {
        hints
            .iter()
            .find(|h| h.position.line == line)
            .map(|h| match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap_or_default()
    };
    assert_eq!(label(1), "= $94.75 · bagels 25.75 · doughnuts 14");
    assert_eq!(label(4), "████████ 400 ≤ 400 · binding");
    assert_eq!(label(5), "█░░░░░░░ 32.75 ≤ 200 · slack 167.25");
    assert_eq!(label(6), "25.75 ≥ 12 · slack 13.75");
    let outline = symbols::document_symbols(&ws, path(), now());
    let bakery = outline.iter().find(|s| s.name == "bakery").unwrap();
    assert_eq!(bakery.kind, tower_lsp::lsp_types::SymbolKind::STRUCT);
    assert_eq!(bakery.range.end.line, 7);
    let children: Vec<_> = bakery
        .children
        .as_ref()
        .unwrap()
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        children,
        [
            "bagels",
            "doughnuts",
            "flour",
            "milk",
            "bagel_min",
            "doughnut_min"
        ]
    );
    let ragged =
        "[p] := maximize(x + y)\n|constraint|expression|\n|---|---|\n|a|x <= 4|\n|b|y <= x|\n";
    let doc = Document::parse(ragged.into());
    let formatted = actions::apply_edits(ragged, &tables::formatting(&doc)).unwrap();
    assert!(formatted.contains("| constraint | expression |\n| ---------- | ---------- |\n| a          | x <= 4     |\n"), "{formatted}");
    let typed =
        actions::apply_edits(ragged, &typing::on_type(&doc, Position::new(4, 10), "|")).unwrap();
    assert_eq!(typed, formatted);
}

#[test]
fn the_dependency_graph_links_plans_constants_and_variables() {
    let ws = note(BAKERY);
    let names = |edges: Vec<(Symbol, Vec<wtf::document::Span>)>| {
        edges
            .into_iter()
            .map(|(s, _)| hierarchy::label(&ws, &s))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(hierarchy::dependencies(&ws, &def(1))),
        ["flour_stock"]
    );
    let bagels = Symbol {
        path: path().into(),
        kind: SymbolKind::Variable(0, 0),
    };
    assert_eq!(names(hierarchy::dependencies(&ws, &bagels)), ["bakery"]);
    assert_eq!(
        hierarchy::item(&ws, &bagels, now()).detail.as_deref(),
        Some("decision variable of bakery · 25.75")
    );
    assert_eq!(
        hierarchy::item(&ws, &def(1), now()).detail.as_deref(),
        Some("plan · maximize $94.75 · 2 variables")
    );
    let mut readers = names(hierarchy::dependents(&ws, &def(1)));
    readers.sort();
    assert_eq!(readers, ["bagels", "doughnuts"]);
}

#[test]
fn alps_problem_files_round_trip() {
    let ws = note(BAKERY);
    let (_, p) = plans::plan(&ws, &def(1)).unwrap();
    let exported = plans::export(&mut Engine::at(&ws, now()), &def(1), p).unwrap();
    assert_eq!(
        exported["objective"]["expression"],
        "3 * bagels + 1.25 * doughnuts"
    );
    assert_eq!(
        exported["constraints"][0]["expression"],
        "12 * bagels + 6.5 * doughnuts <= 400"
    );
    assert_eq!(exported["variables"]["bagels"], serde_json::json!({}));
    let source = plans::import("bakery", &exported).unwrap();
    let reimported = note(&source);
    assert_eq!(plan(&reimported, "bakery").objective, Value::Number(94.75));
    assert!(
        tables::formatting(&reimported.documents[path()]).is_empty(),
        "{source}"
    );
    let bounded = serde_json::json!({"variables": {"x": {"min": 2, "max": 5}}, "objective": {"goal": "min", "expression": "x"}, "constraints": []});
    let source = plans::import("tiny", &bounded).unwrap();
    assert!(source.contains("| x_min      | x >= 2     |"), "{source}");
    assert_eq!(plan(&note(&source), "tiny").objective, Value::Number(2.0));
    assert!(plans::import("not valid", &bounded).is_err());
}

#[test]
fn goal_seek_inverts_a_chain_of_calculations() {
    let ws = note(
        "[$1,200]:saved\n[9]:months_left\n[saved_by_june] := monthly * months_left + saved\n[monthly] := solve(saved_by_june >= $5,000)\nSave [monthly] a month.\n",
    );
    let mut engine = Engine::at(&ws, now());
    assert_eq!(
        engine.named(path(), "monthly").unwrap().display(),
        "$422.22"
    );
    assert_eq!(
        engine.named(path(), "saved_by_june").unwrap().display(),
        "$5,000"
    );
    assert_eq!(messages(&ws), Vec::<String>::new());
    let hover = intelligence::hover(&ws, &def(3), now());
    assert!(
        hover.contains("Goal seek: the smallest value that satisfies `saved_by_june >= $5,000`"),
        "{hover}"
    );
    // Units follow the chain: a duration unknown, a plain-number unknown, a ceiling.
    let hours = note("[30m]:per_call\n[calls] := solve(calls * per_call <= 4h)\n");
    assert_eq!(
        Engine::at(&hours, now()).named(path(), "calls").unwrap(),
        Value::Number(8.0)
    );
    let exact = note("[price] := solve(price * 40 == $1,000)\n");
    assert_eq!(
        Engine::at(&exact, now()).named(path(), "price").unwrap(),
        Value::Money(25.0, wtf::engine::Currency::USD)
    );
    let unrelated = note("[x] := solve(3 >= 2)\n");
    assert_eq!(
        messages(&unrelated),
        ["The constraint does not depend on x"]
    );
    // A second unknown is an ordinary unknown name; only plans infer variables.
    let two = note("[x] := solve(x + y >= 2)\n");
    assert_eq!(messages(&two), ["Unknown name 'y'"]);
    let declared = note("[y] := solve(y >= 1)\n[x] := solve(x + y >= 2)\n");
    assert_eq!(
        Engine::at(&declared, now()).named(path(), "x").unwrap(),
        Value::Number(1.0)
    );
    let cyclic = note("[a] := b\n[b] := a\n[x] := solve(a >= 1)\n");
    assert!(
        messages(&cyclic).iter().any(|m| m.contains("cycle")),
        "{:?}",
        messages(&cyclic)
    );
}

const GEAR: &str = "\
[gear] := table
| item   | weight | value | take? |
| ------ | ------ | ----- | ----- |
| tent   | 3      | 9     |       |
| stove  | 1      | 4     |       |
| camera | 2      | 7     |       |
| books  | 4      | 3     |       |
[pack] := maximize(sum(gear, value * take))
| constraint | expression                    |
| ---------- | ----------------------------- |
| weight     | sum(gear, weight * take) <= 6 |
[menu] := table
| dish  | cost | protein | servings# |
| ----- | ---- | ------- | --------- |
| beans | $2   | 15      |           |
| eggs  | $3   | 12      |           |
[diet] := minimize(sum(menu, cost * servings))
| constraint | expression                          |
| ---------- | ----------------------------------- |
| protein    | sum(menu, protein * servings) >= 50 |
[wrong] := sum(gear, weight * take)
";

#[test]
fn decision_columns_become_per_row_choices_and_counts() {
    let ws = note(GEAR);
    let pack = plan(&ws, "pack");
    assert_eq!(pack.objective, Value::Number(20.0));
    assert!(
        pack.variables.is_empty(),
        "columns are not scalar variables: {:?}",
        pack.variables
    );
    let choices: Vec<_> = pack
        .rows
        .iter()
        .map(|(r, v)| (r.name.as_str(), v.clone()))
        .collect();
    assert_eq!(
        choices,
        [
            ("gear.take[1]", Value::Bool(true)),
            ("gear.take[2]", Value::Bool(true)),
            ("gear.take[3]", Value::Bool(true)),
            ("gear.take[4]", Value::Bool(false)),
        ]
    );
    let diet = plan(&ws, "diet");
    assert_eq!(
        diet.objective,
        Value::Money(8.0, wtf::engine::Currency::USD)
    );
    assert_eq!(diet.rows[0].1, Value::Number(4.0));
    assert_eq!(diet.rows[1].1, Value::Number(0.0));
    // Decision columns are not data outside a plan, and column names never leak as variables.
    let issues = messages(&ws);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("decision column"), "{issues:?}");
    assert!(
        ws.symbols()
            .iter()
            .all(|s| !matches!(s.kind, SymbolKind::Variable(..)))
    );
    let hover = intelligence::hover(
        &ws,
        &Symbol {
            path: path().into(),
            kind: SymbolKind::Column(0, 3),
        },
        now(),
    );
    assert!(
        hover.contains("Decision column of `gear` (name?)"),
        "{hover}"
    );
    let plan_hover = intelligence::hover(&ws, &def(1), now());
    assert!(
        plan_hover.contains("take: tent, stove, camera, ~~books~~"),
        "{plan_hover}"
    );
}

#[test]
fn decision_cells_get_inlays_and_a_code_action_writes_them_back() {
    let ws = note(GEAR);
    let hints = presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(30, 0)),
    );
    let labels = |line: u32| {
        hints
            .iter()
            .filter(|h| h.position.line == line)
            .map(|h| match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(3), ["→ yes"]);
    assert_eq!(labels(6), ["→ no"]);
    assert_eq!(labels(7), ["= 20 · take 3 of 4"]);
    assert_eq!(labels(14), ["→ 4"]);
    assert_eq!(labels(16), ["= $8 · servings 4"]);
    let line = Position::new(7, 2);
    let actions = wtf::refactor::actions_for(&ws, path(), Range::new(line, line), now());
    let fill = actions
        .iter()
        .find(|a| a.title == "Write the plan's choices into the table")
        .unwrap();
    assert_eq!(fill.edits.len(), 4);
    let written = actions::apply_edits(GEAR, &fill.edits).unwrap();
    assert!(
        written.contains("| tent   | 3      | 9     | yes   |"),
        "{written}"
    );
    assert!(
        written.contains("| books  | 4      | 3     | no    |"),
        "{written}"
    );
    // Written values are notes, not data: the plan still decides, and formatting holds.
    let rewritten = note(&written);
    assert_eq!(plan(&rewritten, "pack").objective, Value::Number(20.0));
    assert!(tables::formatting(&rewritten.documents[path()]).is_empty());
    let none = wtf::refactor::actions_for(&rewritten, path(), Range::new(line, line), now());
    assert!(none.iter().all(|a| !a.title.starts_with("Write the plan")));
}
