//! File-based tracing setup.

use anyhow::{Context, Result};

/// Initialize file-based tracing. Logs go to `~/.cache/glab-dash/glab-dash.log`
/// (or `$GLAB_DASH_LOG_DIR` if set). Level controlled by `GLAB_DASH_LOG` env
/// var (default: `info` with both crates at `debug`, e.g. override with
/// `glab_tui=trace,reqwest=info`).
///
/// Returns the `WorkerGuard` — must be kept alive for the duration of the
/// program so the background writer flushes on exit.
pub fn init() -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = std::env::var_os("GLAB_DASH_LOG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("glab-dash")))
        .context("Could not determine log directory")?;
    std::fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let file_appender = tracing_appender::rolling::never(&log_dir, "glab-dash.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter =
        tracing_subscriber::EnvFilter::try_from_env("GLAB_DASH_LOG").unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new("info,glab_dash=debug,glab_tui=debug")
        });

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
