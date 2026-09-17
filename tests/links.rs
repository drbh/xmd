use jot::{document::Document, resources::Resource, workspace::Workspace};
use std::path::Path;

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}

#[test]
fn raw_resources_are_precise_prose_links_without_becoming_symbols() {
    let source = "🦀 See https://example.com/docs?q=1#part, and (https://example.com/a_(b)).\nFiles: ./notes.jot, ../src/main.rs; src/lib.rs and README.md.\nConfig ~/.config/zed/settings.json or /tmp/log.txt; file:///tmp/a%20b.txt.\nMap geo:40.7,-74.0.\n# Review https://github.com/zed-industries/zed\n- [ ] Read https://example.com/@at(today)/[page] @estimate(5m)\n";
    let doc = Document::parse(source.into());
    let targets: Vec<_> = doc.links.iter().map(|l| l.target.as_str()).collect();
    assert_eq!(
        targets,
        [
            "https://example.com/docs?q=1#part",
            "https://example.com/a_(b)",
            "./notes.jot",
            "../src/main.rs",
            "src/lib.rs",
            "README.md",
            "~/.config/zed/settings.json",
            "/tmp/log.txt",
            "file:///tmp/a%20b.txt",
            "geo:40.7,-74.0",
            "https://github.com/zed-industries/zed",
            "https://example.com/@at(today)/[page]",
        ]
    );
    assert!(doc.references.is_empty());
    assert!(doc.problems.is_empty(), "{:?}", doc.problems);
    assert_eq!(doc.tasks[0].attributes.len(), 1);
    for link in &doc.links {
        assert_eq!(
            &doc.line(link.span.line)[link.span.start..link.span.end],
            link.target
        );
    }
}

#[test]
fn code_comments_formulas_numbers_and_existing_links_are_not_reparsed_as_raw_prose() {
    let source = "`https://example.com ./code.rs`\n```\n./hidden.rs\n```\n<!-- /hidden/file.rs -->\nNo links: 3.14 3/4 2026-09-16 and/or e.g. example.com\n[a] := 3 / 4\n[link](./one.jot) [https://example.com]:site\n";
    let doc = Document::parse(source.into());
    assert_eq!(doc.links.len(), 1, "{:?}", doc.links);
    assert_eq!(doc.links[0].target, "./one.jot");
    assert_eq!(doc.definitions.len(), 2);
    assert!(doc.problems.is_empty());
    let malicious = Document::parse("javascript:alert(1) data:text/html,bad https://\n".into());
    assert!(malicious.links.is_empty());
}

#[test]
fn link_targets_hovers_and_controls_share_origin_and_utf16_ranges() {
    let path = Path::new("/notes/trips/today.jot");
    let source = "🦀 ../packing.jot and ./images/map.png\n[src/main.rs]:source_file\nOpen [source_file].\n[asset] := table\n| name | file |\n| --- | --- |\n| map | ./images/map.png |\n";
    let doc = Document::parse(source.into());
    let ws = Workspace {
        roots: vec!["/notes".into()],
        documents: [(path.into(), doc)].into(),
        cache: Default::default(),
    };
    let links = jot::presentation::document_links(&ws, path, now());
    assert_eq!(links.len(), 5, "{links:?}");
    assert_eq!(links[0].range.start.character, 3);
    assert_eq!(
        links[0].target.as_ref().unwrap().as_str(),
        "file:///notes/packing.jot"
    );
    assert_eq!(
        links[1].target.as_ref().unwrap().as_str(),
        "file:///notes/trips/images/map.png"
    );
    assert_eq!(links[2].target, links[3].target);
    assert_eq!(
        links[2].target.as_ref().unwrap().as_str(),
        "file:///notes/trips/src/main.rs"
    );
    let hover = jot::intelligence::link_hover(&ws, path, lsp_types::Position::new(0, 4)).unwrap();
    assert_eq!(hover.range, Some(links[0].range));
    assert!(
        serde_json::to_string(&hover)
            .unwrap()
            .contains("file:///notes/packing.jot")
    );
    let commands = jot::interaction::row_commands(&ws, path, 0, now(), false);
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.command == "jot.openResource")
            .count(),
        2
    );
    assert!(jot::intelligence::link_hover(&ws, path, lsp_types::Position::new(0, 1)).is_none());
}

