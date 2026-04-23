mod app;
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

/// Non-interactive debug mode: exercise the fetch paths and log results.
/// Run with: cargo run -- --debug — output goes to the tracing log file.
async fn debug_fetch() -> Result<()> {
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
    let db = crate::db::Db::open().context("Failed to open database")?;
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

/// Initialize file-based tracing. Logs go to `~/.cache/glab-dash/glab-dash.log`
/// (or `$GLAB_DASH_LOG_DIR` if set). Level controlled by `GLAB_DASH_LOG` env
/// var (default: `info`, use e.g. `glab_dash=debug,reqwest=info`).
///
/// Returns the `WorkerGuard` — must be kept alive for the duration of the
/// program so the background writer flushes on exit.
fn init_tracing() -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = std::env::var_os("GLAB_DASH_LOG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("glab-dash")))
        .context("Could not determine log directory")?;
    std::fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let file_appender = tracing_appender::rolling::never(&log_dir, "glab-dash.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_env("GLAB_DASH_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,glab_dash=debug"));

    // Local-time timestamps via chrono. ANSI colors are kept in the log file:
    // `tail -f` and `less -R` render them; plain `cat` shows escape codes but
    // that's rare for log inspection. Disable with `GLAB_DASH_LOG_NO_COLOR=1`.
    let ansi = std::env::var_os("GLAB_DASH_LOG_NO_COLOR").is_none();
    let timer =
        tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f".to_string());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(ansi)
        .with_timer(timer)
        .with_target(true)
        .init();

    tracing::info!(log_dir = %log_dir.display(), "tracing initialized");
    Ok(guard)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let _log_guard = init_tracing()?;

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
    let refresh_interval = Duration::from_secs(app.ctx.config.refresh_interval_secs);
    let mut refresh_timer = tokio::time::interval(refresh_interval);
    refresh_timer.tick().await; // consume the immediate first tick

    // Load cache for instant startup, then fetch fresh data in background
    app.load_from_db();
    app.ui.loading = true;
    app.ui.fetch_started_at = Some(app::App::now_millis());
    app.fetch_all();

    // Main loop — event-driven rendering with drain-before-paint.
    // Block on select! for the first event, then drain all pending events
    // before rendering once.  This gives immediate visual feedback while
    // coalescing bursts (e.g. held-key scrolling) into a single paint.
    loop {
        if app.ui.needs_redraw {
            terminal.draw(|frame| app.render(frame))?;
            app.ui.needs_redraw = false;
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
                        app.ui.needs_redraw = true;
                    }
                    _ => {}
                }
            }
            Some(msg) = async_rx.recv() => {
                app.process_async_msg(msg);
                app.ui.needs_redraw = true;
            }
            _ = refresh_timer.tick() => {
                app.ui.fetch_started_at = Some(app::App::now_millis());
                app.fetch_all();
                app.ui.needs_redraw = true;
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
            app.ui.needs_redraw = true;
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
