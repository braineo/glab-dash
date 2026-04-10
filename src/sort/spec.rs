use std::cmp::Ordering;
use std::collections::HashMap;

use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};

use super::label_order::compare_by_label_scope;

#[derive(Debug, Clone, PartialEq)]
pub enum SortField {
    Iid,
    Title,
    UpdatedAt,
    CreatedAt,
    State,
    Author,
    Assignee,
    Label,
    Milestone,
    UserNotesCount,
    Project,
    // MR-only
    Pipeline,
    Draft,
}

impl SortField {
    pub fn all_issue() -> &'static [SortField] {
        &[
            SortField::Iid,
            SortField::Title,
            SortField::UpdatedAt,
            SortField::CreatedAt,
            SortField::State,
            SortField::Author,
            SortField::Assignee,
            SortField::Label,
            SortField::Milestone,
            SortField::UserNotesCount,
            SortField::Project,
        ]
    }

    pub fn all_mr() -> &'static [SortField] {
        &[
            SortField::Iid,
            SortField::Title,
            SortField::UpdatedAt,
            SortField::CreatedAt,
            SortField::State,
            SortField::Author,
            SortField::Assignee,
            SortField::Label,
            SortField::Milestone,
            SortField::UserNotesCount,
            SortField::Project,
            SortField::Pipeline,
            SortField::Draft,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            SortField::Iid => "iid",
            SortField::Title => "title",
            SortField::UpdatedAt => "updated_at",
            SortField::CreatedAt => "created_at",
            SortField::State => "state",
            SortField::Author => "author",
            SortField::Assignee => "assignee",
            SortField::Label => "label",
            SortField::Milestone => "milestone",
            SortField::UserNotesCount => "comments",
            SortField::Project => "project",
            SortField::Pipeline => "pipeline",
            SortField::Draft => "draft",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "iid" => Some(SortField::Iid),
            "title" => Some(SortField::Title),
            "updated_at" => Some(SortField::UpdatedAt),
            "created_at" => Some(SortField::CreatedAt),
            "state" => Some(SortField::State),
            "author" => Some(SortField::Author),
            "assignee" => Some(SortField::Assignee),
            "label" => Some(SortField::Label),
            "milestone" => Some(SortField::Milestone),
            "comments" | "user_notes_count" => Some(SortField::UserNotesCount),
            "project" => Some(SortField::Project),
            "pipeline" => Some(SortField::Pipeline),
            "draft" => Some(SortField::Draft),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn arrow(&self) -> &'static str {
        match self {
            SortDirection::Asc => "↑",
            SortDirection::Desc => "↓",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "asc" => Some(SortDirection::Asc),
            "desc" => Some(SortDirection::Desc),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
    /// For Label field: which scope prefix to sort by (e.g., "workflow", "p")
    pub label_scope: Option<String>,
}

impl SortSpec {
    pub fn display(&self) -> String {
        let arrow = self.direction.arrow();
        if self.field == SortField::Label
            && let Some(ref scope) = self.label_scope
        {
            return format!("{arrow} {scope}::");
        }
        format!("{arrow} {}", self.field.name())
    }
}

pub fn sort_issues(
    indices: &mut [usize],
    issues: &[TrackedIssue],
    specs: &[SortSpec],
    label_orders: &HashMap<String, Vec<String>>,
) {
    if specs.is_empty() {
        return;
    }
    indices.sort_by(|&a, &b| {
        for spec in specs {
            let ord = compare_issue(&issues[a], &issues[b], spec, label_orders);
            let ord = match spec.direction {
                SortDirection::Asc => ord,
                SortDirection::Desc => ord.reverse(),
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
}

pub fn sort_mrs(
    indices: &mut [usize],
    mrs: &[TrackedMergeRequest],
    specs: &[SortSpec],
    label_orders: &HashMap<String, Vec<String>>,
) {
    if specs.is_empty() {
        return;
    }
    indices.sort_by(|&a, &b| {
        for spec in specs {
            let ord = compare_mr(&mrs[a], &mrs[b], spec, label_orders);
            let ord = match spec.direction {
                SortDirection::Asc => ord,
                SortDirection::Desc => ord.reverse(),
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
}

fn compare_issue(
    a: &TrackedIssue,
    b: &TrackedIssue,
    spec: &SortSpec,
    label_orders: &HashMap<String, Vec<String>>,
) -> Ordering {
    match spec.field {
        SortField::Iid => a.issue.iid.cmp(&b.issue.iid),
        SortField::Title => a
            .issue
            .title
            .to_lowercase()
            .cmp(&b.issue.title.to_lowercase()),
        SortField::UpdatedAt => a.issue.updated_at.cmp(&b.issue.updated_at),
        SortField::CreatedAt => a.issue.created_at.cmp(&b.issue.created_at),
        SortField::State => cmp_state(&a.issue.state, &b.issue.state),
        SortField::Author => cmp_optional_str(
            a.issue.author.as_ref().map(|u| u.username.as_str()),
            b.issue.author.as_ref().map(|u| u.username.as_str()),
        ),
        SortField::Assignee => cmp_optional_str(
            a.issue.assignees.first().map(|u| u.username.as_str()),
            b.issue.assignees.first().map(|u| u.username.as_str()),
        ),
        SortField::Label => {
            let scope = spec.label_scope.as_deref().unwrap_or("");
            let priority = label_orders.get(scope).map(|v| v.as_slice()).unwrap_or(&[]);
            compare_by_label_scope(&a.issue.labels, &b.issue.labels, scope, priority)
        }
        SortField::Milestone => cmp_optional_str(
            a.issue.milestone.as_ref().map(|m| m.title.as_str()),
            b.issue.milestone.as_ref().map(|m| m.title.as_str()),
        ),
        SortField::UserNotesCount => a.issue.user_notes_count.cmp(&b.issue.user_notes_count),
        SortField::Project => a.project_path.cmp(&b.project_path),
        // MR-only fields are no-ops for issues
        SortField::Pipeline | SortField::Draft => Ordering::Equal,
    }
}

fn compare_mr(
    a: &TrackedMergeRequest,
    b: &TrackedMergeRequest,
    spec: &SortSpec,
    label_orders: &HashMap<String, Vec<String>>,
) -> Ordering {
    match spec.field {
        SortField::Iid => a.mr.iid.cmp(&b.mr.iid),
        SortField::Title => a.mr.title.to_lowercase().cmp(&b.mr.title.to_lowercase()),
        SortField::UpdatedAt => a.mr.updated_at.cmp(&b.mr.updated_at),
        SortField::CreatedAt => a.mr.created_at.cmp(&b.mr.created_at),
        SortField::State => cmp_state(&a.mr.state, &b.mr.state),
        SortField::Author => cmp_optional_str(
            a.mr.author.as_ref().map(|u| u.username.as_str()),
            b.mr.author.as_ref().map(|u| u.username.as_str()),
        ),
        SortField::Assignee => cmp_optional_str(
            a.mr.assignees.first().map(|u| u.username.as_str()),
            b.mr.assignees.first().map(|u| u.username.as_str()),
        ),
        SortField::Label => {
            let scope = spec.label_scope.as_deref().unwrap_or("");
            let priority = label_orders.get(scope).map(|v| v.as_slice()).unwrap_or(&[]);
            compare_by_label_scope(&a.mr.labels, &b.mr.labels, scope, priority)
        }
        SortField::Milestone => cmp_optional_str(
            a.mr.milestone.as_ref().map(|m| m.title.as_str()),
            b.mr.milestone.as_ref().map(|m| m.title.as_str()),
        ),
        SortField::UserNotesCount => a.mr.user_notes_count.cmp(&b.mr.user_notes_count),
        SortField::Project => a.project_path.cmp(&b.project_path),
        SortField::Pipeline => {
            let rank = |s: Option<&str>| match s {
                Some("success" | "passed") => 0,
                Some("running") => 1,
                Some("pending") => 2,
                Some("failed") => 3,
                _ => 4,
            };
            let ra = rank(a.mr.head_pipeline.as_ref().map(|p| p.status.as_str()));
            let rb = rank(b.mr.head_pipeline.as_ref().map(|p| p.status.as_str()));
            ra.cmp(&rb)
        }
        SortField::Draft => {
            let da = a.mr.draft || a.mr.work_in_progress;
            let db = b.mr.draft || b.mr.work_in_progress;
            da.cmp(&db) // false (0) < true (1), so non-drafts first
        }
    }
}

/// Compare state strings with a defined order: opened > merged > closed
fn cmp_state(a: &str, b: &str) -> Ordering {
    fn rank(s: &str) -> u8 {
        match s {
            "opened" => 0,
            "merged" => 1,
            "closed" => 2,
            _ => 3,
        }
    }
    rank(a).cmp(&rank(b))
}

/// Compare optional strings; None sorts last.
fn cmp_optional_str(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