#[test]
fn file_resolution_preserves_spaces_unicode_and_home_paths() {
    let path = Path::new("/notes/trip.jot");
    assert_eq!(
        Resource::parse("./maps/日本 map.png")
            .unwrap()
            .url(path)
            .unwrap()
            .as_str(),
        "file:///notes/maps/%E6%97%A5%E6%9C%AC%20map.png"
    );
    assert_eq!(
        Resource::parse("~/.config/zed/settings.json")
            .unwrap()
            .url(path)
            .unwrap(),
        jot::paths::file_url(
            Path::new(&std::env::var_os("HOME").unwrap()).join(".config/zed/settings.json")
        )
        .unwrap()
    );
    assert!(Resource::parse("README.md").is_some());
    assert!(Resource::parse(".gitignore").is_some());
    assert!(Resource::parse("3.14").is_none());
    assert!(Resource::parse(".25").is_none());
    assert!(Resource::parse("example.com").is_none());
}

#[test]
fn github_links_in_prose_carry_a_status_badge_that_ages() {
    use chrono::{DateTime, Utc};
    let source = "- [ ] multicast https://github.com/NVlabs/cuda-oxide/pull/606\nSee [the fix](https://github.com/NVlabs/cuda-oxide/pull/975) and https://github.com/NVlabs/cuda-oxide/pull/1286.\nNot a PR: https://github.com/drbh/kawaii\n";
    let mut ws = jot::workspace::Workspace {
        roots: vec!["/notes".into()],
        documents: [(
            std::path::PathBuf::from("/notes/work.jot"),
            jot::document::Document::parse(source.into()),
        )]
        .into(),
        cache: Default::default(),
    };
    let fetched: DateTime<Utc> = "2026-09-16T16:00:00Z".parse().unwrap();
    let now = DateTime::parse_from_rfc3339("2026-09-16T18:05:00+00:00").unwrap();
    ws.cache.insert(
        "https://github.com/NVlabs/cuda-oxide/pull/606".into(),
        jot::resources::metadata("pull", &serde_json::json!({"title":"multicast","state":"MERGED","mergedAt":"2026-09-10T00:00:00Z","statusCheckRollup":[{"conclusion":"SUCCESS"}],"reviewDecision":"APPROVED"}), fetched).unwrap(),
    );
    ws.cache.insert(
        "https://github.com/NVlabs/cuda-oxide/pull/975".into(),
        jot::resources::metadata("pull", &serde_json::json!({"title":"mir pass","state":"OPEN","statusCheckRollup":[{"conclusion":"FAILURE"}],"reviewDecision":"CHANGES_REQUESTED"}), fetched - chrono::Duration::days(9)).unwrap(),
    );
    let path = std::path::Path::new("/notes/work.jot");
    let hints = jot::presentation::hints_at(
        &ws,
        path,
        now,
        tower_lsp::lsp_types::Range::new(
            tower_lsp::lsp_types::Position::new(0, 0),
            tower_lsp::lsp_types::Position::new(9, 0),
        ),
    );
    let labels: Vec<(u32, String)> = hints
        .iter()
        .map(|h| {
            (
                h.position.line,
                match &h.label {
                    tower_lsp::lsp_types::InlayHintLabel::String(s) => s.clone(),
                    other => panic!("{other:?}"),
                },
            )
        })
        .collect();
    assert_eq!(
        labels,
        [
            (0, "merged · checks ok · approved · 2h ago".to_string()),
            (
                1,
                "open · checks FAILING · CHANGES REQUESTED · stale · 9d ago".to_string()
            ),
            (1, "PR #1286 · refresh for status".to_string()),
        ]
    );
    // The badge sits right after the link, and the hover carries the title.
    assert_eq!(
        hints[2].position.character,
        source.lines().nth(1).unwrap().find("/1286").unwrap() as u32 + 5
    );
    match &hints[0].tooltip {
        Some(tower_lsp::lsp_types::InlayHintTooltip::MarkupContent(m)) => {
            assert!(m.value.contains("**multicast**"), "{}", m.value)
        }
        other => panic!("{other:?}"),
    }
}
