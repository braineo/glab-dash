//! The shapes GitLab's GraphQL responses arrive in, and how they fold into the
//! [`glab_core::domain`] types the rest of glab-dash works with.
//!
//! Types are named for the schema type they select.  One module per document,
//! since the same schema type is selected differently by different documents,
//! rooted at the `Query` selection that document sends.

use serde::Deserialize;

/// GraphQL answers `OPEN`/`CLOSED` where the rest of glab-dash spells the same
/// states `opened`/`closed`.
pub(crate) fn normalize_state(state: &str) -> String {
    match state.to_lowercase().as_str() {
        "open" => "opened".to_string(),
        other => other.to_string(),
    }
}

/// A GraphQL response envelope. Errors are handled before deserialization, so
/// only `data` is read here.
#[derive(Deserialize)]
pub(crate) struct Response<T> {
    pub data: T,
}

/// A connection selected without `pageInfo`, read for its nodes alone.
#[derive(Deserialize)]
pub(crate) struct Nodes<T> {
    pub nodes: Vec<T>,
}

/// One page of a cursor-paginated connection.
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

/// A response body that holds one paginated connection.
///
/// The path down to the connection differs per query and can be absent — an
/// unknown project, a user the token cannot see — which `None` reports as an
/// empty walk rather than an error.
pub(crate) trait Paged<T> {
    fn page(self) -> Option<Page<T>>;
}

/// `namespace.workItems`, and the `workItemUpdate` payload that answers in the
/// same shape.
pub(crate) mod work_items {
    use chrono::{DateTime, FixedOffset, Utc};
    use serde::Deserialize;

    use glab_core::domain::{Issue, Iteration, Milestone, StatusValue, User};

    use super::{Nodes, Page, Paged, normalize_state};

    #[derive(Deserialize)]
    pub(crate) struct Query {
        pub namespace: Option<Namespace>,
    }

    #[derive(Deserialize)]
    pub(crate) struct Namespace {
        #[serde(rename = "workItems")]
        pub work_items: Page<WorkItem>,
    }

    impl Paged<Issue> for Query {
        fn page(self) -> Option<Page<Issue>> {
            let page = self.namespace?.work_items;
            Some(Page {
                nodes: page.nodes.into_iter().map(Issue::from).collect(),
                page_info: page.page_info,
            })
        }
    }

    /// A work item as `WorkItemFields` selects it.
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
}

/// The root `issues` connection, which answers in the domain's own shape.
pub(crate) mod root_issues {
    use serde::Deserialize;

    use glab_core::domain::Issue;

    use super::{Page, Paged};

    #[derive(Deserialize)]
    pub(crate) struct Query {
        issues: Page<Issue>,
    }

    impl Paged<Issue> for Query {
        fn page(self) -> Option<Page<Issue>> {
            Some(self.issues)
        }
    }
}

/// `project.mergeRequests`.
pub(crate) mod project_mrs {
    use serde::Deserialize;

    use glab_core::domain::MergeRequest;

    use super::{Page, Paged};

    #[derive(Deserialize)]
    pub(crate) struct Query {
        project: Option<Project>,
    }

    #[derive(Deserialize)]
    struct Project {
        #[serde(rename = "mergeRequests")]
        merge_requests: Page<MergeRequest>,
    }

    impl Paged<MergeRequest> for Query {
        fn page(self) -> Option<Page<MergeRequest>> {
            Some(self.project?.merge_requests)
        }
    }
}

/// A `user`'s merge requests, under whichever of its connections the document
/// selected.
pub(crate) mod user_mrs {
    use serde::Deserialize;

    use glab_core::domain::MergeRequest;

    use super::{Page, Paged};

    #[derive(Deserialize)]
    pub(crate) struct Query {
        user: Option<User>,
    }

    /// One response shape for every merge-request connection a user has —
    /// `authoredMergeRequests`, `assignedMergeRequests` or
    /// `reviewRequestedMergeRequests` — whichever the document selected wins.
    #[derive(Deserialize)]
    struct User {
        #[serde(
            alias = "authoredMergeRequests",
            alias = "assignedMergeRequests",
            alias = "reviewRequestedMergeRequests"
        )]
        mrs: Page<MergeRequest>,
    }

    impl Paged<MergeRequest> for Query {
        fn page(self) -> Option<Page<MergeRequest>> {
            Some(self.user?.mrs)
        }
    }
}

/// `group.iterations`.
pub(crate) mod iterations {
    use serde::Deserialize;

    use glab_core::domain::Iteration;

    use super::{Page, Paged};

    #[derive(Deserialize)]
    pub(crate) struct Query {
        group: Option<Group>,
    }

    #[derive(Deserialize)]
    struct Group {
        iterations: Page<Iteration>,
    }

    impl Paged<Iteration> for Query {
        fn page(self) -> Option<Page<Iteration>> {
            Some(self.group?.iterations)
        }
    }
}

/// The statuses a namespace's work item types allow.
pub(crate) mod statuses {
    use serde::Deserialize;

    use glab_core::domain;

    use super::Nodes;

    #[derive(Deserialize)]
    pub(crate) struct Query {
        pub namespace: Option<Namespace>,
    }

    #[derive(Deserialize)]
    pub(crate) struct Namespace {
        #[serde(rename = "workItemTypes")]
        pub work_item_types: Nodes<WorkItemType>,
    }

    #[derive(Deserialize)]
    pub(crate) struct WorkItemType {
        #[serde(rename = "widgetDefinitions")]
        pub widget_definitions: Vec<WorkItemWidgetDefinition>,
    }

    /// One widget definition. `allowedStatuses` comes from an inline fragment on
    /// the status widget alone, so it is absent on every other definition in the
    /// array.
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
}

/// A work item's activity notes, for telling a system note from a comment.
pub(crate) mod notes {
    use chrono::{DateTime, FixedOffset};
    use serde::Deserialize;

    use super::Nodes;

    #[derive(Deserialize)]
    pub(crate) struct Query {
        pub workspace: Option<Namespace>,
    }

    #[derive(Deserialize)]
    pub(crate) struct Namespace {
        #[serde(rename = "workItem")]
        pub work_item: Option<WorkItem>,
    }

    #[derive(Deserialize)]
    pub(crate) struct WorkItem {
        pub widgets: Vec<WorkItemWidget>,
    }

    /// `discussions` comes from an inline fragment, so it is absent on any other
    /// widget the array happens to carry.
    #[derive(Deserialize)]
    pub(crate) struct WorkItemWidget {
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
}
