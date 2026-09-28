use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event as CEvent, EventStream,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::supports_keyboard_enhancement,
};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::app::{App, AsyncMsg};

pub async fn run(mut app: App, mut async_rx: mpsc::UnboundedReceiver<AsyncMsg>) -> Result<()> {
    // `try_init` gives raw mode, the alternate screen and a panic hook that
    // undoes both.  Mouse capture and the keyboard flags are ours to unset.
    // ponytail: the panic hook does not know about those two, so a panic leaves
    // mouse capture on.  Wrap them in the hook if that ever bites.
    let mut terminal = ratatui::try_init()?;
    let mut stdout = io::stdout();
    let has_keyboard_enhancement = supports_keyboard_enhancement().unwrap_or(false);
    execute!(stdout, EnableMouseCapture)?;
    if has_keyboard_enhancement {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }

    let mut event_stream = EventStream::new();

    let refresh_interval = Duration::from_secs(app.ctx.config.refresh_interval_secs);
    let mut refresh_timer = tokio::time::interval(refresh_interval);
    refresh_timer.tick().await; // consume the immediate first tick

    app.load_from_db();
    app.fetch_all();

    // Block on select! for the first event, then drain the rest before
    // rendering once, so a burst of held keys coalesces into one paint.
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
                app.fetch_all();
                app.ui.needs_redraw = true;
            }
        }

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

    if has_keyboard_enhancement {
        execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    execute!(terminal.backend_mut(), DisableMouseCapture)?;
    terminal.show_cursor()?;
    ratatui::try_restore()?;

    Ok(())
}
