use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Utc};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;

use crate::config::Config;
use crate::gitlab::types::{
    ApprovalUser, Discussion, Issue, Iteration, MergeRequest, MergeRequestApprovals, Milestone,
    Note, ProjectLabel, References, TrackedIssue, TrackedMergeRequest, User, WorkItemStatus,
};

// ── GraphQL response types (serde-driven) ──

fn gid_to_u64(gid: &str) -> u64 {
    gid.rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("invalid GID: {gid}"))
}

fn deserialize_string_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    match &v {
        serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("not u64")),
        _ => Err(serde::de::Error::custom("expected string or number")),
    }
}

// ── GraphQL serde types ──

#[derive(Deserialize)]
struct GqlResponse<T> {
    data: T,
}

#[derive(Deserialize)]
struct GqlConnection<T> {
    nodes: Vec<T>,
    #[serde(default, rename = "pageInfo")]
    page_info: Option<GqlPageInfo>,
}

impl<T> Default for GqlConnection<T> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            page_info: None,
        }
    }
}

#[derive(Deserialize)]
struct GqlPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(default, rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct GqlNamespaceData {
    namespace: GqlNamespace,
}

#[derive(Deserialize)]
struct GqlNamespace {
    #[serde(rename = "workItems")]
    work_items: GqlConnection<GqlWorkItem>,
}

#[derive(Deserialize)]
struct GqlWorkItem {
    id: String,
    #[serde(deserialize_with = "deserialize_string_u64")]
    iid: u64,
    title: String,
    state: String,
    #[serde(default)]
    author: Option<GqlUser>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<FixedOffset>,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<FixedOffset>,
    #[serde(rename = "closedAt")]
    closed_at: Option<DateTime<FixedOffset>>,
    #[serde(rename = "webUrl")]
    web_url: String,
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    namespace: Option<GqlItemNamespace>,
    #[serde(default)]
    widgets: Vec<GqlWidget>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GqlItemNamespace {
    #[serde(rename = "fullPath")]
    full_path: String,
}

#[derive(Deserialize, Default)]
struct GqlUser {
    #[serde(default)]
    id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "webUrl")]
    web_url: String,
}

#[derive(Deserialize, Default)]
struct GqlLabel {
    title: String,
}

#[derive(Deserialize)]
struct GqlMilestone {
    id: String,
    title: String,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct GqlStatusValue {
    name: String,
    category: Option<String>,
}

#[derive(Deserialize)]
struct GqlIteration {
    id: String,
    /// Nullable in GitLab schema — iterations may have no title.
    title: Option<String>,
    #[serde(default, rename = "startDate")]
    start_date: Option<String>,
    #[serde(default, rename = "dueDate")]
    due_date: Option<String>,
    state: String,
}

/// Serde flattens all widget types into one struct.
/// Unknown fields are ignored; each widget type only populates its fields.
#[derive(Deserialize, Default)]
struct GqlWidget {
    #[serde(default)]
    assignees: Option<GqlConnection<GqlUser>>,
    #[serde(default)]
    labels: Option<GqlConnection<GqlLabel>>,
    #[serde(default)]
    milestone: Option<GqlMilestone>,
    #[serde(default)]
    status: Option<GqlStatusValue>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    iteration: Option<GqlIteration>,
    #[serde(default)]
    weight: Option<u32>,
}

#[derive(Deserialize)]
struct GqlAllowedStatus {
    id: String,
    name: String,
    #[serde(default, rename = "iconName")]
    icon_name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    position: Option<i32>,
    #[serde(default)]
    category: Option<String>,
}

impl From<GqlAllowedStatus> for WorkItemStatus {
    fn from(s: GqlAllowedStatus) -> Self {
        WorkItemStatus {
            id: s.id,
            name: s.name,
            icon_name: s.icon_name,
            color: s.color,
            position: s.position,
            category: s.category,
        }
    }
}

// ── MR GraphQL types ──

#[derive(Deserialize)]
struct GqlProjectMrData {
    project: Option<GqlProjectMrs>,
}

#[derive(Deserialize)]
struct GqlProjectMrs {
    #[serde(default, rename = "mergeRequests")]
    merge_requests: GqlConnection<GqlMergeRequest>,
}

/// All fields here are explicitly requested in `MR_FIELDS`.
/// No `#[serde(default)]` on queried fields — if deserialization fails
/// the error surfaces instead of silently producing None/0.
#[derive(Deserialize)]
struct GqlMergeRequest {
    id: String,
    #[serde(deserialize_with = "deserialize_string_u64")]
    iid: u64,
    title: String,
    state: String,
    draft: bool,
    author: Option<GqlUser>,
    assignees: GqlConnection<GqlUser>,
    reviewers: GqlConnection<GqlUser>,
    labels: GqlConnection<GqlLabel>,
    milestone: Option<GqlMilestone>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<FixedOffset>,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<FixedOffset>,
    #[serde(rename = "webUrl")]
    web_url: String,
    description: Option<String>,
    #[serde(rename = "userNotesCount")]
    user_notes_count: u64,
    #[serde(rename = "sourceBranch")]
    source_branch: String,
    #[serde(rename = "targetBranch")]
    target_branch: String,
    #[serde(rename = "mergeStatusEnum")]
    merge_status_enum: Option<String>,
    reference: Option<String>,
    #[serde(rename = "diffStatsSummary")]
    diff_stats_summary: Option<GqlDiffStatsSummary>,
    approved: Option<bool>,
    #[serde(rename = "approvedBy")]
    approved_by: GqlConnection<GqlUser>,
    #[serde(rename = "headPipeline")]
    head_pipeline: Option<GqlPipelineRef>,
    #[serde(rename = "resolvableDiscussionsCount")]
    resolvable_discussions_count: u64,
    #[serde(rename = "resolvedDiscussionsCount")]
    resolved_discussions_count: u64,
}

#[derive(Deserialize)]
struct GqlDiffStatsSummary {
    additions: u64,
    deletions: u64,
    #[serde(rename = "fileCount")]
    file_count: u64,
}

#[derive(Deserialize)]
struct GqlPipelineRef {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct GqlUserMrData {
    user: Option<GqlUserMrs>,
}

/// Unified response for both `assignedMergeRequests` and
/// `reviewRequestedMergeRequests` — whichever field is present wins.
#[derive(Deserialize)]
struct GqlUserMrs {
    #[serde(
        alias = "assignedMergeRequests",
        alias = "reviewRequestedMergeRequests"
    )]
    mrs: GqlConnection<GqlMergeRequest>,
}

