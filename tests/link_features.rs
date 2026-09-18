use chrono::{DateTime, FixedOffset};
use lsp_types::{InlayHintLabel, Position, Range, Url};
use std::path::Path;
use wtf::{
    document::Document,
    engine::{Engine, Value},
    inlays,
    link_features::{LinkContext, LinkFeature, LinkFeatures},
    resources::{Cache, Metadata, Resource},
    workspace::Workspace,
};

const TARGET: &str = "https://builds.example/runs/42";
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/work.wtf")
}
fn workspace(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().into(), Document::parse(source.into()))].into(),
        cache: Cache::new(),
        lookups: Default::default(),
        plugins: Default::default(),
    }
}
fn all() -> Range {
    Range::new(Position::new(0, 0), Position::new(u32::MAX, 0))
}
fn label(hint: &lsp_types::InlayHint) -> &str {
    let InlayHintLabel::String(label) = &hint.label else {
        panic!("expected text")
    };
    label
}

// A static link provider needs exactly two methods and no document/host plumbing.
struct Documentation;
impl LinkFeature for Documentation {
    fn matches(&self, url: &Url) -> bool {
        url.scheme() == "https" && url.host_str() == Some("docs.example")
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        format!("docs · {}", ctx.url.path().trim_start_matches('/'))
    }
}

