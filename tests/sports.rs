//! The workspace `sports` link module and its `scores` library, compiled from
//! the files under examples/modules and exercised through the ordinary engine.
use chrono::{DateTime, FixedOffset};
use lsp_types::{InlayHintLabel, Position, Range};
use std::{path::Path, sync::Arc};
use wtf::{
    document::Document,
    engine::{Engine, Value},
    modules::ModuleRegistry,
    workspace::Workspace,
};

const URL: &str = "https://www.espn.com/nba/game/_/gameId/401584896";

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/main.wtf")
}
fn modules() -> Arc<ModuleRegistry> {
    Arc::new(
        ModuleRegistry::compile(
            [
                (
                    "/notes/.wtf/modules/scores.wtf".into(),
                    include_str!("../examples/modules/scores.wtf").into(),
                ),
                (
                    "/notes/.wtf/modules/sports.wtf".into(),
                    include_str!("../examples/modules/sports.wtf").into(),
                ),
            ]
            .into(),
        )
        .unwrap(),
    )
}
fn workspace() -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(
            path().into(),
            Document::parse(format!("# Game\n{URL}:game\nMargin [game.margin].\n")),
        )]
        .into(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: modules(),
    }
}
fn labels(ws: &Workspace) -> Vec<String> {
    wtf::RequestContext::new(ws, now())
        .hints(
            path(),
            Range::new(Position::new(0, 0), Position::new(99, 0)),
        )
        .hints
        .into_iter()
        .filter_map(|h| match h.label {
            InlayHintLabel::String(s) => Some(s),
            _ => None,
        })
        .collect()
}
fn summary() -> serde_json::Value {
    serde_json::json!({
        "header": {
            "competitions": [{
                "status": {"type": {
                    "completed": true,
                    "state": "post",
                    "detail": "Final",
                    "shortDetail": "Final"
                }},
                "competitors": [
                    {"homeAway": "home", "winner": true, "score": "112",
                     "team": {"abbreviation": "BOS", "displayName": "Boston Celtics"}},
                    {"homeAway": "away", "winner": false, "score": "108",
                     "team": {"abbreviation": "LAL", "displayName": "Los Angeles Lakers"}}
                ]
            }]
        }
    })
}

#[test]
fn sports_links_match_only_supported_espn_game_pages() {
    let ws = workspace();
    let links = ws.link_features();
    for target in [
        URL,
        "https://espn.com/nfl/game/_/gameId/401671793",
        "https://www.espn.com/soccer/eng.1/game/_/gameId/704321",
    ] {
        assert!(
            links
                .presentation(target, &ws.cache, now().to_utc())
                .is_some(),
            "{target}"
        );
    }
    for target in [
        "https://www.espn.com/nba/standings",
        "https://www.espn.com/nba/game/_/gameId/not-a-number",
        "https://www.espn.com/quidditch/game/_/gameId/1",
        "https://www.espn.com/soccer/game/_/gameId/704321",
        "https://www.espn.com.evil/nba/game/_/gameId/401584896",
    ] {
        assert!(
            links
                .presentation(target, &ws.cache, now().to_utc())
                .is_none(),
            "{target}"
        );
    }
}

#[test]
fn refresh_requests_the_espn_summary_endpoint_without_running_it() {
    let ws = workspace();
    let request = ws.link_features().refresh_request(URL).unwrap();
    assert_eq!(request.title, "⟳ score");
    assert_eq!(request.program, "curl");
    assert_eq!(
        request.args,
        [
            "-sSL",
            "https://site.api.espn.com/apis/site/v2/sports/basketball/nba/summary?event=401584896"
        ]
    );
    let soccer = ws
        .link_features()
        .refresh_request("https://www.espn.com/soccer/eng.1/game/_/gameId/704321")
        .unwrap();
    assert_eq!(
        soccer.args[1],
        "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/summary?event=704321"
    );
}