impl From<GqlMergeRequest> for MergeRequest {
    fn from(gql: GqlMergeRequest) -> Self {
        let unresolved = gql
            .resolvable_discussions_count
            .saturating_sub(gql.resolved_discussions_count);

        let (diff_additions, diff_deletions, diff_file_count) = match gql.diff_stats_summary {
            Some(ds) => (Some(ds.additions), Some(ds.deletions), Some(ds.file_count)),
            None => (None, None, None),
        };

        let pipeline = gql.head_pipeline.and_then(|p| {
            p.status.map(|status| crate::gitlab::types::Pipeline {
                id: 0,
                status: status.to_lowercase(),
                ref_name: None,
                web_url: String::new(),
            })
        });

        MergeRequest {
            id: gid_to_u64(&gql.id),
            iid: gql.iid,
            title: gql.title,
            state: gql.state,
            author: gql.author.map(|u| User {
                id: gid_to_u64(&u.id),
                username: u.username,
                name: u.name,
                avatar_url: None,
                web_url: u.web_url,
            }),
            assignees: gql
                .assignees
                .nodes
                .into_iter()
                .map(|u| User {
                    id: gid_to_u64(&u.id),
                    username: u.username,
                    name: u.name,
                    avatar_url: None,
                    web_url: u.web_url,
                })
                .collect(),
            reviewers: gql
                .reviewers
                .nodes
                .into_iter()
                .map(|u| User {
                    id: gid_to_u64(&u.id),
                    username: u.username,
                    name: u.name,
                    avatar_url: None,
                    web_url: u.web_url,
                })
                .collect(),
            labels: gql.labels.nodes.into_iter().map(|l| l.title).collect(),
            milestone: gql.milestone.map(|m| Milestone {
                id: gid_to_u64(&m.id),
                title: m.title,
                state: m.state.unwrap_or_default(),
            }),
            created_at: gql.created_at.with_timezone(&Utc),
            updated_at: gql.updated_at.with_timezone(&Utc),
            web_url: gql.web_url,
            description: gql.description,
            draft: gql.draft,
            work_in_progress: false,
            merge_status: gql.merge_status_enum,
            source_branch: gql.source_branch,
            target_branch: gql.target_branch,
            head_pipeline: pipeline,
            user_notes_count: gql.user_notes_count,
            references: gql.reference.map(|r| References { full_ref: r }),
            approved_by: gql
                .approved_by
                .nodes
                .into_iter()
                .map(|u| ApprovalUser {
                    user: User {
                        id: gid_to_u64(&u.id),
                        username: u.username,
                        name: u.name,
                        avatar_url: None,
                        web_url: u.web_url,
                    },
                })
                .collect(),
            diff_additions,
            diff_deletions,
            diff_file_count,
            approved: gql.approved,
            unresolved_threads: Some(unresolved),
        }
    }
}

// ── Root issues query (for assigned issues outside tracking namespace) ──

#[derive(Deserialize)]
struct GqlRootIssuesData {
    issues: GqlConnection<GqlRootIssue>,
}

/// Issue from the root `issues` query — has assignees/labels/status as direct fields.
#[derive(Deserialize)]
struct GqlRootIssue {
    id: String,
    #[serde(deserialize_with = "deserialize_string_u64")]
    iid: u64,
    title: String,
    state: String,
    #[serde(default)]
    author: Option<GqlUser>,
    #[serde(default)]
    assignees: GqlConnection<GqlUser>,
    #[serde(default)]
    labels: GqlConnection<GqlLabel>,
    #[serde(default)]
    milestone: Option<GqlMilestone>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<FixedOffset>,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<FixedOffset>,
    #[serde(rename = "closedAt")]
    closed_at: Option<DateTime<FixedOffset>>,
    #[serde(rename = "webUrl")]
    web_url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    status: Option<GqlStatusValue>,
    #[serde(default)]
    iteration: Option<GqlIteration>,
    #[serde(default)]
    weight: Option<u32>,
}

