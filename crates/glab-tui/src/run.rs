//! The terminal event loop.
//!
//! The imperative shell around [`App`]: it takes over the terminal, drives the
//! handle → reconcile → execute cycle from crossterm key events, async results
//! and the auto-refresh timer, and restores the terminal on the way out.

use std::io;
use std::time::Duration;

use anyhow::Result;
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

use crate::app::{App, AsyncMsg};

/// Take over the terminal and run `app` until a quit action ends it, draining
/// `async_rx` for results of the fetches and mutations it spawns. The terminal
/// is put into raw mode on an alternate screen for the duration and restored
/// before returning.
pub async fn run(mut app: App, mut async_rx: mpsc::UnboundedReceiver<AsyncMsg>) -> Result<()> {
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
    app.ui.fetch_started_at = Some(App::now_millis());
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
                app.ui.fetch_started_at = Some(App::now_millis());
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