#[test]
fn decoded_scores_drive_the_inlay_label_hover_and_properties() {
    let mut ws = workspace();
    assert!(labels(&ws).iter().any(|s| s == "◌ score (refresh)"));
    assert!(
        Engine::at(&ws, now())
            .eval(path(), "game.margin")
            .unwrap_err()
            .contains("No cached score for this game; refresh the link first")
    );
    let metadata = ws
        .link_features()
        .decode_refresh(URL, &summary(), now().to_utc())
        .unwrap();
    assert_eq!(metadata.title, "LAL @ BOS");
    assert_eq!(metadata.state, "final");
    let data = metadata.data.clone().unwrap();
    assert_eq!(data["home"], "BOS");
    assert_eq!(data["away_score"], 108);
    assert_eq!(data["winner"], "home");
    assert_eq!(data["completed"], true);
    ws.cache.insert(URL.into(), metadata);
    assert!(
        labels(&ws)
            .iter()
            .any(|s| s == "✓ BOS 112 – LAL 108 · Final"),
        "{:?}",
        labels(&ws)
    );
    let mut names = ws.link_features().property_names(URL);
    names.sort();
    assert_eq!(
        names,
        [
            "away",
            "away_score",
            "completed",
            "home",
            "home_score",
            "margin",
            "status",
            "winner"
        ]
    );
    assert_eq!(
        Engine::at(&ws, now()).eval(path(), "game.margin").unwrap(),
        Value::Number(4.0)
    );
    assert_eq!(
        Engine::at(&ws, now()).eval(path(), "game.winner").unwrap(),
        Value::Text("home".into())
    );
    assert!(
        ws.link_features()
            .presentation(URL, &ws.cache, now().to_utc())
            .unwrap()
            .hover
            .unwrap()
            .contains("Fetched 2026-09-18 12:00 UTC")
    );
}

#[test]
fn live_and_scheduled_games_use_their_own_markers() {
    let ws = workspace();
    let mut live = summary();
    live["header"]["competitions"][0]["status"]["type"] =
        serde_json::json!({"completed": false, "state": "in", "shortDetail": "8:12 - 3rd"});
    live["header"]["competitions"][0]["competitors"][0]["winner"] = serde_json::json!(false);
    let decoded = ws
        .link_features()
        .decode_refresh(URL, &live, now().to_utc())
        .unwrap();
    assert_eq!(decoded.state, "live");
    assert_eq!(decoded.data.unwrap()["winner"], serde_json::Value::Null);
    let mut upcoming = summary();
    upcoming["header"]["competitions"][0]["status"]["type"] =
        serde_json::json!({"completed": false, "state": "pre", "shortDetail": "Wed, 7:30 PM"});
    upcoming["header"]["competitions"][0]["competitors"][0]["score"] = serde_json::Value::Null;
    upcoming["header"]["competitions"][0]["competitors"][0]["winner"] = serde_json::json!(false);
    upcoming["header"]["competitions"][0]["competitors"][1]["score"] = serde_json::Value::Null;
    let decoded = ws
        .link_features()
        .decode_refresh(URL, &upcoming, now().to_utc())
        .unwrap();
    assert_eq!(decoded.state, "scheduled");
    let data = decoded.data.clone().unwrap();
    assert_eq!(data["home_score"], serde_json::Value::Null);
    let mut ws = workspace();
    ws.cache.insert(URL.into(), decoded);
    assert!(
        labels(&ws).iter().any(|s| s == "LAL @ BOS · Wed, 7:30 PM"),
        "{:?}",
        labels(&ws)
    );
}

#[test]
fn the_scores_library_resolves_results_for_notes() {
    let ws = workspace();
    let registry = &ws.modules;
    for (home, away, winner, margin) in [
        (112.0, 108.0, "home", 4.0),
        (99.0, 101.0, "away", 2.0),
        (2.0, 2.0, "draw", 0.0),
    ] {
        assert_eq!(
            registry
                .call(
                    "scores",
                    "resolve",
                    vec![Value::Number(home), Value::Number(away)],
                    now()
                )
                .unwrap(),
            Value::Text(winner.into())
        );
        assert_eq!(
            registry
                .call(
                    "scores",
                    "margin",
                    vec![Value::Number(home), Value::Number(away)],
                    now()
                )
                .unwrap(),
            Value::Number(margin)
        );
    }
    let mut ws = workspace();
    ws.documents.insert(
        path().into(),
        Document::parse("won := import(\"scores\").resolve(112, 108)\n".into()),
    );
    assert_eq!(
        Engine::at(&ws, now()).named(path(), "won").unwrap(),
        Value::Text("home".into())
    );
}
