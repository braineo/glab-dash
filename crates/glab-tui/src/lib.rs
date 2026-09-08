//! The glab-dash terminal UI: the [`app`] state machine that owns all view and
//! overlay state, the [`cmd`] side-effect descriptors it emits, the
//! [`keybindings`] registry that drives dispatch, help and status hints alike,
//! the [`ui`] layer that paints it, and the [`run`] event loop that drives the
//! whole cycle in the terminal.
//!
//! The domain model, filters and sorts live in `glab-core`; the user's
//! `config.toml` is read into those types by `glab-config`; every HTTP
//! round-trip to GitLab lives in `glab-api`; the SQLite cache lives in
//! `glab-store`.

pub mod app;
pub mod cmd;
pub mod keybindings;
pub mod run;
pub mod ui;
