//! The glab-dash domain layer: the [`domain`] model of GitLab issues, merge
//! requests and the metadata around them, the [`label`] names they carry and
//! the [`kanban`] columns a board groups them into, the [`filter`] conditions
//! applied to those items client-side, and the [`sort`] specs that order them.
//!
//! This crate is the shared vocabulary every layer above speaks. It performs no
//! I/O and knows nothing about the terminal, the GitLab API, or SQLite.

pub mod de;
pub mod domain;
pub mod filter;
pub mod kanban;
pub mod label;
pub mod sort;
