//! Refreshing what a workspace reads from outside, only when a person asks:
//! running the program a link module requests for each resource, and fetching
//! every lookup the notes want. The command line and the language server both
//! refresh through here.
use chrono::{DateTime, FixedOffset};
use lang::eval::Workspace;
use lang::eval::link_features::{LinkFeatures, RefreshFormat, RefreshRequest};
use lang::eval::resources::Metadata;
use std::{collections::BTreeMap, path::Path};

/// Run the refresh program for a link and decode its output through the
/// module. `fetched_at` is the clock once the program returns; a frozen
/// `XMD_NOW` freezes it too.
pub async fn fetch_link(features: LinkFeatures<'_>, target: &str) -> Result<Metadata, String> {
    let request = features
        .refresh_request(target)
        .ok_or("This link feature does not support refresh")?;
    run_refresh(features, target, request).await
}

/// Run a refresh program the link's module already asked for.
async fn run_refresh(
    features: LinkFeatures<'_>,
    target: &str,
    request: RefreshRequest,
) -> Result<Metadata, String> {
    let mut command = tokio::process::Command::new(&request.program);
    command.args(&request.args).envs(request.env);
    let stdout = crate::io::output(
        &mut command,
        &format!("{} timed out", request.title),
        &request.program,
        &request.program,
    )
    .await?;
    // A feed is markup, so the host parses it into the records a module reads.
    let data = match request.format {
        RefreshFormat::Json => serde_json::from_slice(&stdout).map_err(|e| e.to_string())?,
        RefreshFormat::Feed => crate::feeds::parse(&String::from_utf8_lossy(&stdout))?,
    };
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
    let features = workspace.link_features();
    let requests: BTreeMap<_, _> = workspace
        .documents()
        .iter()
        .filter(|(path, _)| only.is_none_or(|only| *path == only))
        .flat_map(|(_, doc)| {
            doc.definitions
                .iter()
                .filter(|d| !d.expression)
                .map(|d| d.source.as_str())
                .chain(doc.links.iter().map(|l| l.target.as_str()))
        })
        .filter_map(|s| Some((s.to_owned(), features.refresh_request(s)?)))
        .collect();
    let mut errors = Vec::new();
    for (target, request) in requests {
        match run_refresh(workspace.link_features(), &target, request).await {
            Ok(metadata) => {
                workspace.store_link_status(target, metadata);
            }
            Err(e) => errors.push(format!("{target}: {e}")),
        }
    }
    errors.extend(crate::lookups::refresh(workspace, now, only).await);
    errors
}
