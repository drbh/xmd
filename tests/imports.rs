use chrono::NaiveDate;
use lsp_types::Position;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use wtf::{
    document::Document,
    engine::{Engine, Value},
    workspace::Workspace,
};

fn workspace(notes: &[(&str, &str)]) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: notes
            .iter()
            .map(|(p, s)| {
                (
                    PathBuf::from(format!("/notes/{p}")),
                    Document::parse((*s).into()),
                )
            })
            .collect(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn engine(ws: &Workspace) -> Engine<'_> {
    Engine::new(ws, NaiveDate::from_ymd_opt(2026, 9, 18).unwrap())
}
fn path() -> &'static Path {
    Path::new("/notes/main.wtf")
}

#[test]
fn names_and_solver_variables_are_local_even_when_other_notes_are_loaded() {
    let ws = workspace(&[
        (
            "main.wtf",
            "result := amount\nlocal := 3\nplan := maximize(widgets)\n| constraint | expression |\n| --- | --- |\n| capacity | widgets <= 4 |\n",
        ),
        ("other.wtf", "amount := 42\nlocal := 9\nwidgets := 100\n"),
    ]);
    assert_eq!(
        engine(&ws).named(path(), "result").unwrap_err(),
        "Unknown name 'amount'"
    );
    assert_eq!(engine(&ws).named(path(), "local").unwrap().display(), "3");
    assert_eq!(
        ws.plan_variables(path(), &ws.documents[path()].plans[0])[0]
            .1
            .name,
        "widgets"
    );
    let issues = wtf::diagnostics::collect(
        &ws,
        path(),
        NaiveDate::from_ymd_opt(2026, 9, 18).unwrap(),
        chrono::DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap(),
        false,
    );
    assert!(issues.iter().any(|d| d.message == "Unknown name 'amount'"));
}

#[test]
fn imports_are_lazy_and_preserve_function_and_resource_origins() {
    let ws = workspace(&[
        (
            "main.wtf",
            "src := import(\"./sub/values.wtf\")\namount := 999\nresult := src.double(src.amount)\nlink := src.receipt\n",
        ),
        (
            "sub/values.wtf",
            "amount := 21\ndouble := fn(n) => n + amount\nbroken := missing\n./receipt.svg:receipt\n",
        ),
        ("noise.wtf", "amount := 7\n"),
    ]);
    let mut engine = engine(&ws);
    assert_eq!(engine.named(path(), "result").unwrap().display(), "42");
    assert_eq!(
        engine
            .eval(path(), "get(src, \"amount\")")
            .unwrap()
            .display(),
        "21"
    );
    assert_eq!(
        engine.eval(path(), "get(src, \"absent\")").unwrap(),
        Value::Null
    );
    assert!(engine.eval(path(), "get(src, \"broken\")").is_err());
    assert_eq!(
        engine.eval(path(), "[src, src].amount").unwrap().display(),
        "[21, 21]"
    );
    let Value::Resource(resource) = engine.named(path(), "link").unwrap() else {
        panic!()
    };
    assert_eq!(
        resource.url(path()).unwrap().as_str(),
        "file:///notes/sub/receipt.svg"
    );
    assert!(
        engine
            .eval(path(), "import(\"./\" + \"sub/values.wtf\")")
            .unwrap_err()
            .contains("literal path")
    );
    assert!(
        engine
            .eval(path(), "import(\"values.wtf\")")
            .unwrap_err()
            .contains("explicit .wtf path")
    );
}

#[test]
fn cycles_cross_explicit_imports_without_eagerly_poisoning_other_exports() {
    let ws = workspace(&[
        (
            "main.wtf",
            "other := import(\"./other.wtf\")\na := other.b\nsafe := other.safe\n",
        ),
        (
            "other.wtf",
            "main := import(\"./main.wtf\")\nb := main.a\nsafe := 12\n",
        ),
    ]);
    assert!(
        engine(&ws)
            .named(path(), "a")
            .unwrap_err()
            .contains("cycle")
    );
    assert_eq!(engine(&ws).named(path(), "safe").unwrap().display(), "12");
}

