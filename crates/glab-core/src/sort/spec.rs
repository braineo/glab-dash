use std::cmp::Ordering;
use std::collections::HashMap;

use crate::domain::{Issue, MergeRequest};
use serde::{Deserialize, Serialize};
use strum::{EnumString, IntoStaticStr, VariantArray};

use super::label_order::compare_by_label_scope;

/// A sortable attribute of an issue or merge request.
///
/// As with `Field`, the snake_case strum strings are the config-file names and
/// the serde representation stays at the default PascalCase, because persisted
/// view state is written in that shape.
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, IntoStaticStr, EnumString, VariantArray,
)]
#[strum(serialize_all = "snake_case")]
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
    /// Named `comments` in config; the GraphQL spelling is accepted too.
    #[strum(to_string = "comments", serialize = "user_notes_count")]
    UserNotesCount,
    Project,
    Weight,
    Iteration,
    // MR-only
    Pipeline,
    Draft,
}

impl SortField {
    /// The fields offered for issues. Hand-written because it is a subset:
    /// `VariantArray` only knows every variant, not which kind each belongs to.
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
            SortField::Weight,
            SortField::Iteration,
        ]
    }

    /// The fields offered for merge requests. See [`SortField::all_issue`].
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
        self.into()
    }

    pub fn from_str(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, EnumString)]
#[strum(serialize_all = "snake_case")]
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
        s.parse().ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    issues: &[Issue],
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
    mrs: &[MergeRequest],
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
    a: &Issue,
    b: &Issue,
    spec: &SortSpec,
    label_orders: &HashMap<String, Vec<String>>,
) -> Ordering {
    match spec.field {
        SortField::Iid => a.iid.cmp(&b.iid),
        SortField::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        SortField::UpdatedAt => a.updated_at.cmp(&b.updated_at),
        SortField::CreatedAt => a.created_at.cmp(&b.created_at),
        SortField::State => cmp_state(&a.state, &b.state),
        SortField::Author => cmp_optional_str(
            a.author.as_ref().map(|u| u.username.as_str()),
            b.author.as_ref().map(|u| u.username.as_str()),
        ),
        SortField::Assignee => cmp_optional_str(
            a.assignees.first().map(|u| u.username.as_str()),
            b.assignees.first().map(|u| u.username.as_str()),
        ),
        SortField::Label => {
            let scope = spec.label_scope.as_deref().unwrap_or("");
            let priority = label_orders.get(scope).map_or([].as_slice(), Vec::as_slice);
            compare_by_label_scope(&a.labels, &b.labels, scope, priority)
        }
        SortField::Milestone => cmp_optional_str(
            a.milestone.as_ref().map(|m| m.title.as_str()),
            b.milestone.as_ref().map(|m| m.title.as_str()),
        ),
        SortField::UserNotesCount => a.user_notes_count.cmp(&b.user_notes_count),
        SortField::Project => a.project_path().cmp(b.project_path()),
        SortField::Weight => {
            let wa = a.weight.unwrap_or(0);
            let wb = b.weight.unwrap_or(0);
            wa.cmp(&wb)
        }
        SortField::Iteration => cmp_optional_str(
            a.iteration.as_ref().and_then(|i| i.title.as_deref()),
            b.iteration.as_ref().and_then(|i| i.title.as_deref()),
        ),
        // MR-only fields are no-ops for issues
        SortField::Pipeline | SortField::Draft => Ordering::Equal,
    }
}

fn compare_mr(
    a: &MergeRequest,
    b: &MergeRequest,
    spec: &SortSpec,
    label_orders: &HashMap<String, Vec<String>>,
) -> Ordering {
    match spec.field {
        SortField::Iid => a.iid.cmp(&b.iid),
        SortField::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        SortField::UpdatedAt => a.updated_at.cmp(&b.updated_at),
        SortField::CreatedAt => a.created_at.cmp(&b.created_at),
        SortField::State => cmp_state(&a.state, &b.state),
        SortField::Author => cmp_optional_str(
            a.author.as_ref().map(|u| u.username.as_str()),
            b.author.as_ref().map(|u| u.username.as_str()),
        ),
        SortField::Assignee => cmp_optional_str(
            a.assignees.first().map(|u| u.username.as_str()),
            b.assignees.first().map(|u| u.username.as_str()),
        ),
        SortField::Label => {
            let scope = spec.label_scope.as_deref().unwrap_or("");
            let priority = label_orders.get(scope).map_or([].as_slice(), Vec::as_slice);
            compare_by_label_scope(&a.labels, &b.labels, scope, priority)
        }
        SortField::Milestone => cmp_optional_str(
            a.milestone.as_ref().map(|m| m.title.as_str()),
            b.milestone.as_ref().map(|m| m.title.as_str()),
        ),
        SortField::UserNotesCount => a.user_notes_count.cmp(&b.user_notes_count),
        SortField::Project => a.project_path().cmp(b.project_path()),
        SortField::Pipeline => {
            let rank = |s: Option<&str>| match s {
                Some("success" | "passed") => 0,
                Some("running") => 1,
                Some("pending") => 2,
                Some("failed") => 3,
                _ => 4,
            };
            let ra = rank(a.pipeline_status());
            let rb = rank(b.pipeline_status());
            ra.cmp(&rb)
        }
        // Issue-only fields are no-ops for MRs
        SortField::Weight | SortField::Iteration => Ordering::Equal,
        SortField::Draft => {
            let da = a.draft;
            let db = b.draft;
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
