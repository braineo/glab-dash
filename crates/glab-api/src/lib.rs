//! The GitLab API client: every HTTP round-trip glab-dash makes, and nothing
//! else.
//!
//! GitLab answers on two APIs and this crate speaks both — GraphQL for the list
//! queries and work-item mutations, REST v4 for the endpoints GraphQL does not
//! cover (notes, discussions, approvals, merges, labels, user search). Wire
//! shapes stay inside: a caller passes plain arguments and gets
//! [`glab_core::domain`] types back, so the `Gql*` structs, the query documents
//! and GitLab's own error envelopes never reach the layers above.
//!
//! The client owns one round-trip per method, plus the cursor pagination a
//! single connection needs. It holds no configuration and knows nothing of
//! tracking projects or team membership: which namespaces to ask about, which
//! results to keep, and how to sequence a refresh are the caller's to decide.

pub mod client;
pub mod discussions;
pub mod issues;
pub mod merge_requests;
pub mod meta;
pub mod planning;
pub mod query;
pub mod wire;

pub use client::GitLabClient;
pub use discussions::Issuable;
pub use issues::IssueState;
pub use merge_requests::MrState;