// A provider can also supply cached properties or clock-dependent labels.
struct Build;
impl LinkFeature for Build {
    fn matches(&self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.host_str() == Some("builds.example")
            && url.path().starts_with("/runs/")
    }
    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        format!(
            "build {} · {}",
            ctx.url.path().rsplit('/').next().unwrap(),
            ctx.cached.map(|m| m.state.as_str()).unwrap_or("unknown")
        )
    }
    fn hover(&self, ctx: &LinkContext<'_>) -> Option<String> {
        Some(format!("Build status at {}", ctx.now.to_rfc3339()))
    }
    fn time_dependent(&self, _: &LinkContext<'_>) -> bool {
        true
    }
    fn property_names(&self, _: &Url) -> Vec<String> {
        ["state", "observed"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
    fn property(&self, ctx: &LinkContext<'_>, name: &str) -> Result<Value, String> {
        match name {
            "state" => ctx
                .cached
                .map(|m| Value::Text(m.state.clone()))
                .ok_or("Refresh build first".into()),
            "observed" => Ok(Value::DateTime(ctx.now.fixed_offset())),
            _ => Err(format!("Unknown build property '{name}'")),
        }
    }
}

#[test]
fn one_link_impl_covers_raw_markdown_named_alias_and_cross_note_references() {
    let source = "🦀 https://docs.example/intro\nSee [manual](https://docs.example/intro).\n[https://docs.example/intro]:guide\n[alias] := guide\nRead [guide], [alias], and [shared].\nOrdinary https://example.com/plain\n";
    let mut ws = workspace(source);
    ws.documents.insert(
        "/notes/other.wtf".into(),
        Document::parse("[https://docs.example/intro]:shared\n".into()),
    );
    let features = LinkFeatures::new(&[&Documentation]);
    let mut engine = Engine::at(&ws, now()).with_link_features(features);
    let result = inlays::collect(&mut engine, path(), all(), wtf::inlay_providers::BUILTINS);
    assert_eq!(result.hints.len(), 7, "{:?}", result.hints);
    assert!(result.hints.iter().all(|h| label(h) == "docs · intro"));
    assert_eq!(result.hints[0].position.character, 29); // crab is two UTF-16 units
    assert!(!result.time_dependent);
    assert!(result.hints.iter().all(|h| h.text_edits.is_none()));
    assert_eq!(ws.documents[path()].text, source);
    assert!(
        features
            .refresh_request("https://docs.example/intro")
            .is_none()
    );
    assert!(
        features
            .property_names("https://docs.example/intro")
            .is_empty()
    );
    assert!(
        features
            .property(
                "https://docs.example/intro",
                &ws.cache,
                now().to_utc(),
                "title"
            )
            .is_err()
    );
}

#[test]
fn provider_properties_and_tooltips_share_the_cache_and_request_clock() {
    let mut ws = workspace(&format!(
        "[{TARGET}]:build\nStatus [build.state].\nObserved [build.observed].\n[summary] := build.state\n"
    ));
    let features = LinkFeatures::new(&[&Build]);
    let mut engine = Engine::at(&ws, now()).with_link_features(features);
    assert_eq!(
        engine.eval(path(), "build.state").unwrap_err(),
        "Refresh build first"
    );
    ws.cache.insert(
        TARGET.into(),
        Metadata {
            title: "Release build".into(),
            state: "passed".into(),
            merged: None,
            checks: None,
            review: None,
            fetched_at: now().to_utc(),
            provider: None,
            data: None,
        },
    );
    let mut engine = Engine::at(&ws, now()).with_link_features(features);
    assert_eq!(
        engine.eval(path(), "build.state").unwrap(),
        Value::Text("passed".into())
    );
    assert_eq!(
        engine.eval(path(), "build.observed").unwrap(),
        Value::DateTime(now().to_utc().fixed_offset())
    );
    assert!(engine.time_dependent);
    assert_eq!(features.property_names(TARGET), ["state", "observed"]);
    assert!(
        engine
            .eval(path(), "build.merged")
            .unwrap_err()
            .contains("Unknown build property")
    );
    assert_eq!(
        engine.eval(path(), "build.url").unwrap(),
        Value::Text(TARGET.into())
    );
    let result = inlays::collect(&mut engine, path(), all(), wtf::inlay_providers::BUILTINS);
    assert!(result.time_dependent);
    assert_eq!(label(&result.hints[0]), "build 42 · passed");
    assert_eq!(label(&result.hints[1]), "passed");
    assert_eq!(label(&result.hints[3]), "= passed");
    let summary = serde_json::to_string(&result.hints[3].tooltip).unwrap();
    assert!(summary.contains("passed"));
    assert!(!summary.contains("Unknown resource property"));
    let tooltip = serde_json::to_string(&result.hints[0].tooltip).unwrap();
    assert!(tooltip.contains(TARGET));
    assert!(tooltip.contains("Build status at 2026-09-16T18:00:00+00:00"));
}

#[test]
fn registry_priority_and_unrecognized_resources_have_predictable_fallbacks() {
    struct Override;
    impl LinkFeature for Override {
        fn matches(&self, url: &Url) -> bool {
            Documentation.matches(url)
        }
        fn inlay(&self, _: &LinkContext<'_>) -> String {
            "override".into()
        }
    }
    let features = LinkFeatures::new(&[&Override, &Documentation]);
    let cache = Cache::new();
    assert_eq!(
        features
            .presentation("https://docs.example/a", &cache, now().to_utc())
            .unwrap()
            .label,
        "override"
    );
    for target in [
        "https://docs.example.evil/a",
        "http://docs.example/a",
        "https://",
        "./local.txt",
    ] {
        assert!(
            features
                .presentation(target, &cache, now().to_utc())
                .is_none(),
            "{target}"
        );
        assert!(features.refresh_request(target).is_none());
    }
    let resource = Resource {
        target: "./images/map.png".into(),
        origin: Some("/notes/trips/source.wtf".into()),
    };
    let view = resource.presentation(path(), &cache, now().to_utc(), features);
    assert_eq!(view.label, "image · open preview");
    assert!(!view.known_link);
    assert!(!view.time_dependent);
    assert!(view.hover.contains("file:///notes/trips/images/map.png"));
    assert!(view.hover.contains("![Preview]"));
}

#[test]
fn github_uses_the_shared_registry_for_badges_properties_actions_and_cache_age() {
    let url = "https://github.com/acme/app/pull/42";
    let mut ws = workspace(&format!("[{url}]:pr\nSee [pr] and {url}.\n"));
    assert!(!wtf::presentation::live_hints(&ws, path(), now()));
    ws.cache.insert(
        url.into(),
        wtf::github::metadata(
            "pull",
            &serde_json::json!({
                "title":"Ship it", "state":"MERGED", "statusCheckRollup":[{"conclusion":"SUCCESS"}]
            }),
            now().to_utc() - chrono::Duration::hours(2),
        )
        .unwrap(),
    );
    let mut engine = Engine::at(&ws, now());
    assert_eq!(engine.eval(path(), "pr.merged").unwrap(), Value::Bool(true));
    assert_eq!(
        engine.eval(path(), "pr.checks_passed").unwrap(),
        Value::Bool(true)
    );
    let resource = engine.eval(path(), "pr").unwrap();
    assert_eq!(
        wtf::intelligence::property_names(&resource),
        ["url", "title", "state", "merged", "checks_passed"]
    );
    let result = inlays::collect(&mut engine, path(), all(), wtf::inlay_providers::BUILTINS);
    assert_eq!(result.hints.len(), 3);
    assert!(
        result
            .hints
            .iter()
            .all(|h| label(h) == "✓ merged · ● checks · 2h ago")
    );
    assert!(result.time_dependent);
    assert!(wtf::presentation::live_hints(&ws, path(), now()));
    let commands = wtf::interaction::row_commands(&ws, path(), 1, now(), false);
    let refresh: Vec<_> = commands
        .iter()
        .filter(|c| c.command == "wtf.refreshResource")
        .collect();
    assert_eq!(refresh.len(), 1); // duplicate appearances on a row share a command
    assert_eq!(refresh[0].title, "Refresh GitHub status");
    let request = wtf::link_features::BUILTINS.refresh_request(url).unwrap();
    assert_eq!(request.program, "gh");
    assert_eq!(
        &request.args[..5],
        ["pr", "view", "42", "--repo", "acme/app"]
    );
}

#[cfg(all(feature = "native", unix))]
#[tokio::test]
async fn explicit_refresh_executes_a_provider_request_and_requires_decodable_success() {
    use chrono::Utc;
    use std::os::unix::fs::PermissionsExt;
    use wtf::link_features::RefreshRequest;
    struct Fixture {
        program: String,
        data: String,
    }
    impl LinkFeature for Fixture {
        fn matches(&self, url: &Url) -> bool {
            Build.matches(url)
        }
        fn inlay(&self, ctx: &LinkContext<'_>) -> String {
            Build.inlay(ctx)
        }
        fn refresh_request(&self, _: &Url) -> Option<RefreshRequest> {
            Some(RefreshRequest {
                title: "Refresh build".into(),
                program: self.program.clone(),
                args: vec![self.data.clone()],
                env: vec![("WTF_FIXTURE_READY".into(), "yes".into())],
            })
        }
        fn decode_refresh(
            &self,
            _: &Url,
            data: &serde_json::Value,
            now: DateTime<Utc>,
        ) -> Result<Metadata, String> {
            Ok(Metadata {
                title: data["title"].as_str().ok_or("Missing build title")?.into(),
                state: data["state"].as_str().ok_or("Missing build state")?.into(),
                merged: None,
                checks: None,
                review: None,
                fetched_at: now,
                provider: None,
                data: None,
            })
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("fake build service");
    std::fs::write(&program, "#!/bin/sh\n[ \"$WTF_FIXTURE_READY\" = yes ] || exit 7\nif [ \"$1\" = fail ]; then echo rejected >&2; exit 1; fi\nprintf '%s' \"$1\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let title = "release $(false); literal arguments";
    let mut fixture = Fixture {
        program: program.to_string_lossy().into(),
        data: serde_json::json!({"title":title,"state":"passed"}).to_string(),
    };
    let before = Utc::now();
    let metadata = LinkFeatures::new(&[&fixture]).fetch(TARGET).await.unwrap();
    assert_eq!(metadata.title, title);
    assert_eq!(metadata.state, "passed");
    assert!(metadata.fetched_at >= before);
    for (input, expected) in [
        ("fail", "rejected"),
        ("invalid", "expected value"),
        ("{}", "Missing build title"),
    ] {
        fixture.data = input.into();
        let error = LinkFeatures::new(&[&fixture])
            .fetch(TARGET)
            .await
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
    assert!(
        LinkFeatures::new(&[&Documentation])
            .fetch("https://docs.example/a")
            .await
            .unwrap_err()
            .contains("does not support refresh")
    );
}
