//! The domain layer.  No I/O: this crate knows nothing about the terminal, the
//! GitLab API or SQLite.

pub mod comment_filter;
pub mod de;
pub mod domain;
pub mod filter;
pub mod kanban;
pub mod label;
pub mod sort;
pub mod team;
