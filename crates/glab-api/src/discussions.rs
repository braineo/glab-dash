//! Notes and discussions, over REST.
//!
//! GitLab routes an issue's and a merge request's notes through the same
//! endpoints under a different collection segment, so [`Issuable`] names which
//! and the three operations are written once.

use anyhow::Result;
use reqwest::Method;

use glab_core::domain::{Discussion, Note};
use urlencoding::encode;

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
    ///
    /// Walks every page: a busy merge request has more than one page of
    /// threads, and stopping at the first drops the rest of the conversation.
    pub async fn list_discussions(
        &self,
        kind: Issuable,
        project: &str,
        iid: &str,
    ) -> Result<Vec<Discussion>> {
        const PER_PAGE: usize = 100;
        let path = Self::issuable_path(kind, project, iid, "discussions");
        let mut all: Vec<Discussion> = Vec::new();
        for page in 1.. {
            let request = self.rest(Method::GET, &path).query(&[
                ("sort", "asc"),
                ("per_page", &PER_PAGE.to_string()),
                ("page", &page.to_string()),
            ]);
            let batch: Vec<Discussion> = Self::send(request).await?;
            let done = batch.len() < PER_PAGE;
            all.extend(batch);
            if done {
                break;
            }
        }
        Ok(all)
    }

    /// Open a new thread on the issuable `iid` in `project` with `body` as its
    /// first note.
    ///
    /// Posts to `discussions` rather than `notes` on purpose.  A note posted to
    /// `notes` comes back as an individual note, which GitLab shows without a
    /// reply box and — on a merge request — will not resolve; a thread takes
    /// replies and resolves from the moment it exists.
    pub async fn create_thread(
        &self,
        kind: Issuable,
        project: &str,
        iid: &str,
        body: &str,
    ) -> Result<Discussion> {
        let request = self
            .rest(
                Method::POST,
                &Self::issuable_path(kind, project, iid, "discussions"),
            )
            .json(&serde_json::json!({ "body": body }));
        Self::send(request).await
    }

    /// Post `body` as a reply into the existing thread `discussion_id`.
    ///
    /// Works on an individual note too: GitLab turns a single comment into a
    /// thread when the first reply lands on it.
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

    /// Resolve or reopen the merge request thread `discussion_id`.
    ///
    /// Merge requests only: an issue's notes come back with `resolvable: false`
    /// and GitLab has no route to resolve them, so there is no `kind` to pass.
    pub async fn resolve_discussion(
        &self,
        project: &str,
        iid: &str,
        discussion_id: &str,
        resolved: bool,
    ) -> Result<Discussion> {
        let path = Self::issuable_path(
            Issuable::MergeRequest,
            project,
            iid,
            &format!("discussions/{discussion_id}"),
        );
        let request = self
            .rest(Method::PUT, &path)
            .json(&serde_json::json!({ "resolved": resolved }));
        Self::send(request).await
    }

    /// The REST route for `tail` under the issuable `iid` in `project`.
    fn issuable_path(kind: Issuable, project: &str, iid: &str, tail: &str) -> String {
        format!(
            "/projects/{}/{}/{iid}/{tail}",
            encode(project),
            kind.segment(),
        )
    }
}
