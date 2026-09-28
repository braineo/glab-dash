//! An issue's and a merge request's notes share these REST endpoints under a
//! different collection segment, which [`ItemKind`] names.

use anyhow::Result;
use reqwest::Method;

use glab_core::domain::ItemKind;
use glab_core::domain::{Discussion, Note};

use crate::client::{GitLabClient, item_path};

impl GitLabClient {
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

    /// Posts to `discussions`, not `notes`: a note posted to `notes` comes back
    /// as an individual note, which takes no replies and will not resolve.
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

    /// Works on an individual note too: GitLab turns it into a thread on the
    /// first reply.
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

    /// GitLab refuses a note the token's user may not edit.
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

    /// Merge requests only: GitLab has no route to resolve an issue's notes.
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
