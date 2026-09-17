//! Notes and discussions, over REST.
//!
//! GitLab routes an issue's and a merge request's notes through the same
//! endpoints under a different collection segment, so [`ItemKind`] names which
//! and the three operations are written once.

use anyhow::Result;
use reqwest::Method;

use glab_core::domain::ItemKind;
use glab_core::domain::{Discussion, Note};

use crate::client::{GitLabClient, item_path};

impl GitLabClient {
    /// List the discussion threads on the issuable `iid` in `project`, oldest
    /// thread first.
    ///
    /// Walks every page: a busy merge request has more than one page of
    /// threads, and stopping at the first drops the rest of the conversation.
    pub async fn list_discussions(
        &self,
        kind: ItemKind,
        project: &str,
        iid: &str,
    ) -> Result<Vec<Discussion>> {
        const PER_PAGE: usize = 100;
        let path = item_path(kind, project, iid, "discussions");
        let mut all: Vec<Discussion> = Vec::new();
        for page in 1.. {
            let request = self.rest(Method::GET, &path).query(&[
                ("sort", "asc"),
                ("per_page", &PER_PAGE.to_string()),
                ("page", &page.to_string()),
            ]);
            let batch: Vec<Discussion> = Self::fetch(request).await?;
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
        kind: ItemKind,
        project: &str,
        iid: &str,
        body: &str,
    ) -> Result<Discussion> {
        let request = self
            .rest(Method::POST, &item_path(kind, project, iid, "discussions"))
            .json(&serde_json::json!({ "body": body }));
        Self::send(request).await
    }

    /// Post `body` as a reply into the existing thread `discussion_id`.
    ///
    /// Works on an individual note too: GitLab turns a single comment into a
    /// thread when the first reply lands on it.
    pub async fn reply_to_discussion(
        &self,
        kind: ItemKind,
        project: &str,
        iid: &str,
        discussion_id: &str,
        body: &str,
    ) -> Result<Note> {
        let path = item_path(
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

    /// Rewrite the note `note_id` with `body`.
    ///
    /// Addressed through the flat `notes` collection rather than the thread it
    /// sits in: the note's id is enough, so a reply and a standalone comment
    /// take the same route.  GitLab refuses a note the token's user may not
    /// edit, which is what keeps this to the author and the project's
    /// maintainers.
    pub async fn update_note(
        &self,
        kind: ItemKind,
        project: &str,
        iid: &str,
        note_id: u64,
        body: &str,
    ) -> Result<Note> {
        let path = item_path(kind, project, iid, &format!("notes/{note_id}"));
        let request = self
            .rest(Method::PUT, &path)
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
        let path = item_path(
            ItemKind::MergeRequest,
            project,
            iid,
            &format!("discussions/{discussion_id}"),
        );
        let request = self
            .rest(Method::PUT, &path)
            .json(&serde_json::json!({ "resolved": resolved }));
        Self::send(request).await
    }
}
