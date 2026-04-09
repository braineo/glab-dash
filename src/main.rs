mod app;
mod config;
#[cfg(test)]
mod config_tests;
mod event;
mod filter;
mod gitlab;
mod onboarding;
#[cfg(test)]
mod onboarding_tests;
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

#[tokio::main]
async fn main() -> Result<()> {
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

    // Initial data fetch
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
                    Event::Resize(_, _) => {
                        // Terminal auto-handles resize on next draw
                    }
                    Event::Tick => {
                        // Could add periodic refresh here
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
