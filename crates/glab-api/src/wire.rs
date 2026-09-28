//! Types are named for the schema type they select, prefixed with the document
//! where two documents select the same type differently; every document's root
//! is its `<Document>Query`.

use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;

use glab_core::domain::{self, Issue, Iteration, MergeRequest, Milestone, StatusValue, User};

/// GraphQL answers `OPEN`/`CLOSED`; everything else spells them
/// `opened`/`closed`.
pub(crate) fn normalize_state(state: &str) -> String {
    match state.to_lowercase().as_str() {
        "open" => "opened".to_string(),
        other => other.to_string(),
    }
}

#[derive(Deserialize)]
pub(crate) struct Response<T> {
    pub data: T,
}

#[derive(Deserialize)]
pub(crate) struct Nodes<T> {
    pub nodes: Vec<T>,
}

#[derive(Deserialize)]
pub(crate) struct Page<T> {
    pub nodes: Vec<T>,
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
}

#[derive(Deserialize)]
pub(crate) struct PageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
    #[serde(rename = "endCursor")]
    pub end_cursor: Option<String>,
}

/// The path down to the connection differs per query and can be absent — an
/// unknown project, a user the token cannot see — which `None` reports.
pub(crate) trait Paged<T> {
    fn page(self) -> Option<Page<T>>;
}

#[derive(Deserialize)]
pub(crate) struct WorkItemsQuery {
    pub namespace: Option<WorkItemsNamespace>,
}

#[derive(Deserialize)]
pub(crate) struct WorkItemsNamespace {
    #[serde(rename = "workItems")]
    pub work_items: Page<WorkItem>,
}