#[test]
fn imported_members_have_definition_rename_completion_and_graph_provenance() {
    let source = "src := import(\"./values.wtf\")\nresult := src.amount + 1\nSee [src.amount].\nlocal := fn(src) => src.amount\n";
    let ws = workspace(&[("main.wtf", source), ("values.wtf", "amount := 20\n")]);
    let foreign = ws
        .resolve(Path::new("/notes/values.wtf"), "amount")
        .unwrap();
    for at in [Position::new(1, 16), Position::new(2, 10)] {
        let (symbol, span) = wtf::intelligence::symbol_at(&ws, path(), at).unwrap();
        assert_eq!(symbol, foreign);
        assert_eq!(span.source(source), "amount");
    }
    let occurrences = wtf::intelligence::occurrences(&ws, &foreign);
    assert_eq!(occurrences.len(), 3, "{occurrences:?}");
    assert!(
        occurrences
            .iter()
            .all(|(p, s)| s.source(&ws.documents[p].text) == "amount")
    );
    let local = ws.resolve(path(), "src").unwrap();
    assert_eq!(wtf::intelligence::occurrences(&ws, &local).len(), 3);
    let result = ws.resolve(path(), "result").unwrap();
    assert!(
        wtf::hierarchy::dependencies(&ws, &result)
            .iter()
            .any(|(s, _)| *s == foreign)
    );
    let items = wtf::intelligence::completions(
        &ws,
        path(),
        Position::new(1, 14),
        chrono::DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap(),
        false,
    );
    assert!(items.iter().any(|c| c.label == "amount"));
    assert!(wtf::tables::validate_rename(&ws, &foreign, "result").is_ok());
}

#[cfg(feature = "native")]
#[test]
fn file_loading_follows_only_explicit_dependencies_including_hidden_and_external_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("notes");
    std::fs::create_dir_all(root.join(".hidden")).unwrap();
    let main = root.join("main.wtf");
    std::fs::write(
        &main,
        "src := import(\"./.hidden/values.wtf\")\nresult := src.amount\n",
    )
    .unwrap();
    std::fs::write(root.join(".gitignore"), ".hidden\n").unwrap();
    std::fs::write(
        root.join(".hidden/values.wtf"),
        "amount := import(\"../../external.wtf\").amount\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("external.wtf"), "amount := 42\n").unwrap();
    std::fs::write(root.join("unrelated.wtf"), [0xff]).unwrap();
    let ws = Workspace::load_file(vec![root], &main).unwrap();
    assert_eq!(ws.documents.len(), 3);
    assert_eq!(engine(&ws).named(&main, "result").unwrap().display(), "42");
}

#[test]
fn multiline_grouped_imports_and_reexports_keep_exact_member_ranges() {
    let text =
        "src := (\n  import(\"./bridge.wtf\")\n)\nanswer := (\n  (src.remote\n    .amount)\n)\n";
    let ws = workspace(&[
        ("main.wtf", text),
        ("bridge.wtf", "remote := import(\"./values.wtf\")\n"),
        ("values.wtf", "amount := 42\n"),
    ]);
    assert_eq!(engine(&ws).named(path(), "answer").unwrap().display(), "42");
    let foreign = ws
        .resolve(Path::new("/notes/values.wtf"), "amount")
        .unwrap();
    let (target, span) = wtf::intelligence::symbol_at(&ws, path(), Position::new(5, 7)).unwrap();
    assert_eq!(target, foreign);
    assert_eq!(span.source(text), "amount");
    let edits = wtf::intelligence::occurrences(&ws, &foreign);
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[1].1.source(text), "amount");
    assert!(ws.documents[path()].imports.contains("./bridge.wtf"));
}
