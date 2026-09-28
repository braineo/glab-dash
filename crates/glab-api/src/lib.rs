//! Every HTTP round-trip glab-dash makes.  GraphQL for the list queries and
//! work-item mutations, REST v4 for what GraphQL does not cover.
//!
//! The client holds no configuration: which namespaces to ask about, which
//! results to keep and how to sequence a refresh are the caller's.

pub mod client;
pub mod discussions;
pub mod issues;
pub mod merge_requests;
pub mod meta;
pub mod planning;
pub mod related;
pub mod wire;

pub use client::GitLabClient;
pub use issues::IssueState;
pub use merge_requests::MrState;
