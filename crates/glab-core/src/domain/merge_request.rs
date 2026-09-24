//! A merge request and the fields only a merge request carries: its branches,
//! its diff, its pipeline and its approvals.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Item, ItemKind, Milestone, User};

/// `diffStatsSummary` on a merge request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStats {
    pub additions: u64,
    pub deletions: u64,
    pub file_count: u64,
}

/// `headPipeline` on a merge request. GraphQL exposes only the status here,
/// as a `SCREAMING_CASE` enum that is lowercased on the way in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRef {
    #[serde(deserialize_with = "crate::de::lower_opt")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRequest {
    /// `id: ID!`
    pub id: String,
    /// `iid: String!`
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
    /// Nullable on a merge request, unlike on an issue.
    pub web_url: Option<String>,
    pub description: Option<String>,
    /// `userNotesCount: Int` — nullable.
    pub user_notes_count: Option<u64>,
    pub source_branch: String,
    pub target_branch: String,
    /// `reference(full: true)` — `group/project!123`, used to recover the
    /// project an externally-fetched merge request belongs to.
    pub reference: String,
    pub diff_stats_summary: Option<DiffStats>,
    pub approved: Option<bool>,
    /// `detailedMergeStatus` — why the merge button is or is not enabled, as a
    /// `SCREAMING_CASE` enum lowercased on the way in. `mergeable` is the one
    /// value that means GitLab would let the merge through: approval rules
    /// satisfied, threads resolved, no conflicts, pipeline not blocking.
    #[serde(deserialize_with = "crate::de::lower_opt")]
    pub detailed_merge_status: Option<String>,
    #[serde(deserialize_with = "crate::de::nodes")]
    pub approved_by: Vec<User>,
    pub head_pipeline: Option<PipelineRef>,
    /// `resolvableDiscussionsCount: Int` — nullable.
    pub resolvable_discussions_count: Option<u64>,
    /// `resolvedDiscussionsCount: Int` — nullable.
    pub resolved_discussions_count: Option<u64>,
}

impl MergeRequest {
    /// Whether it landed.
    pub fn is_merged(&self) -> bool {
        self.state == "merged"
    }

    /// Discussion threads still open, from the two counters GraphQL reports.
    /// Both are nullable; a missing counter reads as zero.
    pub fn unresolved_threads(&self) -> u64 {
        self.resolvable_discussions_count
            .unwrap_or(0)
            .saturating_sub(self.resolved_discussions_count.unwrap_or(0))
    }

    /// User notes on the merge request; the nullable counter reads as zero.
    pub fn notes_count(&self) -> u64 {
        self.user_notes_count.unwrap_or(0)
    }

    /// The head pipeline's status, if the merge request has a pipeline.
    pub fn pipeline_status(&self) -> Option<&str> {
        self.head_pipeline.as_ref()?.status.as_deref()
    }

    /// The merge request's diff stats, if GitLab reported them.
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