impl Paged<Issue> for WorkItemsQuery {
    fn page(self) -> Option<Page<Issue>> {
        let page = self.namespace?.work_items;
        Some(Page {
            nodes: page.nodes.into_iter().map(Issue::from).collect(),
            page_info: page.page_info,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct WorkItem {
    id: String,
    iid: String,
    title: String,
    state: String,
    author: Option<User>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<FixedOffset>,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<FixedOffset>,
    #[serde(rename = "closedAt")]
    closed_at: Option<DateTime<FixedOffset>>,
    #[serde(rename = "webUrl")]
    web_url: String,
    reference: String,
    widgets: Vec<WorkItemWidget>,
}

#[derive(Deserialize)]
struct Label {
    title: String,
}

#[derive(Deserialize, Default)]
struct WorkItemWidget {
    #[serde(default)]
    assignees: Option<Nodes<User>>,
    #[serde(default)]
    labels: Option<Nodes<Label>>,
    #[serde(default)]
    milestone: Option<Milestone>,
    #[serde(default)]
    status: Option<StatusValue>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    iteration: Option<Iteration>,
    #[serde(default)]
    weight: Option<u32>,
}

impl From<WorkItem> for Issue {
    fn from(w: WorkItem) -> Self {
        let mut assignees = Vec::new();
        let mut labels = Vec::new();
        let mut milestone = None;
        let mut status = None;
        let mut description = None;
        let mut iteration = None;
        let mut weight = None;

        for widget in w.widgets {
            if let Some(a) = widget.assignees {
                assignees = a.nodes;
            }
            if let Some(l) = widget.labels {
                labels = l.nodes.into_iter().map(|l| l.title).collect();
            }
            if let Some(m) = widget.milestone {
                milestone = Some(m);
            }
            if let Some(s) = widget.status {
                status = Some(s);
            }
            if let Some(d) = widget.description {
                description = Some(d);
            }
            if let Some(i) = widget.iteration {
                iteration = Some(i);
            }
            if let Some(w) = widget.weight {
                weight = Some(w);
            }
        }

        Issue {
            id: w.id,
            iid: w.iid,
            title: w.title,
            state: normalize_state(&w.state),
            author: w.author,
            assignees,
            labels,
            milestone,
            created_at: w.created_at.with_timezone(&Utc),
            updated_at: w.updated_at.with_timezone(&Utc),
            closed_at: w.closed_at.map(|dt| dt.with_timezone(&Utc)),
            web_url: w.web_url,
            description,
            user_notes_count: 0,
            reference: w.reference,
            status,
            iteration,
            weight,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct RootIssuesQuery {
    issues: Page<Issue>,
}

impl Paged<Issue> for RootIssuesQuery {
    fn page(self) -> Option<Page<Issue>> {
        Some(self.issues)
    }
}

#[derive(Deserialize)]
pub(crate) struct ProjectMrsQuery {
    project: Option<Project>,
}

#[derive(Deserialize)]
struct Project {
    #[serde(rename = "mergeRequests")]
    merge_requests: Page<MergeRequest>,
}

impl Paged<MergeRequest> for ProjectMrsQuery {
    fn page(self) -> Option<Page<MergeRequest>> {
        Some(self.project?.merge_requests)
    }
}

#[derive(Deserialize)]
pub(crate) struct UserMrsQuery {
    user: Option<UserMrConnection>,
}

/// Serves `authoredMergeRequests`, `assignedMergeRequests` and
/// `reviewRequestedMergeRequests`: whichever the document selected wins.
#[derive(Deserialize)]
struct UserMrConnection {
    #[serde(
        alias = "authoredMergeRequests",
        alias = "assignedMergeRequests",
        alias = "reviewRequestedMergeRequests"
    )]
    mrs: Page<MergeRequest>,
}

impl Paged<MergeRequest> for UserMrsQuery {
    fn page(self) -> Option<Page<MergeRequest>> {
        Some(self.user?.mrs)
    }
}

#[derive(Deserialize)]
pub(crate) struct IterationsQuery {
    group: Option<Group>,
}

#[derive(Deserialize)]
struct Group {
    iterations: Page<Iteration>,
}

impl Paged<Iteration> for IterationsQuery {
    fn page(self) -> Option<Page<Iteration>> {
        Some(self.group?.iterations)
    }
}

#[derive(Deserialize)]
pub(crate) struct StatusesQuery {
    pub namespace: Option<StatusesNamespace>,
}

#[derive(Deserialize)]
pub(crate) struct StatusesNamespace {
    #[serde(rename = "workItemTypes")]
    pub work_item_types: Nodes<WorkItemType>,
}

#[derive(Deserialize)]
pub(crate) struct WorkItemType {
    #[serde(rename = "widgetDefinitions")]
    pub widget_definitions: Vec<WorkItemWidgetDefinition>,
}

#[derive(Deserialize)]
pub(crate) struct WorkItemWidgetDefinition {
    #[serde(default, rename = "allowedStatuses")]
    pub allowed_statuses: Option<Vec<WorkItemStatus>>,
}

#[derive(Deserialize)]
pub(crate) struct WorkItemStatus {
    id: String,
    name: String,
    position: Option<i32>,
    category: Option<String>,
}

impl From<WorkItemStatus> for domain::WorkItemStatus {
    fn from(s: WorkItemStatus) -> Self {
        domain::WorkItemStatus {
            id: s.id,
            name: s.name,
            position: s.position,
            category: s.category,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct NotesQuery {
    pub workspace: Option<NotesNamespace>,
}

#[derive(Deserialize)]
pub(crate) struct NotesNamespace {
    #[serde(rename = "workItem")]
    pub work_item: Option<NotesWorkItem>,
}

#[derive(Deserialize)]
pub(crate) struct NotesWorkItem {
    pub widgets: Vec<NotesWidget>,
}

#[derive(Deserialize)]
pub(crate) struct NotesWidget {
    #[serde(default)]
    pub discussions: Option<Nodes<Discussion>>,
}

#[derive(Deserialize)]
pub(crate) struct Discussion {
    pub notes: Nodes<Note>,
}

#[derive(Deserialize)]
pub(crate) struct Note {
    pub system: bool,
    #[serde(rename = "systemNoteIconName")]
    pub icon: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<FixedOffset>,
}
