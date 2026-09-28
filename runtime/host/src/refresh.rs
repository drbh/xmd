//! Refreshing what a workspace reads from outside, only when a person asks:
//! running the program a link module requests for each resource, and fetching
//! every lookup the notes want. The command line and the language server both
//! refresh through here.
use chrono::{DateTime, FixedOffset};
use eval::Workspace;
use eval::link_features::{LinkFeatures, RefreshFormat};
use eval::resources::Metadata;
use std::{collections::BTreeSet, path::Path};

/// Run the refresh program for a link and decode its output through the module.
pub async fn fetch_link(features: LinkFeatures<'_>, target: &str) -> Result<Metadata, String> {
    let request = features
        .refresh_request(target)
        .ok_or("This link feature does not support refresh")?;
    let mut command = tokio::process::Command::new(&request.program);
    command
        .args(&request.args)
        .envs(request.env)
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(20), command.output())
        .await
        .map_err(|_| format!("{} timed out", request.title))?
        .map_err(|e| format!("Cannot run {}: {e}", request.program))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            request.program,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    // A feed is markup, so the host parses it into the records a module reads.
    let data = match request.format {
        RefreshFormat::Json => serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?,
        RefreshFormat::Feed => crate::feeds::json(&crate::feeds::parse(&String::from_utf8_lossy(
            &output.stdout,
        ))?),
    };
    // The request clock, so a frozen `XMD_NOW` also freezes `fetched_at`.
    features
        .decode_refresh(target, &data, crate::now().to_utc())
        .map_err(|e| e.to_string())
}

/// Refresh every refreshable resource and every wanted lookup in memory,
/// optionally for one note only; returns the errors. The caller decides when
/// to save the caches, since an editor first checks its modules did not change.
pub async fn refresh_workspace(
    workspace: &mut Workspace,
    now: DateTime<FixedOffset>,
    only: Option<&Path>,
) -> Vec<String> {
    let targets: BTreeSet<_> = workspace
        .documents
        .iter()
        .filter(|(path, _)| only.is_none_or(|only| *path == only))
        .flat_map(|(_, doc)| {
            doc.definitions
                .iter()
                .filter(|d| !d.expression)
                .map(|d| d.source.as_str())
                .chain(doc.links.iter().map(|l| l.target.as_str()))
        })
        .filter(|s| workspace.link_features().refresh_request(s).is_some())
        .map(str::to_owned)
        .collect();
    let mut errors = Vec::new();
    for target in targets {
        match fetch_link(workspace.link_features(), &target).await {
            Ok(metadata) => {
                workspace.cache.insert(target, metadata);
            }
            Err(e) => errors.push(format!("{target}: {e}")),
        }
    }
    errors.extend(crate::lookups_impl::refresh(workspace, now, only).await);
    errors
}
