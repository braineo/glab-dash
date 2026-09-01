use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// user id REST API returns number but new GraphQL returns "gid://gitlab/User/{id}"
    #[serde(deserialize_with = "crate::de::user_id")]
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Iteration {
    /// GitLab GID, e.g. "gid://gitlab/Iteration/123". Kept as the GID because
    /// that is the form every mutation taking an iteration expects.
    pub id: String,
    /// Nullable in the GraphQL schema — iterations may have no title.
    pub title: Option<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub state: String,
}

/// A work-item status (`status { name category }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusValue {
    pub name: String,
    /// Status category from GitLab, e.g. "to_do", "in_progress", "done".
    pub category: Option<String>,
}

/// An issue, shaped as the root `issues` GraphQL query returns one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// The work item's global id (`ID!`), normalized to the `WorkItem` prefix
    /// by [`de::work_item_gid`](crate::de::work_item_gid) so that the same
    /// issue carries one id whether it arrived from `namespace.workItems` or
    /// the root `issues` query. This is the form mutations take, so it is
    /// passed straight through with no reconstruction.
    #[serde(deserialize_with = "crate::de::work_item_gid")]
    pub id: String,
    /// Internal ID (`iid: String!`), as GraphQL sends it.
    pub iid: String,
    pub title: String,
    pub state: String,
    pub author: Option<User>,
    #[serde(deserialize_with = "crate::de::nodes")]
    pub assignees: Vec<User>,
    #[serde(deserialize_with = "crate::de::label_titles")]
    pub labels: Vec<String>,
    pub milestone: Option<Milestone>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub web_url: String,
    pub description: Option<String>,
    pub user_notes_count: u64,
    /// `reference(full: true)` — `group/project#123`.
    pub reference: String,
    /// Custom workflow status, from GitLab's work-item status system.
    pub status: Option<StatusValue>,
    pub iteration: Option<Iteration>,
    pub weight: Option<u32>,
}

impl Issue {
    /// The project the issue lives in, e.g. `group/project`, taken from the
    /// full reference.
    pub fn project_path(&self) -> &str {
        project_from_reference(&self.reference)
    }

    /// The custom workflow status name, if the issue has one.
    pub fn status_name(&self) -> Option<&str> {
        self.status.as_ref().map(|s| s.name.as_str())
    }

    /// The custom workflow status category, if the issue has one.
    pub fn status_category(&self) -> Option<&str> {
        self.status.as_ref()?.category.as_deref()
    }
}

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
    #[serde(deserialize_with = "crate::de::nodes")]
    pub approved_by: Vec<User>,
    pub head_pipeline: Option<PipelineRef>,
    /// `resolvableDiscussionsCount: Int` — nullable.
    pub resolvable_discussions_count: Option<u64>,
    /// `resolvedDiscussionsCount: Int` — nullable.
    pub resolved_discussions_count: Option<u64>,
}

impl MergeRequest {
    /// The project the merge request lives in, e.g. `group/project`, taken
    /// from the full reference.
    pub fn project_path(&self) -> &str {
        project_from_reference(&self.reference)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub body: String,
    pub author: User,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discussion {
    pub id: String,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLabel {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// A work item status from GitLab's custom status system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItemStatus {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub position: Option<i32>,
    /// Status category from GitLab (e.g. "active", "done", "canceled").
    #[serde(default)]
    pub category: Option<String>,
}

/// Strip the `#123` / `!45` suffix off a full reference, leaving the project
/// path. Full refs look like `group/project#123` or `group/sub/project!45`.
fn project_from_reference(full_ref: &str) -> &str {
    match full_ref.rfind(['#', '!']) {
        Some(idx) => &full_ref[..idx],
        None => full_ref,
    }
}

#[cfg(test)]
mod tests {
    use super::project_from_reference;

    #[test]
    fn strips_the_iid_suffix_from_a_full_reference() {
        assert_eq!(project_from_reference("group/project#123"), "group/project");
        assert_eq!(
            project_from_reference("group/sub/project!45"),
            "group/sub/project"
        );
    }

    #[test]
    fn a_reference_without_an_iid_is_already_a_project_path() {
        assert_eq!(project_from_reference("group/project"), "group/project");
    }
}
