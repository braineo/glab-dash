//! Notes and discussions, over REST.
//!
//! GitLab routes an issue's and a merge request's notes through the same
//! endpoints under a different collection segment, so [`Issuable`] names which
//! and the three operations are written once.

use anyhow::Result;
use reqwest::Method;

use glab_core::domain::{Discussion, Note};

use crate::client::GitLabClient;

/// The two issuable kinds that carry notes and discussions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issuable {
    Issue,
    MergeRequest,
}

impl Issuable {
    /// The collection segment naming this kind in a REST route.
    fn segment(self) -> &'static str {
        match self {
            Issuable::Issue => "issues",
            Issuable::MergeRequest => "merge_requests",
        }
    }
}

impl GitLabClient {
    /// List the discussion threads on the issuable `iid` in `project`, oldest
    /// thread first.
    pub async fn list_discussions(
        &self,
        kind: Issuable,
        project: &str,
        iid: &str,
    ) -> Result<Vec<Discussion>> {
        let request = self
            .rest(
                Method::GET,
                &Self::issuable_path(kind, project, iid, "discussions"),
            )
            .query(&[("sort", "asc"), ("per_page", "100")]);
        Self::send(request).await
    }

    /// Post `body` as a new top-level note on the issuable `iid` in `project`.
    pub async fn create_note(
        &self,
        kind: Issuable,
        project: &str,
        iid: &str,
        body: &str,
    ) -> Result<Note> {
        let request = self
            .rest(
                Method::POST,
                &Self::issuable_path(kind, project, iid, "notes"),
            )
            .json(&serde_json::json!({ "body": body }));
        Self::send(request).await
    }

    /// Post `body` as a reply into the existing thread `discussion_id`.
    pub async fn reply_to_discussion(
        &self,
        kind: Issuable,
        project: &str,
        iid: &str,
        discussion_id: &str,
        body: &str,
    ) -> Result<Note> {
        let path = Self::issuable_path(
            kind,
            project,
            iid,
            &format!("discussions/{discussion_id}/notes"),
        );
        let request = self
            .rest(Method::POST, &path)
            .json(&serde_json::json!({ "body": body }));
        Self::send(request).await
    }

    /// The REST route for `tail` under the issuable `iid` in `project`.
    fn issuable_path(kind: Issuable, project: &str, iid: &str, tail: &str) -> String {
        format!(
            "/projects/{}/{}/{iid}/{tail}",
            Self::project_id(project),
            kind.segment(),
        )
    }
}
