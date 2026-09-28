use anyhow::{Context, Result};

/// Logs go to `~/.cache/glab-dash/glab-dash.log` (or `$GLAB_DASH_LOG_DIR`), at
/// the level `GLAB_DASH_LOG` sets.
///
/// The returned `WorkerGuard` must outlive the program: the background writer
/// flushes when it drops.
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

    // ANSI colors are kept in the log file; `GLAB_DASH_LOG_NO_COLOR=1` drops
    // them.
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