impl From<GqlRootIssue> for Issue {
    fn from(g: GqlRootIssue) -> Self {
        let to_user = |u: GqlUser| User {
            id: gid_to_u64(&u.id),
            username: u.username,
            name: u.name,
            avatar_url: None,
            web_url: u.web_url,
        };
        Issue {
            id: gid_to_u64(&g.id),
            iid: g.iid,
            title: g.title,
            state: g.state, // already "opened"/"closed"
            author: g.author.map(&to_user),
            assignees: g.assignees.nodes.into_iter().map(&to_user).collect(),
            labels: g.labels.nodes.into_iter().map(|l| l.title).collect(),
            milestone: g.milestone.map(|m| Milestone {
                id: gid_to_u64(&m.id),
                title: m.title,
                state: m.state.unwrap_or_else(|| "active".to_string()),
            }),
            created_at: g.created_at.with_timezone(&Utc),
            updated_at: g.updated_at.with_timezone(&Utc),
            closed_at: g.closed_at.map(|dt| dt.with_timezone(&Utc)),
            web_url: g.web_url,
            description: g.description,
            user_notes_count: 0,
            references: g.reference.map(|r| References { full_ref: r }),
            custom_status_category: g.status.as_ref().and_then(|s| s.category.clone()),
            custom_status: g.status.map(|s| s.name),
            iteration: g.iteration.map(|i| Iteration {
                id: i.id,
                title: i.title.unwrap_or_default(),
                start_date: i.start_date,
                due_date: i.due_date,
                state: i.state,
            }),
            weight: g.weight,
        }
    }
}

impl From<GqlWorkItem> for Issue {
    fn from(w: GqlWorkItem) -> Self {
        let to_user = |u: GqlUser| User {
            id: gid_to_u64(&u.id),
            username: u.username,
            name: u.name,
            avatar_url: None,
            web_url: u.web_url,
        };

        let mut assignees = Vec::new();
        let mut labels = Vec::new();
        let mut milestone = None;
        let mut custom_status = None;
        let mut custom_status_category = None;
        let mut description = None;
        let mut iteration = None;
        let mut weight = None;

        for widget in w.widgets {
            if let Some(a) = widget.assignees {
                assignees = a.nodes.into_iter().map(&to_user).collect();
            }
            if let Some(l) = widget.labels {
                labels = l.nodes.into_iter().map(|l| l.title).collect();
            }
            if let Some(m) = widget.milestone {
                milestone = Some(Milestone {
                    id: gid_to_u64(&m.id),
                    title: m.title,
                    state: m.state.unwrap_or_else(|| "active".to_string()),
                });
            }
            if let Some(s) = widget.status {
                custom_status = Some(s.name);
                custom_status_category = s.category;
            }
            if let Some(d) = widget.description {
                description = Some(d);
            }
            if let Some(i) = widget.iteration {
                iteration = Some(Iteration {
                    id: i.id,
                    title: i.title.unwrap_or_default(),
                    start_date: i.start_date,
                    due_date: i.due_date,
                    state: i.state,
                });
            }
            if let Some(w) = widget.weight {
                weight = Some(w);
            }
        }

        Issue {
            id: gid_to_u64(&w.id),
            iid: w.iid,
            title: w.title,
            // workItems returns OPEN/CLOSED; normalize to opened/closed
            state: match w.state.to_lowercase().as_str() {
                "open" => "opened".to_string(),
                other => other.to_string(),
            },
            author: w.author.map(&to_user),
            assignees,
            labels,
            milestone,
            created_at: w.created_at.with_timezone(&Utc),
            updated_at: w.updated_at.with_timezone(&Utc),
            closed_at: w.closed_at.map(|dt| dt.with_timezone(&Utc)),
            web_url: w.web_url,
            description,
            user_notes_count: 0,
            references: w.reference.map(|r| References { full_ref: r }),
            custom_status,
            custom_status_category,
            iteration,
            weight,
        }
    }
}

#[derive(Clone)]
pub struct GitLabClient {
    client: reqwest::Client,
    base_url: String,
    config: Config,
}

