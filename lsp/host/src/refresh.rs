//! Running the refresh a link module requests: its program, with its arguments
//! and environment, only when a person asks for a refresh.
use eval::link_features::{LinkFeatures, RefreshFormat};
use eval::resources::Metadata;

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
        .decode_refresh(target, &data, eval::clock::now().to_utc())
        .map_err(|e| e.to_string())
}
