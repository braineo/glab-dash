//! The glab-dash binary: initialize logging, then run the requested command.

mod debug;
mod logging;
mod onboarding;
#[cfg(test)]
mod onboarding_tests;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use glab_tui::app::App;
use glab_tui::config::Config;
use glab_tui::db::Db;
use glab_tui::gitlab::client::GitLabClient;
use tokio::sync::mpsc;

/// Ultra-fast TUI for managing GitLab issues and merge requests across teams.
#[derive(Parser)]
#[command(name = "glab-dash", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// What to run. With no subcommand, the dashboard opens.
#[derive(Subcommand)]
enum Command {
    /// Exercise the fetch paths without a terminal; results go to the log file.
    Debug,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let _log_guard = logging::init()?;
    match Cli::parse().command {
        Some(Command::Debug) => debug::run().await,
        None => run_dashboard().await,
    }
}

/// Load the config (running onboarding first when there is none), open the
/// cache, and hand a built [`App`] to the event loop.
async fn run_dashboard() -> Result<()> {
    let config = if onboarding::needs_onboarding() {
        onboarding::run_onboarding().await?
    } else {
        Config::load().context("Failed to load configuration")?
    };
    let client = GitLabClient::new(&config).context("Failed to create GitLab client")?;
    let db = Db::open().context("Failed to open database")?;

    let (async_tx, async_rx) = mpsc::unbounded_channel();
    let app = App::new(config, client, async_tx, db);

    glab_tui::run::run(app, async_rx).await
}
