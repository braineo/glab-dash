//! Non-interactive debug mode: exercise the fetch paths and log results.

use anyhow::{Context, Result};
use glab_tui::app::App;
use glab_tui::config::Config;
use glab_tui::db::Db;
use glab_tui::gitlab::client::GitLabClient;
use tokio::sync::mpsc;

/// Exercise the fetch paths and log results. Output goes to the tracing log
/// file, not the terminal.
pub async fn run() -> Result<()> {
    let config = Config::load().context("Failed to load configuration")?;
    let client = GitLabClient::new(&config).context("Failed to create GitLab client")?;
    let members = config.team_members(0);

    tracing::info!(
        projects = %config.tracking_projects.join(", "),
        "debug: fetching tracking issues"
    );
    match client.fetch_tracking_issues("opened", None).await {
        Ok(issues) => tracing::info!(count = issues.len(), "debug: tracking issues ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: tracking issues ✗"),
    }

    tracing::info!(members = members.len(), "debug: fetching assigned issues");
    match client.fetch_assigned_issues(&members, "opened", None).await {
        Ok(issues) => tracing::info!(count = issues.len(), "debug: assigned issues ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: assigned issues ✗"),
    }

    tracing::info!("debug: fetching work item statuses");
    match client
        .fetch_work_item_statuses(config.primary_tracking_project())
        .await
    {
        Ok(statuses) => tracing::info!(count = statuses.len(), "debug: statuses ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: statuses ✗"),
    }

    // Simulate what the app does: store issues, refilter, check count
    tracing::info!("debug: simulating app flow");
    let (async_tx, _async_rx) = mpsc::unbounded_channel();
    let db = Db::open().context("Failed to open database")?;
    let mut app = App::new(config, client, async_tx, db);
    let tracking = app.ctx.client.fetch_tracking_issues("opened", None).await?;
    let assigned = app
        .ctx
        .client
        .fetch_assigned_issues(&members, "opened", None)
        .await?;
    app.data.issues = tracking;
    app.data.issues.extend(assigned);
    app.refilter_issues();
    tracing::info!(
        total_issues = app.data.issues.len(),
        filtered = app.ui.views.issue_list.list.len(),
        "debug: app flow done"
    );
    Ok(())
}
