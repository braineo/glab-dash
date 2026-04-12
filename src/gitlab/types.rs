use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub name: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    pub web_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub id: u64,
    pub title: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Iteration {
    /// GitLab GID, e.g. "gid://gitlab/Iteration/123"
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: u64,
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub author: Option<User>,
    #[serde(default)]
    pub assignees: Vec<User>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub milestone: Option<Milestone>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub web_url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub user_notes_count: u64,
    // References to identify which project this belongs to
    #[serde(default)]
    pub references: Option<References>,
    // Custom workflow status (from GraphQL widgets, not in REST API)
    #[serde(default)]
    pub custom_status: Option<String>,
    #[serde(default)]
    pub custom_status_category: Option<String>,
    #[serde(default)]
    pub iteration: Option<Iteration>,
    #[serde(default)]
    pub weight: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct References {
    #[serde(rename = "full")]
    pub full_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: u64,
    pub status: String,
    #[serde(rename = "ref")]
    pub ref_name: Option<String>,
    pub web_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequest {
    pub id: u64,
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub author: Option<User>,
    #[serde(default)]
    pub assignees: Vec<User>,
    #[serde(default)]
    pub reviewers: Vec<User>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub milestone: Option<Milestone>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub web_url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub work_in_progress: bool,
    #[serde(default)]
    pub merge_status: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub head_pipeline: Option<Pipeline>,
    #[serde(default)]
    pub user_notes_count: u64,
    #[serde(default)]
    pub references: Option<References>,
    #[serde(default)]
    pub approved_by: Vec<ApprovalUser>,
    // ── Enrichment fields (populated via GraphQL, not REST list endpoints) ──
    #[serde(default)]
    pub diff_additions: Option<u64>,
    #[serde(default)]
    pub diff_deletions: Option<u64>,
    #[serde(default)]
    pub diff_file_count: Option<u64>,
    /// Whether the MR has been approved (from GraphQL `approved` field).
    #[serde(default)]
    pub approved: Option<bool>,
    /// Number of unresolved discussion threads.
    #[serde(default)]
    pub unresolved_threads: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalUser {
    pub user: User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub body: String,
    pub author: User,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discussion {
    pub id: String,
    #[serde(default)]
    pub individual_note: bool,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLabel {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequestApprovals {
    #[serde(default)]
    pub approved_by: Vec<ApprovalUser>,
}

/// A work item status from GitLab's custom status system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItemStatus {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon_name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub position: Option<i32>,
    /// Status category from GitLab (e.g. "active", "done", "canceled").
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedIssue {
    pub issue: Issue,
    pub project_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedMergeRequest {
    pub mr: MergeRequest,
    pub project_path: String,
}
