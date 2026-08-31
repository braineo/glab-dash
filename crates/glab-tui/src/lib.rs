//! The glab-dash terminal UI: the [`app`] state machine that owns all view and
//! overlay state, the [`cmd`] side-effect descriptors it emits, the
//! [`keybindings`] registry that drives dispatch, help and status hints alike,
//! the [`ui`] layer that paints it, and the [`run`] event loop that drives the
//! whole cycle in the terminal.
//!
//! This crate is temporarily fat: [`config`], [`db`], [`gitlab`], [`filter`]
//! and [`sort`] still live here and are carved out into their own crates in
//! later steps.

pub mod app;
pub mod cmd;
pub mod config;
#[cfg(test)]
mod config_tests;
pub mod db;
pub mod filter;
pub mod gitlab;
pub mod keybindings;
pub mod run;
pub mod sort;
pub mod ui;
