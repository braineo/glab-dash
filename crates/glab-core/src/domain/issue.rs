//! An issue and what only an issue carries: its workflow status, its
//! iteration, its weight.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Item, ItemKind, User};

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

/// The state a closed issue reports.
const STATE_CLOSED: &str = "closed";

/// What a status means, whatever a project chose to call it.  A project names
/// its own — "In Review", "Shipped" — and GitLab files each under one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCategory {
    Triage,
    ToDo,
    InProgress,
    Done,
    Canceled,
    Other,
}

impl StatusCategory {
    /// An unrecognized category reads as [`Self::Other`] rather than failing:
    /// GitLab may add one, and the item still has to show.
    pub fn parse(category: &str) -> Self {
        match category {
            "triage" => Self::Triage,
            "to_do" => Self::ToDo,
            "in_progress" => Self::InProgress,
            "done" => Self::Done,
            "canceled" => Self::Canceled,
            _ => Self::Other,
        }
    }

    pub fn is_done(self) -> bool {
        self == Self::Done
    }

    /// Abandoned — a duplicate, a won't-do.  Off the board, but not delivered.
    pub fn is_canceled(self) -> bool {
        self == Self::Canceled
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::ToDo | Self::InProgress)
    }

    /// Off the board, delivered or not.
    pub fn is_settled(self) -> bool {
        self.is_done() || self.is_canceled()
    }
}

/// A work-item status (`status { name category }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusValue {
    pub name: String,
    /// Status category from GitLab, e.g. "to_do", "in_progress", "done".
    pub category: Option<String>,
}

impl StatusValue {
    pub fn category(&self) -> StatusCategory {
        self.category
            .as_deref()
            .map_or(StatusCategory::Other, StatusCategory::parse)
    }
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
    /// The custom workflow status name, if the issue has one.
    pub fn status_name(&self) -> Option<&str> {
        self.status.as_ref().map(|s| s.name.as_str())
    }

    pub fn status_category(&self) -> Option<StatusCategory> {
        Some(self.status.as_ref()?.category())
    }

    /// Its status says so, or — with no custom status — it is closed.
    pub fn is_done(&self) -> bool {
        self.status_category()
            .map_or(self.state == STATE_CLOSED, StatusCategory::is_done)
    }

    /// Never true without a custom status: a plain closed issue does not say
    /// which it was.
    pub fn is_canceled(&self) -> bool {
        self.status_category()
            .is_some_and(StatusCategory::is_canceled)
    }

    pub fn is_active(&self) -> bool {
        self.status_category()
            .is_some_and(StatusCategory::is_active)
    }

    pub fn in_iteration(&self, gid: &str) -> bool {
        self.iteration.as_ref().is_some_and(|it| it.id == gid)
    }
}

impl Item for Issue {
    fn kind(&self) -> ItemKind {
        ItemKind::Issue
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
        Some(&self.web_url)
    }

    fn labels(&self) -> &[String] {
        &self.labels
    }

    fn assignees(&self) -> &[User] {
        &self.assignees
    }
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

impl WorkItemStatus {
    pub fn category(&self) -> StatusCategory {
        self.category
            .as_deref()
            .map_or(StatusCategory::Other, StatusCategory::parse)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{Issue, Item, ItemKind, StatusCategory, StatusValue};
    use crate::domain::ItemRef;

    fn issue(state: &str, status: Option<(&str, &str)>) -> Issue {
        Issue {
            id: "gid://gitlab/WorkItem/1".to_string(),
            iid: "7".to_string(),
            title: "an issue".to_string(),
            state: state.to_string(),
            author: None,
            assignees: Vec::new(),
            labels: Vec::new(),
            milestone: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
            web_url: String::new(),
            description: None,
            user_notes_count: 0,
            reference: "team/app#7".to_string(),
            status: status.map(|(name, category)| StatusValue {
                name: name.to_string(),
                category: Some(category.to_string()),
            }),
            iteration: None,
            weight: None,
        }
    }

    #[test]
    fn a_custom_status_answers_for_the_issue_and_the_state_answers_without_one() {
        let shipped = issue("opened", Some(("Shipped", "done")));
        assert!(shipped.is_done());
        assert!(!shipped.is_active());
        assert!(!shipped.is_open() || shipped.is_done());

        let dropped = issue("closed", Some(("Duplicate", "canceled")));
        assert!(dropped.is_canceled());
        assert!(!dropped.is_done(), "abandoned work is not delivered work");

        let doing = issue("opened", Some(("In Review", "in_progress")));
        assert!(doing.is_active());
        assert!(!doing.is_done());

        // An unknown category must not read as any of them.
        let odd = issue("opened", Some(("Parked", "on_the_moon")));
        assert_eq!(odd.status_category(), Some(StatusCategory::Other));
        assert!(!odd.is_done() && !odd.is_active() && !odd.is_canceled());

        let plain_open = issue("opened", None);
        assert!(plain_open.is_open() && !plain_open.is_done());
        let plain_closed = issue("closed", None);
        assert!(plain_closed.is_done());
        assert!(!plain_closed.is_canceled());
    }

    #[test]
    fn an_issue_names_itself_as_an_item() {
        let issue = issue("opened", None);
        assert_eq!(issue.project_path(), "team/app");
        assert_eq!(issue.kind(), ItemKind::Issue);
        let named = issue.item_ref();
        assert_eq!(named.reference(), issue.reference());
        assert_eq!(named, ItemRef::issue("team/app", "7"));
    }
}
