use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Item, ItemKind, Milestone, User};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStats {
    pub additions: u64,
    pub deletions: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRef {
    #[serde(deserialize_with = "crate::de::lower_opt")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRequest {
    pub id: String,
    pub iid: String,
    pub title: String,
    pub state: String,
    pub draft: bool,
    pub author: Option<User>,
    #[serde(deserialize_with = "crate::de::nodes")]
    pub assignees: Vec<User>,
    #[serde(deserialize_with = "crate::de::nodes")]
    pub reviewers: Vec<User>,
    #[serde(deserialize_with = "crate::de::label_titles")]
    pub labels: Vec<String>,
    pub milestone: Option<Milestone>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub web_url: Option<String>,
    pub description: Option<String>,
    pub user_notes_count: Option<u64>,
    pub source_branch: String,
    pub target_branch: String,
    /// Full: `group/project!123`, never a bare `!123`.
    pub reference: String,
    pub diff_stats_summary: Option<DiffStats>,
    pub approved: Option<bool>,
    /// `mergeable` is the only value GitLab lets a merge through on.
    #[serde(deserialize_with = "crate::de::lower_opt")]
    pub detailed_merge_status: Option<String>,
    #[serde(deserialize_with = "crate::de::nodes")]
    pub approved_by: Vec<User>,
    pub head_pipeline: Option<PipelineRef>,
    pub resolvable_discussions_count: Option<u64>,
    pub resolved_discussions_count: Option<u64>,
}

impl MergeRequest {
    pub fn is_merged(&self) -> bool {
        self.state == "merged"
    }

    pub fn unresolved_threads(&self) -> u64 {
        self.resolvable_discussions_count
            .unwrap_or(0)
            .saturating_sub(self.resolved_discussions_count.unwrap_or(0))
    }

    pub fn notes_count(&self) -> u64 {
        self.user_notes_count.unwrap_or(0)
    }

    pub fn pipeline_status(&self) -> Option<&str> {
        self.head_pipeline.as_ref()?.status.as_deref()
    }

    pub fn diff_stats(&self) -> Option<&DiffStats> {
        self.diff_stats_summary.as_ref()
    }
}

impl Item for MergeRequest {
    fn kind(&self) -> ItemKind {
        ItemKind::MergeRequest
    }

    fn gid(&self) -> &str {
        &self.id
    }

    fn iid(&self) -> &str {
        &self.iid
    }

    fn reference(&self) -> &str {
        &self.reference
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn state(&self) -> &str {
        &self.state
    }

    fn web_url(&self) -> Option<&str> {
        self.web_url.as_deref()
    }

    fn labels(&self) -> &[String] {
        &self.labels
    }

    fn assignees(&self) -> &[User] {
        &self.assignees
    }
}
