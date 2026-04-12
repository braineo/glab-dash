mod app;
mod cache;
mod cmd;
mod config;
#[cfg(test)]
mod config_tests;
mod db;
mod filter;
mod gitlab;
mod keybindings;
mod onboarding;
#[cfg(test)]
mod onboarding_tests;
mod sort;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event as CEvent, EventStream,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
        supports_keyboard_enhancement,
    },
};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::app::App;
use crate::config::Config;
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
    let db = crate::db::Db::open().context("Failed to open database")?;
    let mut app = App::new(config, client, async_tx, db);
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
        app.issue_list_state.list.len()
    );
    // Check a few filtered issues
    for i in app.issue_list_state.list.indices.iter().take(3) {
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

#[tokio::main(flavor = "current_thread")]
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
    let db = crate::db::Db::open().context("Failed to open database")?;

    // Async message channel
    let (async_tx, mut async_rx) = mpsc::unbounded_channel();

    let mut app = App::new(config, client, async_tx, db);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let has_keyboard_enhancement = supports_keyboard_enhancement().unwrap_or(false);
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    if has_keyboard_enhancement {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Crossterm event stream — native tokio integration, no polling thread
    let mut event_stream = EventStream::new();

    // Auto-refresh timer using configured interval (default 60s)
    let refresh_interval = Duration::from_secs(app.config.refresh_interval_secs);
    let mut refresh_timer = tokio::time::interval(refresh_interval);
    refresh_timer.tick().await; // consume the immediate first tick

    // Load cache for instant startup, then fetch fresh data in background
    app.load_from_db();
    app.loading = true;
    app.fetch_all();

    // Main loop — event-driven rendering with drain-before-paint.
    // Block on select! for the first event, then drain all pending events
    // before rendering once.  This gives immediate visual feedback while
    // coalescing bursts (e.g. held-key scrolling) into a single paint.
    loop {
        if app.needs_redraw {
            terminal.draw(|frame| app.render(frame))?;
            app.needs_redraw = false;
        }

        tokio::select! {
            Some(Ok(event)) = event_stream.next() => {
                match event {
                    CEvent::Key(key)
                        if key.kind == crossterm::event::KeyEventKind::Press
                            && app.process_key(key) =>
                    {
                        break; // quit
                    }
                    CEvent::Resize(_, _) => {
                        app.needs_redraw = true;
                    }
                    _ => {}
                }
            }
            Some(msg) = async_rx.recv() => {
                app.process_async_msg(msg);
                app.needs_redraw = true;
            }
            _ = refresh_timer.tick() => {
                app.fetch_all();
                app.needs_redraw = true;
            }
        }

        // Drain pending events — coalesce into a single render pass
        let mut quit = false;
        while crossterm::event::poll(Duration::ZERO)? {
            if let CEvent::Key(key) = crossterm::event::read()?
                && key.kind == crossterm::event::KeyEventKind::Press
                && app.process_key(key)
            {
                quit = true;
                break;
            }
        }
        while let Ok(msg) = async_rx.try_recv() {
            app.process_async_msg(msg);
            app.needs_redraw = true;
        }
        if quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    if has_keyboard_enhancement {
        execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
