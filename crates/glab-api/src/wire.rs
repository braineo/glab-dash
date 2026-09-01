//! The shapes GitLab's GraphQL responses arrive in, and how they fold into the
//! [`glab_core::domain`] types the rest of glab-dash works with.
//!
//! Most selections deserialize straight into a domain type. The exception is a
//! work item, whose fields arrive as a heterogeneous `widgets` array that
//! [`Issue`] flattens: `GqlWorkItem` mirrors that array and the `From` impl
//! folds it.

use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;

use glab_core::domain::{
    Issue, Iteration, MergeRequest, Milestone, StatusValue, User, WorkItemStatus,
};

/// A GraphQL response envelope. Errors are handled before deserialization, so
/// only `data` is read here.
#[derive(Deserialize)]
pub(crate) struct GqlResponse<T> {
    pub data: T,
}

/// A connection selected without `pageInfo`, read for its nodes alone.
#[derive(Deserialize)]
pub(crate) struct GqlNodes<T> {
    pub nodes: Vec<T>,
}

/// One page of a cursor-paginated connection.
#[derive(Deserialize)]
pub(crate) struct GqlPage<T> {
    pub nodes: Vec<T>,
    #[serde(rename = "pageInfo")]
    pub page_info: GqlPageInfo,
}

#[derive(Deserialize)]
pub(crate) struct GqlPageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
    #[serde(rename = "endCursor")]
    pub end_cursor: Option<String>,
}

/// A response body that holds one paginated connection.
///
/// The path down to the connection differs per query and can be absent — an
/// unknown project, a user the token cannot see — which `None` reports as an
/// empty walk rather than an error.
pub(crate) trait Paged<T> {
    fn page(self) -> Option<GqlPage<T>>;
}

// ── Work items (namespace.workItems, workItemUpdate) ──

#[derive(Deserialize)]
pub(crate) struct GqlNamespaceWorkItems {
    pub namespace: Option<GqlNamespace>,
}

#[derive(Deserialize)]
pub(crate) struct GqlNamespace {
    #[serde(rename = "workItems")]
    pub work_items: GqlPage<GqlWorkItem>,
}

impl Paged<Issue> for GqlNamespaceWorkItems {
    fn page(self) -> Option<GqlPage<Issue>> {
        let page = self.namespace?.work_items;
        Some(GqlPage {
            nodes: page.nodes.into_iter().map(Issue::from).collect(),
            page_info: page.page_info,
        })
    }
}

/// A work item as `WorkItemFields` selects it.
#[derive(Deserialize)]
pub(crate) struct GqlWorkItem {
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
    widgets: Vec<GqlWidget>,
}

#[derive(Deserialize)]
struct GqlLabel {
    title: String,
}

