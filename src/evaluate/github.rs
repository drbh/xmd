//! GitHub link semantics. Hosts and generic inlay producers never match GitHub URLs.
use crate::{
    engine::Value,
    link_features::{LinkContext, LinkFeature, RefreshRequest},
    resources::{Metadata, ago},
};
use chrono::{DateTime, Utc};
use lsp_types::Url;

pub struct GitHub;
impl LinkFeature for GitHub {
    fn id(&self) -> &str {
        "github"
    }
    fn matches(&self, url: &Url) -> bool {
        parse_url(url).is_some()
    }
    fn inlay(&self, context: &LinkContext<'_>) -> String {
        if let Some(metadata) = context.cached {
            return metadata.badge(context.now);
        }
        let (_, kind, number) = parse_url(context.url).expect("registry matched a GitHub URL");
        format!(
            "{} {} · refresh for status",
            crate::glyphs::PENDING,
            match kind.as_str() {
                "pull" => format!("PR #{number}"),
                "issues" => format!("issue #{number}"),
                _ => format!("commit {}", &number[..number.len().min(7)]),
            }
        )
    }
    fn hover(&self, context: &LinkContext<'_>) -> Option<String> {
        Some(if let Some(m) = context.cached {
            format!(
                "**{}**\n\n{}\n\nLast refreshed: {}. Use **Refresh GitHub status** to update.",
                m.title,
                m.summary(),
                m.fetched_at.to_rfc3339()
            )
        } else {
            #[cfg(target_arch = "wasm32")]
            let message = "GitHub status refresh is available in the native WTF app, not this browser workspace.";
            #[cfg(not(target_arch = "wasm32"))]
            let message = "No cached status. Run `wtf refresh`, or use the Refresh GitHub status code action on this resource. Requires the GitHub CLI (`gh`).";
            message.into()
        })
    }
    fn time_dependent(&self, context: &LinkContext<'_>) -> bool {
        context.cached.is_some()
    }
    fn property_names(&self, url: &Url) -> Vec<String> {
        if parse_url(url).is_some_and(|(_, kind, _)| kind == "pull") {
            ["title", "state", "merged", "checks_passed"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            ["title", "state"].into_iter().map(str::to_owned).collect()
        }
    }
    fn property(&self, context: &LinkContext<'_>, name: &str) -> Result<Value, String> {
        if !self.property_names(context.url).iter().any(|p| p == name) {
            return Err(format!("Unknown resource property '{name}'"));
        }
        let m = context
            .cached
            .ok_or("No cached GitHub status; run wtf refresh")?;
        match name {
            "title" => Ok(Value::Text(m.title.clone())),
            "state" => Ok(Value::Text(m.state.clone())),
            "merged" => m
                .merged
                .map(Value::Bool)
                .ok_or("merged is only available on pull requests".into()),
            "checks_passed" => m
                .checks
                .as_ref()
                .map(|c| Value::Bool(c == "passing"))
                .ok_or("No checks reported".into()),
            _ => Err(format!("Unknown resource property '{name}'")),
        }
    }
    fn refresh_request(&self, url: &Url) -> Option<RefreshRequest> {
        let (repo, kind, id) = parse_url(url)?;
        let args = match kind.as_str() {
            "pull" => vec![
                "pr".into(),
                "view".into(),
                id,
                "--repo".into(),
                repo,
                "--json".into(),
                "title,state,isDraft,mergedAt,reviewDecision,statusCheckRollup".into(),
            ],
            "issues" => vec![
                "issue".into(),
                "view".into(),
                id,
                "--repo".into(),
                repo,
                "--json".into(),
                "title,state".into(),
            ],
            _ => vec!["api".into(), format!("repos/{repo}/commits/{id}")],
        };
        Some(RefreshRequest {
            title: "Refresh GitHub status".into(),
            program: "gh".into(),
            args,
            env: vec![
                ("GH_PROMPT_DISABLED".into(), "1".into()),
                ("GH_NO_UPDATE_NOTIFIER".into(), "1".into()),
            ],
        })
    }
    fn decode_refresh(
        &self,
        url: &Url,
        data: &serde_json::Value,
        now: DateTime<Utc>,
    ) -> Result<Metadata, String> {
        let (_, kind, _) = parse_url(url).ok_or("Not a supported GitHub link")?;
        metadata(&kind, data, now)
    }
}
impl Metadata {
    /// A compact, scannable status: `✓ merged · ● checks · ✓ approved · 2h ago`.
    /// Failing checks and requested changes shout in caps; stale caches say so.
    pub fn badge(&self, now: DateTime<Utc>) -> String {
        use crate::glyphs::*;
        let state = match self.state.as_str() {
            "merged" => format!("{DONE} merged"),
            "open" => format!("{OFF} open"),
            "draft" => format!("{HALF} draft"),
            "closed" => format!("{FAIL} closed"),
            other => other.to_string(),
        };
        let mut parts = vec![state];
        if let Some(checks) = &self.checks {
            parts.push(match checks.as_str() {
                "passing" => format!("{ON} checks"),
                "failing" => format!("{FAIL} checks FAILING"),
                _ => format!("{PENDING} checks pending"),
            });
        }
        if let Some(review) = &self.review
            && !review.is_empty()
        {
            parts.push(match review.as_str() {
                "APPROVED" => format!("{DONE} approved"),
                "CHANGES_REQUESTED" => format!("{FLAG} CHANGES REQUESTED"),
                "REVIEW_REQUIRED" => format!("{FLAG} review needed"),
                other => other.to_lowercase().replace('_', " "),
            });
        }
        let age = ago(self.fetched_at, now);
        parts.push(if (now - self.fetched_at).num_days() >= 7 {
            format!("{ALERT} stale · {age}")
        } else {
            age
        });
        parts.join(" · ")
    }
    pub fn summary(&self) -> String {
        let mut s = self.state.clone();
        if let Some(checks) = &self.checks {
            s.push_str(&format!(" · checks {checks}"));
        }
        if let Some(review) = &self.review
            && !review.is_empty()
        {
            s.push_str(&format!(" · {}", review.to_lowercase().replace('_', " ")));
        }
        // Always identify cached information; never suggest it is a fresh remote observation.
        s.push_str(&format!(
            " · cached {}",
            self.fetched_at.format("%Y-%m-%d %H:%M UTC")
        ));
        s
    }
}
pub fn parse(target: &str) -> Option<(String, String, String)> {
    let url = Url::parse(target).ok()?;
    parse_url(&url)
}
fn parse_url(url: &Url) -> Option<(String, String, String)> {
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return None;
    }
    let p: Vec<_> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if p.len() != 4 || !matches!(p[2], "pull" | "issues" | "commit") {
        return None;
    }
    if !p.iter().all(|s| {
        s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    }) {
        return None;
    }
    if p[2] != "commit" && !p[3].bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((format!("{}/{}", p[0], p[1]), p[2].into(), p[3].into()))
}
pub fn metadata(
    kind: &str,
    data: &serde_json::Value,
    now: DateTime<Utc>,
) -> Result<Metadata, String> {
    let title = if kind == "commit" {
        data["commit"]["message"].as_str()
    } else {
        data["title"].as_str()
    }
    .ok_or("Missing GitHub title")?
    .lines()
    .next()
    .unwrap_or("")
    .to_string();
    let merged =
        (kind == "pull").then(|| data["state"] == "MERGED" || data["mergedAt"].as_str().is_some());
    let state = if kind == "commit" {
        "commit".into()
    } else if merged == Some(true) {
        "merged".into()
    } else if data["isDraft"] == true {
        "draft".into()
    } else {
        data["state"].as_str().unwrap_or("unknown").to_lowercase()
    };
    let checks = data["statusCheckRollup"]
        .as_array()
        .filter(|a| !a.is_empty())
        .map(|checks| {
            let statuses: Vec<_> = checks
                .iter()
                .map(|c| {
                    c["conclusion"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .or(c["state"].as_str())
                        .unwrap_or("PENDING")
                })
                .collect();
            if statuses.iter().any(|s| {
                matches!(
                    *s,
                    "FAILURE"
                        | "ERROR"
                        | "CANCELLED"
                        | "TIMED_OUT"
                        | "ACTION_REQUIRED"
                        | "STARTUP_FAILURE"
                        | "STALE"
                )
            }) {
                "failing"
            } else if statuses
                .iter()
                .all(|s| matches!(*s, "SUCCESS" | "NEUTRAL" | "SKIPPED"))
            {
                "passing"
            } else {
                "pending"
            }
            .to_string()
        });
    Ok(Metadata {
        title,
        state,
        merged,
        checks,
        review: data["reviewDecision"].as_str().map(str::to_owned),
        fetched_at: now,
        provider: None,
        data: None,
    })
}