impl GitLabClient {
    pub fn new(config: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "PRIVATE-TOKEN",
            HeaderValue::from_str(&config.token).context("Invalid token")?,
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        let base_url = config.gitlab_url.trim_end_matches('/').to_string();

        Ok(Self {
            client,
            base_url,
            config: config.clone(),
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.base_url, path)
    }

    fn encode_project(project: &str) -> String {
        project.replace('/', "%2F")
    }

    // ── Issues (GraphQL via namespace.workItems) ──

    const WORK_ITEM_QUERY: &str = r"
        query($path: ID!, $state: IssuableState, $assigneeUsernames: [String!], $updatedAfter: Time, $after: String) {
            namespace(fullPath: $path) {
                workItems(
                    includeDescendants: true
                    types: [ISSUE]
                    state: $state
                    assigneeUsernames: $assigneeUsernames
                    updatedAfter: $updatedAfter
                    after: $after
                    first: 100
                    sort: UPDATED_DESC
                ) {
                    nodes {
                        id iid title state
                        author { id username name webUrl }
                        createdAt updatedAt closedAt webUrl
                        reference(full: true)
                        namespace { fullPath }
                        widgets(onlyTypes: [STATUS, ASSIGNEES, LABELS, MILESTONE, DESCRIPTION, ITERATION, WEIGHT]) {
                            ... on WorkItemWidgetAssignees {
                                assignees { nodes { id username name webUrl } }
                            }
                            ... on WorkItemWidgetLabels {
                                labels { nodes { title } }
                            }
                            ... on WorkItemWidgetMilestone {
                                milestone { id title state }
                            }
                            ... on WorkItemWidgetStatus {
                                status { name category }
                            }
                            ... on WorkItemWidgetDescription {
                                description
                            }
                            ... on WorkItemWidgetIteration {
                                iteration { id title startDate dueDate state }
                            }
                            ... on WorkItemWidgetWeight {
                                weight
                            }
                        }
                    }
                    pageInfo { hasNextPage endCursor }
                }
            }
        }
    ";

    /// Fetch work items from a namespace with optional filters.
    /// Single method used for both tracking and external issue queries.
    async fn graphql_list_work_items(
        &self,
        namespace: &str,
        state: Option<&str>,
        assignee_usernames: Option<&[String]>,
        updated_after: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        let gql_state = match state {
            Some("opened") => serde_json::json!("opened"),
            Some("closed") => serde_json::json!("closed"),
            _ => serde_json::Value::Null,
        };

        loop {
            let variables = serde_json::json!({
                "path": namespace,
                "state": gql_state,
                "assigneeUsernames": assignee_usernames,
                "updatedAfter": updated_after,
                "after": cursor,
            });
            let body = serde_json::json!({
                "query": Self::WORK_ITEM_QUERY,
                "variables": variables,
            });
            let json = self.graphql_post(&body).await?;
            let resp: GqlResponse<GqlNamespaceData> =
                serde_json::from_value(json).context("failed to deserialize work items")?;

            let connection = resp.data.namespace.work_items;
            all.extend(connection.nodes.into_iter().map(Issue::from));

            match connection.page_info {
                Some(pi) if pi.has_next_page => cursor = pi.end_cursor,
                _ => break,
            }
        }
        Ok(all)
    }

    /// Update a work item (issue) via GraphQL `workItemUpdate` mutation.
    /// `input` should contain the widget fields to update (e.g. `assigneesWidget`,
    /// `labelsWidget`, `stateEvent`). The `id` field is added automatically.
    pub async fn update_issue(&self, issue_id: u64, input: serde_json::Value) -> Result<Issue> {
        let gid = format!("gid://gitlab/WorkItem/{issue_id}");
        let mut full_input = input;
        full_input["id"] = serde_json::json!(gid);

        let query = r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {
                workItemUpdate(input: $input) {
                    errors
                    workItem {
                        id iid title state
                        author { id username name webUrl }
                        createdAt updatedAt closedAt webUrl
                        reference(full: true)
                        namespace { fullPath }
                        widgets(onlyTypes: [STATUS, ASSIGNEES, LABELS, MILESTONE, DESCRIPTION, ITERATION, WEIGHT]) {
                            ... on WorkItemWidgetAssignees {
                                assignees { nodes { id username name webUrl } }
                            }
                            ... on WorkItemWidgetLabels {
                                labels { nodes { title } }
                            }
                            ... on WorkItemWidgetMilestone {
                                milestone { id title state }
                            }
                            ... on WorkItemWidgetStatus {
                                status { name category }
                            }
                            ... on WorkItemWidgetDescription {
                                description
                            }
                            ... on WorkItemWidgetIteration {
                                iteration { id title startDate dueDate state }
                            }
                            ... on WorkItemWidgetWeight {
                                weight
                            }
                        }
                    }
                }
            }
        ";

        let body = serde_json::json!({ "query": query, "variables": { "input": full_input } });
        let json = self.graphql_post(&body).await?;

        // Check for mutation-level errors
        if let Some(errors) = json
            .pointer("/data/workItemUpdate/errors")
            .and_then(|v| v.as_array())
            && !errors.is_empty()
        {
            let msgs: Vec<String> = errors
                .iter()
                .filter_map(|e| e.as_str().map(std::string::ToString::to_string))
                .collect();
            anyhow::bail!("{}", msgs.join(", "));
        }

        let work_item: GqlWorkItem = serde_json::from_value(
            json.pointer("/data/workItemUpdate/workItem")
                .cloned()
                .context("missing workItem in mutation response")?,
        )
        .context("failed to deserialize workItem from mutation response")?;

        Ok(Issue::from(work_item))
    }

    pub async fn update_mr(
        &self,
        project: &str,
        iid: u64,
        payload: serde_json::Value,
    ) -> Result<MergeRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.put(&url).json(&payload).send().await?;
        Self::handle_response(resp).await
    }

