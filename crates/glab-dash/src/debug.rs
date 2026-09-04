//! Non-interactive debug mode: exercise the fetch paths and log results.

use anyhow::{Context, Result};
use glab_api::{GitLabClient, IssueState, MrState};
use glab_store::Db;
use glab_tui::app::App;
use glab_tui::config::Config;
use tokio::sync::mpsc;

/// Exercise the fetch paths and log results. Output goes to the tracing log
/// file, not the terminal.
pub async fn run() -> Result<()> {
    let config = Config::load().context("Failed to load configuration")?;
    let client = GitLabClient::new(&config.gitlab_url, &config.token)
        .context("Failed to create GitLab client")?;
    let members = config.team_members(0);

    tracing::info!(
        projects = %config.tracking_projects.join(", "),
        "debug: fetching tracking issues"
    );
    match client
        .list_namespace_issues(&config.tracking_projects, Some(IssueState::Opened), None)
        .await
    {
        Ok(issues) => tracing::info!(count = issues.len(), "debug: tracking issues ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: tracking issues ✗"),
    }

    tracing::info!(members = members.len(), "debug: fetching assigned issues");
    match client
        .list_assigned_issues(&members, Some(IssueState::Opened), None)
        .await
    {
        Ok(issues) => tracing::info!(count = issues.len(), "debug: assigned issues ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: assigned issues ✗"),
    }

    tracing::info!("debug: fetching tracking MRs");
    match client
        .list_project_mrs(&config.tracking_projects, Some(MrState::Opened), None)
        .await
    {
        Ok(mrs) => tracing::info!(count = mrs.len(), "debug: tracking MRs ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: tracking MRs ✗"),
    }

    tracing::info!(members = members.len(), "debug: fetching external MRs");
    match client
        .list_user_mrs(&members, Some(MrState::Opened), None)
        .await
    {
        Ok(mrs) => tracing::info!(count = mrs.len(), "debug: external MRs ✓"),
        Err(e) => tracing::error!(error = ?e, "debug: external MRs ✗"),
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
    let projects = config.tracking_projects.clone();
    let mut app = App::new(config, client, async_tx, db);
    let tracking = app
        .ctx
        .client
        .list_namespace_issues(&projects, Some(IssueState::Opened), None)
        .await?;
    let assigned = app
        .ctx
        .client
        .list_assigned_issues(&members, Some(IssueState::Opened), None)
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