/// One entry of a work item's `widgets` array, flattened across every widget
/// type the selection asks for.
///
/// Each element carries only the fields of its own type, so every field here is
/// genuinely absent on most elements: the `default`s answer the union's shape,
/// not a distrust of the schema.
#[derive(Deserialize, Default)]
struct GqlWidget {
    #[serde(default)]
    assignees: Option<GqlNodes<User>>,
    #[serde(default)]
    labels: Option<GqlNodes<GqlLabel>>,
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

impl From<GqlWorkItem> for Issue {
    fn from(w: GqlWorkItem) -> Self {
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
            // workItems returns OPEN/CLOSED; normalize to opened/closed
            state: match w.state.to_lowercase().as_str() {
                "open" => "opened".to_string(),
                other => other.to_string(),
            },
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

// ── Root issues query ──

#[derive(Deserialize)]
pub(crate) struct GqlRootIssues {
    issues: GqlPage<Issue>,
}

impl Paged<Issue> for GqlRootIssues {
    fn page(self) -> Option<GqlPage<Issue>> {
        Some(self.issues)
    }
}

// ── Merge requests ──

#[derive(Deserialize)]
pub(crate) struct GqlProjectMrs {
    project: Option<GqlProject>,
}

#[derive(Deserialize)]
struct GqlProject {
    #[serde(rename = "mergeRequests")]
    merge_requests: GqlPage<MergeRequest>,
}

impl Paged<MergeRequest> for GqlProjectMrs {
    fn page(self) -> Option<GqlPage<MergeRequest>> {
        Some(self.project?.merge_requests)
    }
}

#[derive(Deserialize)]
pub(crate) struct GqlUserMrs {
    user: Option<GqlUserMrConnection>,
}

/// One response shape for both `assignedMergeRequests` and
/// `reviewRequestedMergeRequests` — whichever field the document selected wins.
#[derive(Deserialize)]
struct GqlUserMrConnection {
    #[serde(
        alias = "assignedMergeRequests",
        alias = "reviewRequestedMergeRequests"
    )]
    mrs: GqlPage<MergeRequest>,
}

impl Paged<MergeRequest> for GqlUserMrs {
    fn page(self) -> Option<GqlPage<MergeRequest>> {
        Some(self.user?.mrs)
    }
}

// ── Iterations ──

#[derive(Deserialize)]
pub(crate) struct GqlGroupIterations {
    group: Option<GqlGroup>,
}

#[derive(Deserialize)]
struct GqlGroup {
    iterations: GqlPage<Iteration>,
}

impl Paged<Iteration> for GqlGroupIterations {
    fn page(self) -> Option<GqlPage<Iteration>> {
        Some(self.group?.iterations)
    }
}

// ── Work item statuses ──

#[derive(Deserialize)]
pub(crate) struct GqlStatusesData {
    pub namespace: Option<GqlStatusNamespace>,
}

#[derive(Deserialize)]
pub(crate) struct GqlStatusNamespace {
    #[serde(rename = "workItemTypes")]
    pub work_item_types: GqlNodes<GqlWorkItemType>,
}

#[derive(Deserialize)]
pub(crate) struct GqlWorkItemType {
    #[serde(rename = "widgetDefinitions")]
    pub widget_definitions: Vec<GqlWidgetDefinition>,
}

/// One widget definition. `allowedStatuses` comes from an inline fragment on
/// the status widget alone, so it is absent on every other definition in the
/// array.
#[derive(Deserialize)]
pub(crate) struct GqlWidgetDefinition {
    #[serde(default, rename = "allowedStatuses")]
    pub allowed_statuses: Option<Vec<GqlAllowedStatus>>,
}

#[derive(Deserialize)]
pub(crate) struct GqlAllowedStatus {
    id: String,
    name: String,
    position: Option<i32>,
    category: Option<String>,
}

impl From<GqlAllowedStatus> for WorkItemStatus {
    fn from(s: GqlAllowedStatus) -> Self {
        WorkItemStatus {
            id: s.id,
            name: s.name,
            position: s.position,
            category: s.category,
        }
    }
}

// ── Work item activity notes ──

#[derive(Deserialize)]
pub(crate) struct GqlWorkItemNotes {
    pub workspace: Option<GqlNotesNamespace>,
}

#[derive(Deserialize)]
pub(crate) struct GqlNotesNamespace {
    #[serde(rename = "workItem")]
    pub work_item: Option<GqlNotesWorkItem>,
}

#[derive(Deserialize)]
pub(crate) struct GqlNotesWorkItem {
    pub widgets: Vec<GqlNotesWidget>,
}

/// A notes widget. `discussions` comes from an inline fragment, so it is absent
/// on any other widget the array happens to carry.
#[derive(Deserialize)]
pub(crate) struct GqlNotesWidget {
    #[serde(default)]
    pub discussions: Option<GqlNodes<GqlNoteDiscussion>>,
}

#[derive(Deserialize)]
pub(crate) struct GqlNoteDiscussion {
    pub notes: GqlNodes<GqlSystemNote>,
}

#[derive(Deserialize)]
pub(crate) struct GqlSystemNote {
    pub system: bool,
    #[serde(rename = "systemNoteIconName")]
    pub icon: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<FixedOffset>,
}