    pub async fn create_issue_note(&self, project: &str, iid: u64, body: &str) -> Result<Note> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    #[allow(dead_code)]
    pub async fn list_issue_notes(&self, project: &str, iid: u64) -> Result<Vec<Note>> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("sort", "asc"), ("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn list_issue_discussions(&self, project: &str, iid: u64) -> Result<Vec<Discussion>> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}/discussions",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("sort", "asc"), ("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn reply_to_issue_discussion(
        &self,
        project: &str,
        iid: u64,
        discussion_id: &str,
        body: &str,
    ) -> Result<Note> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}/discussions/{}/notes",
            Self::encode_project(project),
            iid,
            discussion_id
        ));
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    // ── Merge Requests (REST, single-item) ──

    #[allow(dead_code)]
    pub async fn get_mr(&self, project: &str, iid: u64) -> Result<MergeRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    pub async fn approve_mr(&self, project: &str, iid: u64) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/approve",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.post(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Approve failed ({status}): {body}");
        }
        Ok(())
    }

    pub async fn merge_mr(&self, project: &str, iid: u64) -> Result<MergeRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/merge",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .put(&url)
            .json(&serde_json::json!({"should_remove_source_branch": true}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn create_mr_note(&self, project: &str, iid: u64, body: &str) -> Result<Note> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    #[allow(dead_code)]
    pub async fn list_mr_notes(&self, project: &str, iid: u64) -> Result<Vec<Note>> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("sort", "asc"), ("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn list_mr_discussions(&self, project: &str, iid: u64) -> Result<Vec<Discussion>> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/discussions",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("sort", "asc"), ("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn reply_to_mr_discussion(
        &self,
        project: &str,
        iid: u64,
        discussion_id: &str,
        body: &str,
    ) -> Result<Note> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/discussions/{}/notes",
            Self::encode_project(project),
            iid,
            discussion_id
        ));
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    #[allow(dead_code)]
    pub async fn get_mr_approvals(&self, project: &str, iid: u64) -> Result<MergeRequestApprovals> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/approvals",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    // ── Issue Status (GraphQL) ──

    fn graphql_url(&self) -> String {
        format!("{}/api/graphql", self.base_url)
    }

    async fn graphql_post(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let resp = self
            .client
            .post(self.graphql_url())
            .json(body)
            .send()
            .await?;
        let json: serde_json::Value = Self::handle_response(resp).await?;
        // Surface top-level GraphQL errors
        if let Some(errors) = json.get("errors").and_then(|v| v.as_array())
            && !errors.is_empty()
        {
            let msgs: Vec<String> = errors
                .iter()
                .filter_map(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .map(std::string::ToString::to_string)
                })
                .collect();
            anyhow::bail!("GraphQL: {}", msgs.join(", "));
        }
        Ok(json)
    }

    /// Fetch available work item statuses for a project via GraphQL.
    pub async fn fetch_work_item_statuses(&self, project: &str) -> Result<Vec<WorkItemStatus>> {
        let query = r"
            query fetchStatuses($path: ID!) {
                namespace(fullPath: $path) {
                    workItemTypes(name: ISSUE) {
                        nodes {
                            widgetDefinitions {
                                type
                                ... on WorkItemWidgetDefinitionStatus {
                                    allowedStatuses { id name category color iconName position }
                                }
                            }
                        }
                    }
                }
            }
        ";
        let variables = serde_json::json!({ "path": project });
        let body = serde_json::json!({ "query": query, "variables": variables });
        let json = self.graphql_post(&body).await?;

        // Walk the response to find the STATUS widget definition
        // The shape is: data.namespace.workItemTypes.nodes[].widgetDefinitions[]
        // We look for the one with allowedStatuses
        let nodes = json
            .pointer("/data/namespace/workItemTypes/nodes")
            .and_then(|v| v.as_array());
        if let Some(nodes) = nodes {
            for type_node in nodes {
                if let Some(widgets) = type_node
                    .get("widgetDefinitions")
                    .and_then(|v| v.as_array())
                {
                    for widget in widgets {
                        if let Some(statuses_val) = widget.get("allowedStatuses") {
                            let statuses: Vec<GqlAllowedStatus> =
                                serde_json::from_value(statuses_val.clone())?;
                            if !statuses.is_empty() {
                                return Ok(statuses
                                    .into_iter()
                                    .map(WorkItemStatus::from)
                                    .collect());
                            }
                        }
                    }
                }
            }
        }

        Ok(Vec::new())
    }

    /// Update a work item's status via GraphQL.
    pub async fn update_issue_status(&self, issue_id: u64, status_id: &str) -> Result<()> {
        let query = r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {
                workItemUpdate(input: $input) {
                    errors
                }
            }
        ";
        let gid = format!("gid://gitlab/WorkItem/{issue_id}");
        let variables = serde_json::json!({
            "input": {
                "id": gid,
                "statusWidget": {
                    "status": status_id
                }
            }
        });
        let body = serde_json::json!({ "query": query, "variables": variables });

        let json = self.graphql_post(&body).await?;

        if let Some(errors) = json
            .pointer("/data/workItemUpdate/errors")
            .and_then(|v| v.as_array())
            && !errors.is_empty()
        {
            let msgs: Vec<String> = errors
                .iter()
                .filter_map(|e| e.as_str().map(std::string::ToString::to_string))
                .collect();
            anyhow::bail!("{}", msgs.join(", "));
        }

        Ok(())
    }

    // ── Iterations ──

    /// Fetch iterations for the tracking namespace.
    pub async fn fetch_iterations(&self) -> Result<Vec<Iteration>> {
        let query = r"
            query($path: ID!, $after: String) {
                group(fullPath: $path) {
                    iterations(
                        first: 50
                        sort: CADENCE_AND_DUE_DATE_ASC
                        after: $after
                    ) {
                        nodes { id title startDate dueDate state }
                        pageInfo { hasNextPage endCursor }
                    }
                }
            }
        ";

        #[derive(Deserialize)]
        struct Resp {
            group: Group,
        }
        #[derive(Deserialize)]
        struct Group {
            iterations: GqlConnection<GqlIteration>,
        }

        // Extract the group path from primary tracking project (everything before the last `/`)
        let primary = self.config.primary_tracking_project();
        let group_path = primary.rsplit_once('/').map_or(primary, |(g, _)| g);

        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let variables = serde_json::json!({
                "path": group_path,
                "after": cursor,
            });
            let body = serde_json::json!({ "query": query, "variables": variables });
            let json = self.graphql_post(&body).await?;

            let resp: GqlResponse<Resp> =
                serde_json::from_value(json).context("failed to deserialize iterations")?;

            let connection = resp.data.group.iterations;
            for gi in connection.nodes {
                all.push(Iteration {
                    id: gi.id,
                    title: gi.title.unwrap_or_default(),
                    start_date: gi.start_date,
                    due_date: gi.due_date,
                    state: gi.state,
                });
            }

            match connection.page_info {
                Some(pi) if pi.has_next_page => cursor = pi.end_cursor,
                _ => break,
            }
        }

        Ok(all)
    }

    /// Update a work item's iteration via GraphQL.
    pub async fn update_issue_iteration(
        &self,
        issue_id: u64,
        iteration_gid: Option<&str>,
    ) -> Result<()> {
        let query = r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {
                workItemUpdate(input: $input) {
                    errors
                }
            }
        ";
        let gid = format!("gid://gitlab/WorkItem/{issue_id}");
        let variables = serde_json::json!({
            "input": {
                "id": gid,
                "iterationWidget": {
                    "iterationId": iteration_gid,
                }
            }
        });
        let body = serde_json::json!({ "query": query, "variables": variables });

        let json = self.graphql_post(&body).await?;

        if let Some(errors) = json
            .pointer("/data/workItemUpdate/errors")
            .and_then(|v| v.as_array())
            && !errors.is_empty()
        {
            let msgs: Vec<String> = errors
                .iter()
                .filter_map(|e| e.as_str().map(std::string::ToString::to_string))
                .collect();
            anyhow::bail!("{}", msgs.join(", "));
        }

        Ok(())
    }

    // ── Labels ──

    pub async fn list_project_labels(&self, project: &str) -> Result<Vec<ProjectLabel>> {
        let url = self.api_url(&format!(
            "/projects/{}/labels",
            Self::encode_project(project)
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    // ── Authenticated User ──

    pub async fn get_authenticated_user(&self) -> Result<serde_json::Value> {
        let url = self.api_url("/user");
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    // ── Members / Users ──

    pub async fn search_users(&self, query: &str) -> Result<Vec<User>> {
        let url = self.api_url("/users");
        let resp = self
            .client
            .get(&url)
            .query(&[("search", query), ("per_page", "20")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    // ── Fetch all data for dashboard ──

    /// Fetch all issues from the tracking namespaces via `namespace.workItems`.
    pub async fn fetch_tracking_issues(
        &self,
        state: Option<&str>,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedIssue>> {
        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        for project in &self.config.tracking_projects {
            let issues = self
                .graphql_list_work_items(project, state, None, updated_after)
                .await?;

            for issue in issues {
                let project_path = issue.references.as_ref().map_or_else(
                    || project.clone(),
                    |r| extract_project_from_ref(&r.full_ref),
                );
                if seen_ids.insert(issue.id) {
                    all.push(TrackedIssue {
                        issue,
                        project_path,
                    });
                }
            }
        }
        Ok(all)
    }

    /// Fetch issues assigned to team members outside the tracking namespace.
    /// Uses root `issues(assigneeUsernames: [...])` query.
    pub async fn fetch_assigned_issues(
        &self,
        members: &[String],
        state: Option<&str>,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedIssue>> {
        let gql_state = match state {
            Some("opened") => serde_json::json!("opened"),
            Some("closed") => serde_json::json!("closed"),
            _ => serde_json::Value::Null,
        };

        let query = r"
            query($assigneeUsernames: [String!], $state: IssuableState, $types: [IssueType!], $after: String, $updatedAfter: Time) {
                issues(
                    assigneeUsernames: $assigneeUsernames
                    state: $state
                    types: $types
                    after: $after
                    updatedAfter: $updatedAfter
                    first: 100
                    sort: UPDATED_DESC
                ) {
                    nodes {
                        id iid title state
                        author { id username name webUrl }
                        assignees { nodes { id username name webUrl } }
                        labels { nodes { title } }
                        milestone { id title state }
                        createdAt updatedAt closedAt webUrl description
                        reference(full: true)
                        status { name }
                        iteration { id title startDate dueDate state }
                        weight
                    }
                    pageInfo { hasNextPage endCursor }
                }
            }
        ";

        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for member in members {
            let mut cursor: Option<String> = None;
            loop {
                let variables = serde_json::json!({
                    "assigneeUsernames": member,
                    "state": gql_state,
                    "types": ["ISSUE"],
                    "after": cursor,
                    "updatedAfter": updated_after,
                });
                let body = serde_json::json!({ "query": query, "variables": variables });
                let json = self.graphql_post(&body).await?;
                let resp: GqlResponse<GqlRootIssuesData> = serde_json::from_value(json)
                    .context("failed to deserialize assigned issues")?;

                let connection = resp.data.issues;
                for issue in connection.nodes.into_iter().map(Issue::from) {
                    let project_path = issue
                        .references
                        .as_ref()
                        .map(|r| extract_project_from_ref(&r.full_ref))
                        .unwrap_or_default();

                    // Skip tracking project issues and duplicates
                    if self.config.is_tracking_project(&project_path) || !seen_ids.insert(issue.id)
                    {
                        continue;
                    }

                    all.push(TrackedIssue {
                        issue,
                        project_path,
                    });
                }

                match connection.page_info {
                    Some(pi) if pi.has_next_page => cursor = pi.end_cursor,
                    _ => break,
                }
            }
        }

        Ok(all)
    }

    // ── Merge Requests (GraphQL) ──

    const MR_FIELDS: &str = r"
        id iid title state draft
        author { id username name webUrl }
        assignees { nodes { id username name webUrl } }
        reviewers { nodes { id username name webUrl } }
        labels { nodes { title } }
        milestone { id title state }
        createdAt updatedAt closedAt webUrl description
        userNotesCount
        sourceBranch targetBranch mergeStatusEnum
        reference(full: true)
        diffStatsSummary { additions deletions fileCount }
        approved
        approvedBy { nodes { id username name webUrl } }
        headPipeline { status }
        resolvableDiscussionsCount
        resolvedDiscussionsCount
    ";

    /// Page size for MR list queries. Kept small to stay within GitLab's
    /// default query complexity limit of 250 (each MR node with nested
    /// discussions contributes ~5 points).
    const MR_PAGE_SIZE: u32 = 25;

    async fn graphql_list_project_mrs(
        &self,
        project: &str,
        state: Option<&str>,
        updated_after: Option<&str>,
    ) -> Result<Vec<MergeRequest>> {
        let gql_state = match state {
            Some("opened") => serde_json::json!("opened"),
            Some("merged") => serde_json::json!("merged"),
            Some("closed") => serde_json::json!("closed"),
            _ => serde_json::Value::Null,
        };

        let query = format!(
            r"
            query($projectPath: ID!, $state: MergeRequestState, $updatedAfter: Time, $after: String) {{
                project(fullPath: $projectPath) {{
                    mergeRequests(
                        state: $state
                        updatedAfter: $updatedAfter
                        after: $after
                        first: {page_size}
                        sort: UPDATED_DESC
                    ) {{
                        nodes {{ {fields} }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}
            ",
            page_size = Self::MR_PAGE_SIZE,
            fields = Self::MR_FIELDS
        );

        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let variables = serde_json::json!({
                "projectPath": project,
                "state": gql_state,
                "updatedAfter": updated_after,
                "after": cursor,
            });
            let body = serde_json::json!({ "query": query, "variables": variables });
            let json = self.graphql_post(&body).await?;
            let resp: GqlResponse<GqlProjectMrData> =
                serde_json::from_value(json).context("failed to deserialize project MRs")?;

            let Some(proj) = resp.data.project else {
                break;
            };

            let connection = proj.merge_requests;
            all.extend(connection.nodes.into_iter().map(MergeRequest::from));

            match connection.page_info {
                Some(pi) if pi.has_next_page => cursor = pi.end_cursor,
                _ => break,
            }
        }
        Ok(all)
    }

    pub async fn fetch_tracking_mrs(
        &self,
        state: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedMergeRequest>> {
        let gql_state = if state == "all" { None } else { Some(state) };
        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        for project in &self.config.tracking_projects {
            let mrs = self
                .graphql_list_project_mrs(project, gql_state, updated_after)
                .await?;
            for mr in mrs {
                let project_path = mr.references.as_ref().map_or_else(
                    || project.clone(),
                    |r| extract_project_from_ref(&r.full_ref),
                );
                if seen_ids.insert(mr.id) {
                    all.push(TrackedMergeRequest { mr, project_path });
                }
            }
        }
        Ok(all)
    }

    pub async fn fetch_external_mrs(
        &self,
        members: &[String],
        state: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedMergeRequest>> {
        let gql_state = match state {
            "opened" => serde_json::json!("opened"),
            "merged" => serde_json::json!("merged"),
            "closed" => serde_json::json!("closed"),
            _ => serde_json::Value::Null,
        };

        let assigned_query = format!(
            r"
            query($username: String!, $state: MergeRequestState, $after: String, $updatedAfter: Time) {{
                user(username: $username) {{
                    assignedMergeRequests(state: $state, after: $after, updatedAfter: $updatedAfter, first: {page_size}, sort: UPDATED_DESC) {{
                        nodes {{ {fields} }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}
            ",
            page_size = Self::MR_PAGE_SIZE,
            fields = Self::MR_FIELDS
        );

        let reviewer_query = format!(
            r"
            query($username: String!, $state: MergeRequestState, $after: String, $updatedAfter: Time) {{
                user(username: $username) {{
                    reviewRequestedMergeRequests(state: $state, after: $after, updatedAfter: $updatedAfter, first: {page_size}, sort: UPDATED_DESC) {{
                        nodes {{ {fields} }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}
            ",
            page_size = Self::MR_PAGE_SIZE,
            fields = Self::MR_FIELDS
        );

        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        let queries = [&assigned_query, &reviewer_query];
        for member in members {
            for query in &queries {
                for item in self
                    .fetch_user_mrs_page(query, member, &gql_state, updated_after)
                    .await?
                {
                    if !self.config.is_tracking_project(&item.project_path)
                        && seen_ids.insert(item.mr.id)
                    {
                        all.push(item);
                    }
                }
            }
        }
        Ok(all)
    }

    /// Paginate a per-user MR query and return the collected results.
    async fn fetch_user_mrs_page(
        &self,
        query: &str,
        member: &str,
        state: &serde_json::Value,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedMergeRequest>> {
        let mut results = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let variables = serde_json::json!({
                "username": member,
                "state": state,
                "after": cursor,
                "updatedAfter": updated_after,
            });
            let body = serde_json::json!({ "query": query, "variables": variables });
            let json = self.graphql_post(&body).await?;

            let resp: GqlResponse<GqlUserMrData> =
                serde_json::from_value(json).context("failed to deserialize user MRs")?;
            let Some(user) = resp.data.user else {
                break;
            };

            let has_next = user
                .mrs
                .page_info
                .as_ref()
                .is_some_and(|pi| pi.has_next_page);

            for mr in user.mrs.nodes.into_iter().map(MergeRequest::from) {
                let project_path = mr
                    .references
                    .as_ref()
                    .map(|r| extract_project_from_ref(&r.full_ref))
                    .unwrap_or_default();
                results.push(TrackedMergeRequest { mr, project_path });
            }

            if has_next {
                cursor = user.mrs.page_info.and_then(|pi| pi.end_cursor);
            } else {
                break;
            }
        }
        Ok(results)
    }

    // ── Iteration health: unplanned work & shadow work ──

    /// GraphQL query to fetch system notes for a work item (for iteration change detection).
    const WORK_ITEM_NOTES_QUERY: &str = r"
        query($fullPath: ID!, $iid: String!) {
            workspace: namespace(fullPath: $fullPath) {
                workItem(iid: $iid) {
                    widgets(onlyTypes: [NOTES]) {
                        ... on WorkItemWidgetNotes {
                            discussions(first: 100, filter: ONLY_ACTIVITY) {
                                nodes {
                                    notes {
                                        nodes {
                                            system
                                            systemNoteIconName
                                            body
                                            createdAt
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    ";

    /// Fetch the timestamp when an issue was added to its current iteration,
    /// by looking for system notes with `systemNoteIconName == "iteration"`.
    /// Returns the `createdAt` of the *last* matching note (most recent assignment).
    pub async fn fetch_work_item_iteration_added_at(
        &self,
        namespace: &str,
        iid: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        let body = serde_json::json!({
            "query": Self::WORK_ITEM_NOTES_QUERY,
            "variables": { "fullPath": namespace, "iid": iid },
        });
        let json = self.graphql_post(&body).await?;

        // Navigate: data.workspace.workItem.widgets[0].discussions.nodes[].notes.nodes[]
        let discussions = json
            .pointer("/data/workspace/workItem/widgets")
            .and_then(|w| w.as_array())
            .and_then(|widgets| {
                widgets
                    .iter()
                    .find_map(|w| w.pointer("/discussions/nodes").and_then(|n| n.as_array()))
            });

        let Some(discussions) = discussions else {
            return Ok(None);
        };

        let mut latest: Option<DateTime<Utc>> = None;

        for disc in discussions {
            let notes = disc.pointer("/notes/nodes").and_then(|n| n.as_array());
            let Some(notes) = notes else { continue };
            for note in notes {
                let is_system = note
                    .get("system")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let icon = note
                    .get("systemNoteIconName")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if is_system
                    && icon == "iteration"
                    && let Some(ts_str) = note.get("createdAt").and_then(serde_json::Value::as_str)
                {
                    // GitLab returns ISO 8601 with timezone
                    if let Ok(dt) = DateTime::<FixedOffset>::parse_from_rfc3339(ts_str) {
                        let utc = dt.with_timezone(&Utc);
                        if latest.is_none_or(|prev| utc > prev) {
                            latest = Some(utc);
                        }
                    }
                }
            }
        }

        Ok(latest)
    }

    /// Batch-fetch "added to iteration" timestamps for multiple issues.
    /// Uses a semaphore to limit concurrency.
    pub async fn fetch_iteration_added_dates_batch(
        &self,
        items: Vec<(String, String, u64)>, // (namespace, iid_str, issue_id)
    ) -> Result<std::collections::HashMap<u64, DateTime<Utc>>> {
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::Semaphore;

        let sem = Arc::new(Semaphore::new(5));
        let mut handles = Vec::with_capacity(items.len());

        for (namespace, iid, issue_id) in items {
            let client = self.clone();
            let permit = Arc::clone(&sem);
            handles.push(tokio::spawn(async move {
                let _permit = permit.acquire().await;
                let result = client
                    .fetch_work_item_iteration_added_at(&namespace, &iid)
                    .await;
                (issue_id, result)
            }));
        }

        let mut map: HashMap<u64, DateTime<Utc>> = HashMap::new();
        for handle in handles {
            if let Ok((issue_id, Ok(Some(dt)))) = handle.await {
                map.insert(issue_id, dt);
            }
        }
        Ok(map)
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        if !status.is_success() {
            let url = resp.url().to_string();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{status} from {url}: {body}");
        }
        resp.json::<T>()
            .await
            .context("Failed to parse GitLab response")
    }
}

/// Extract project path from a full reference like "myorg/myrepo#123" or "myorg/myrepo!45"
fn extract_project_from_ref(full_ref: &str) -> String {
    // Full refs look like "group/project#123" or "group/subgroup/project!45"
    if let Some(idx) = full_ref.rfind(['#', '!']) {
        full_ref[..idx].to_string()
    } else {
        full_ref.to_string()
    }
}
