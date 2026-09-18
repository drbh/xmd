use chrono::{DateTime, Duration, Utc};
use lsp_types::{Position, Range, Url};
use serde_json::{Value, json};
use std::path::Path;
use wtf::{
    document::Document,
    engine::Engine,
    link_features::{LinkContext, LinkFeature},
    workspace::Workspace,
};

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
        .unwrap()
        .to_utc()
}
fn golden(name: &str, value: Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    fn compare(path: &str, a: &Value, b: &Value) {
        match (a, b) {
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{path}");
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    compare(&format!("{path}/{i}"), a, b);
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(
                    a.keys().collect::<Vec<_>>(),
                    b.keys().collect::<Vec<_>>(),
                    "{path}"
                );
                for (key, a) in a {
                    compare(&format!("{path}/{key}"), a, &b[key]);
                }
            }
            _ => assert_eq!(a, b, "{path}"),
        }
    }
    compare(
        name,
        &value,
        &serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap(),
    );
}
#[test]
fn github_matches_the_previous_provider_across_urls_cache_states_and_ages() {
    let provider = &wtf::github::GitHub;
    let urls = [
        "https://github.com/org/repo/pull/42",
        "https://github.com/org/repo/issues/3",
        "https://github.com/org/repo/commit/abcdef12345",
        "http://github.com/org/repo/pull/1",
        "https://github.com.evil/org/repo/pull/1",
        "https://github.com/o/r/pull/no",
        "https://github.com/o/r/pull/1/files",
        "https://github.com/o/r/pull/1?q=yes#top",
        "https://github.com//o//r/pull/01/",
        "https://github.com/o/r/commit/a-b_c.d",
        "https://github.com/o/r/pull/",
        "https://github.com/o/r/tree/main",
        "https://github.com/🦀/r/pull/2",
        "https://github.com/o/r/pull/1%202",
    ];
    let mut cases = vec![];
    for target in urls {
        let url = Url::parse(target).unwrap();
        if !provider.matches(&url) {
            cases.push(json!({"url":target,"matched":false}));
            continue;
        }
        let refresh = provider
            .refresh_request(&url)
            .map(|r| json!({"title":r.title,"program":r.program,"args":r.args,"env":r.env}));
        let ctx = LinkContext {
            url: &url,
            cached: None,
            now: now(),
        };
        let mut states = vec![
            json!({"inlay":provider.inlay(&ctx),"hover":provider.hover(&ctx),"live":provider.time_dependent(&ctx)}),
        ];
        for data in [
            json!({"title":"Hello\nworld","commit":{"message":"Commit\nbody"},"state":"OPEN"}),
            json!({"title":"Draft","state":"OPEN","isDraft":true,"statusCheckRollup":[{"state":"PENDING"}],"reviewDecision":"REVIEW_REQUIRED","commit":{"message":"Commit"}}),
            json!({"title":"Merged","state":"MERGED","mergedAt":"yesterday","statusCheckRollup":[{"conclusion":"SUCCESS"},{"conclusion":"NEUTRAL"},{"conclusion":"SKIPPED"}],"reviewDecision":"APPROVED","commit":{"message":"Commit"}}),
            json!({"title":"Closed","state":"CLOSED","statusCheckRollup":[{"conclusion":"FAILURE"}],"reviewDecision":"CHANGES_REQUESTED","commit":{"message":"Commit"}}),
            json!({"title":"Fallback","state":3,"statusCheckRollup":[{"conclusion":"","state":"ERROR"}],"reviewDecision":"DISMISSED","commit":{"message":"Commit"}}),
        ] {
            for age in [0, 59, 60, 3599, 3600, 86399, 86400, 604800, 1209600, -60] {
                if age != 0 && data["title"] != "Hello\nworld" {
                    continue;
                }
                let m = provider
                    .decode_refresh(&url, &data, now() - Duration::seconds(age))
                    .unwrap();
                let ctx = LinkContext {
                    url: &url,
                    cached: Some(&m),
                    now: now(),
                };
                let properties:Vec<_>=provider.property_names(&url).iter().map(|name|json!({"name":name,"value":provider.property(&ctx,name).map(|v|v.display()).map_err(|e|e.strip_prefix("Plugin github.property: ").unwrap_or(&e).to_owned())})).collect();
                states.push(json!({"age":age,"decoded":{"title":m.title,"state":m.state,"merged":m.merged,"checks":m.checks,"review":m.review},"inlay":provider.inlay(&ctx),"hover":provider.hover(&ctx),"live":provider.time_dependent(&ctx),"properties":properties}));
            }
        }
        cases.push(json!({"url":target,"matched":true,"refresh":refresh,"states":states}));
    }
    golden("github-provider.json", json!(cases));
}
#[test]
fn bundled_inlays_match_previous_positions_labels_tooltips_and_clock_dependencies() {
    let path = Path::new("/notes/parity.wtf");
    let mut cases = vec![];
    for source in [
        "# Work\n- [x] Done @estimate(20m)\n- [ ] Next @estimate(30m)\n## Nested\n- [ ] Parent\n  - [x] Child\n  - [ ] Child @estimate(1h)\n# Empty\n",
        "# Formula\n[$30]:budget\n[$5]:spent\n🦀 Remaining [budget - spent].\n2 + 3\nClock [now()].\nBroken [missing + 2].\n",
        "prices := table\n| Item | Cost |\n| --- | --- |\n| Tea | [$2 * 3] |\n| Pie | [missing + 1] |\n",
        "# Empty\nplain text\n",
    ] {
        let ws = Workspace {
            roots: vec!["/notes".into()],
            documents: [(path.into(), Document::parse(source.into()))].into(),
            cache: Default::default(),
            lookups: Default::default(),
            plugins: Default::default(),
        };
        let mut engine = Engine::at(&ws, now().fixed_offset());
        let features: Vec<&dyn wtf::inlays::InlayFeature> = wtf::plugins::bundled()
            .iter()
            .filter(|m| m.kind == "inlay")
            .map(|m| m as &dyn wtf::inlays::InlayFeature)
            .collect();
        let result = wtf::inlays::collect(
            &mut engine,
            path,
            Range::new(Position::new(0, 0), Position::new(100, 0)),
            &features,
        );
        cases.push(json!({"source":source,"hints":result.hints,"live":result.time_dependent}));
    }
    golden("bundled-inlays.json", json!(cases));
}

#[test]
fn github_title_decoding_and_compatibility_entry_points_preserve_edge_cases() {
    for title in ["", "one\ntwo", "one\r\ntwo", "one\r", "\r", "🦀 title"] {
        let metadata = wtf::github::metadata("pull", &json!({"title":title}), now()).unwrap();
        assert_eq!(metadata.title, title.lines().next().unwrap_or(""));
    }
    assert!(wtf::github::metadata("pull", &json!({"title":42}), now()).is_err());
    let bad = Url::parse("https://github.com/org/repo/tree/main").unwrap();
    assert!(wtf::github::GitHub.refresh_request(&bad).is_none());
    assert!(
        wtf::github::GitHub
            .decode_refresh(&bad, &json!({"title":"Bad"}), now())
            .is_err()
    );
}
