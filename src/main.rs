mod app;
mod cache;
mod config;
#[cfg(test)]
mod config_tests;
mod event;
mod filter;
mod gitlab;
mod onboarding;
#[cfg(test)]
mod onboarding_tests;
mod sort;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::app::App;
use crate::config::Config;
use crate::event::{Event, EventHandler};
use crate::gitlab::client::GitLabClient;

/// Non-interactive debug mode: fetch issues via GraphQL and print them.
/// Run with: cargo run -- --debug
async fn debug_fetch() -> Result<()> {
    let config = Config::load().context("Failed to load configuration")?;
    let client = GitLabClient::new(&config).context("Failed to create GitLab client")?;
    let members = config.team_members(0);

    println!(
        "Fetching tracking issues from {} ...",
        config.tracking_projects.join(", ")
    );
    match client.fetch_tracking_issues(Some("opened"), None).await {
        Ok(issues) => {
            println!("  OK: {} tracking issues", issues.len());
            for item in issues.iter().take(3) {
                println!(
                    "  #{} [{}] {:?} {}",
                    item.issue.iid,
                    item.issue.state,
                    item.issue.custom_status,
                    item.issue.title.chars().take(40).collect::<String>(),
                );
            }
        }
        Err(e) => println!("  FAIL: {e:#}"),
    }

    println!("Fetching assigned issues for {} members ...", members.len());
    match client
        .fetch_assigned_issues(&members, Some("opened"), None)
        .await
    {
        Ok(issues) => {
            println!("  OK: {} assigned issues", issues.len());
            for item in issues.iter().take(3) {
                println!(
                    "  #{} [{}] {:?} {} ({})",
                    item.issue.iid,
                    item.issue.state,
                    item.issue.custom_status,
                    item.issue.title.chars().take(40).collect::<String>(),
                    item.project_path
                );
            }
        }
        Err(e) => println!("  FAIL: {e:#}"),
    }

    println!("Fetching work item statuses ...");
    match client
        .fetch_work_item_statuses(config.primary_tracking_project())
        .await
    {
        Ok(statuses) => {
            println!("  OK: {} statuses", statuses.len());
            for s in &statuses {
                println!("  {} ({})", s.name, s.id);
            }
        }
        Err(e) => println!("  FAIL: {e:#}"),
    }

    // Simulate what the app does: store issues, refilter, check count
    println!("\nSimulating app flow ...");
    let (async_tx, _async_rx) = mpsc::unbounded_channel();
    let mut app = App::new(config, client, async_tx);
    let tracking = app
        .client
        .fetch_tracking_issues(Some("opened"), None)
        .await?;
    let assigned = app
        .client
        .fetch_assigned_issues(&members, Some("opened"), None)
        .await?;
    println!("  tracking={} assigned={}", tracking.len(), assigned.len());
    app.issues = tracking;
    app.issues.extend(assigned);
    app.refilter_issues();
    println!(
        "  total issues={} filtered={}",
        app.issues.len(),
        app.issue_list_state.filtered_indices.len()
    );
    // Check a few filtered issues
    for i in app.issue_list_state.filtered_indices.iter().take(3) {
        let item = &app.issues[*i];
        println!(
            "  #{} [{}] {:?} {}",
            item.issue.iid,
            item.issue.state,
            item.issue.custom_status,
            item.issue.title.chars().take(40).collect::<String>()
        );
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--debug") {
        return debug_fetch().await;
    }

    let config = if onboarding::needs_onboarding() {
        onboarding::run_onboarding().await?
    } else {
        Config::load().context("Failed to load configuration")?
    };
    let client = GitLabClient::new(&config).context("Failed to create GitLab client")?;

    // Async message channel
    let (async_tx, mut async_rx) = mpsc::unbounded_channel();

    let mut app = App::new(config, client, async_tx);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Start event handler
    let mut events = EventHandler::new(Duration::from_millis(100));

    // Load cache for instant startup, then fetch fresh data in background
    app.load_cache();
    app.loading = true;
    app.fetch_all();

    // Main loop
    loop {
        // Render
        terminal.draw(|frame| app.render(frame))?;

        // Handle events
        tokio::select! {
            Some(event) = events.next() => {
                match event {
                    Event::Key(key) => {
                        // Only handle key press events (ignore release/repeat)
                        if key.kind == crossterm::event::KeyEventKind::Press
                            && app.handle_key(key) {
                                break; // quit
                            }
                    }
                    Event::Resize(_, _) | Event::Tick => {
                        // Terminal auto-handles resize on next draw; tick is a no-op
                    }
                }
            }
            Some(msg) = async_rx.recv() => {
                app.handle_async_msg(msg);
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
