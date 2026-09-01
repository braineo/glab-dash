use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Utc};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::Deserialize;

use crate::config::Config;
use glab_core::domain::{
    Discussion, Issue, Iteration, MergeRequest, Milestone, Note, ProjectLabel, StatusValue,
    TrackedIssue, TrackedMergeRequest, User, WorkItemStatus,
};

// ── GraphQL response types (serde-driven) ──

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
    iid: String,
    title: String,
    state: String,
    #[serde(default)]
    author: Option<User>,
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
struct GqlLabel {
    title: String,
}

/// Serde flattens all widget types into one struct.
/// Unknown fields are ignored; each widget type only populates its fields.
#[derive(Deserialize, Default)]
struct GqlWidget {
    #[serde(default)]
    assignees: Option<GqlConnection<User>>,
    #[serde(default)]
    labels: Option<GqlConnection<GqlLabel>>,
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

#[derive(Deserialize)]
struct GqlAllowedStatus {
    id: String,
    name: String,
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
    merge_requests: GqlConnection<MergeRequest>,
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
    mrs: GqlConnection<MergeRequest>,
}

// ── Root issues query (for assigned issues outside tracking namespace) ──

#[derive(Deserialize)]
struct GqlRootIssuesData {
    issues: GqlConnection<Issue>,
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

    fn work_item_query() -> String {
        let doc = r"
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
                        ...WorkItemFields
                    }
                    pageInfo { hasNextPage endCursor }
                }
            }
        }
        ";
        format!("{doc}{}", Self::fragments(Self::WORK_ITEM_FIELDS))
    }

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
                "query": Self::work_item_query(),
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
    pub async fn update_issue(&self, gid: &str, input: serde_json::Value) -> Result<Issue> {
        let mut full_input = input;
        full_input["id"] = serde_json::json!(gid);

        let query = format!(
            "{doc}{}",
            Self::fragments(Self::WORK_ITEM_FIELDS),
            doc = r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {
                workItemUpdate(input: $input) {
                    errors
                    workItem {
                        ...WorkItemFields
                    }
                }
            }
            "
        );

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

    async fn mr_mutation(
        &self,
        mutation: &str,
        input_type: &str,
        project: &str,
        iid: &str,
        mut input: serde_json::Value,
    ) -> Result<MergeRequest> {
        input["projectPath"] = serde_json::json!(project);
        input["iid"] = serde_json::json!(iid);

        let query = format!(
            r"
            mutation($input: {input_type}!) {{
                {mutation}(input: $input) {{
                    errors
                    mergeRequest {{ ...MrFields }}
                }}
            }}
            {fragments}
            ",
            fragments = Self::fragments(Self::MR_FIELDS)
        );

        let body = serde_json::json!({ "query": query, "variables": { "input": input } });
        let json = self.graphql_post(&body).await?;

        if let Some(errors) = json
            .pointer(&format!("/data/{mutation}/errors"))
            .and_then(|v| v.as_array())
            && !errors.is_empty()
        {
            let msgs: Vec<String> = errors
                .iter()
                .filter_map(|e| e.as_str().map(std::string::ToString::to_string))
                .collect();
            anyhow::bail!("{}", msgs.join(", "));
        }

        let mr = json
            .pointer(&format!("/data/{mutation}/mergeRequest"))
            .cloned()
            .with_context(|| format!("missing mergeRequest in {mutation} response"))?;
        serde_json::from_value(mr)
            .with_context(|| format!("failed to deserialize {mutation} response"))
    }

    pub async fn close_mr(&self, project: &str, iid: &str) -> Result<MergeRequest> {
        self.mr_mutation(
            "mergeRequestUpdate",
            "MergeRequestUpdateInput",
            project,
            iid,
            serde_json::json!({ "state": "CLOSED" }),
        )
        .await
    }

    pub async fn set_mr_assignees(
        &self,
        project: &str,
        iid: &str,
        usernames: &[String],
    ) -> Result<MergeRequest> {
        self.mr_mutation(
            "mergeRequestSetAssignees",
            "MergeRequestSetAssigneesInput",
            project,
            iid,
            serde_json::json!({ "assigneeUsernames": usernames }),
        )
        .await
    }

    pub async fn set_mr_labels(
        &self,
        project: &str,
        iid: &str,
        label_ids: &[u64],
    ) -> Result<MergeRequest> {
        let gids: Vec<String> = label_ids
            .iter()
            .map(|id| format!("gid://gitlab/Label/{id}"))
            .collect();
        self.mr_mutation(
            "mergeRequestSetLabels",
            "MergeRequestSetLabelsInput",
            project,
            iid,
            serde_json::json!({ "labelIds": gids }),
        )
        .await
    }

    pub async fn create_issue_note(&self, project: &str, iid: &str, body: &str) -> Result<Note> {
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

    pub async fn list_issue_discussions(
        &self,
        project: &str,
        iid: &str,
    ) -> Result<Vec<Discussion>> {
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
        iid: &str,
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

    pub async fn approve_mr(&self, project: &str, iid: &str) -> Result<()> {
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

    pub async fn merge_mr(&self, project: &str, iid: &str) -> Result<()> {
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
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Merge failed ({status}): {body}");
        }
        Ok(())
    }

    pub async fn create_mr_note(&self, project: &str, iid: &str, body: &str) -> Result<Note> {
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

    pub async fn list_mr_discussions(&self, project: &str, iid: &str) -> Result<Vec<Discussion>> {
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
        iid: &str,
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

    // ── Issue Status (GraphQL) ──

    fn graphql_url(&self) -> String {
        format!("{}/api/graphql", self.base_url)
    }

    async fn graphql_post(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let op = body
            .get("query")
            .and_then(|v| v.as_str())
            .and_then(|q| q.split_whitespace().nth(1))
            .unwrap_or("<anon>")
            .to_string();
        let vars_preview = body
            .get("variables")
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        let started = std::time::Instant::now();
        tracing::debug!(op = %op, vars = %vars_preview, "graphql_post →");
        let resp = match self.client.post(self.graphql_url()).json(body).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(op = %op, error = %e, elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX), "graphql_post ✗ network");
                return Err(e.into());
            }
        };
        let json: serde_json::Value = match Self::handle_response(resp).await {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(op = %op, error = ?e, elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX), "graphql_post ✗ http");
                return Err(e);
            }
        };
        tracing::debug!(op = %op, elapsed_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX), "graphql_post ✓");
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
                                    allowedStatuses { id name category position }
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
    pub async fn update_issue_status(&self, gid: &str, status_id: &str) -> Result<()> {
        let query = r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {
                workItemUpdate(input: $input) {
                    errors
                }
            }
        ";
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
            iterations: GqlConnection<Iteration>,
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
                    title: gi.title,
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
        gid: &str,
        iteration_gid: Option<&str>,
    ) -> Result<()> {
        let query = r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {
                workItemUpdate(input: $input) {
                    errors
                }
            }
        ";
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
        state: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedIssue>> {
        let gql_state = if state == "all" { None } else { Some(state) };
        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        for project in &self.config.tracking_projects {
            let issues = self
                .graphql_list_work_items(project, gql_state, None, updated_after)
                .await?;

            for issue in issues {
                let project_path = issue
                    .reference
                    .as_deref()
                    .map_or_else(|| project.clone(), extract_project_from_ref);
                if seen_ids.insert(issue.id.clone()) {
                    all.push(TrackedIssue {
                        issue,
                        project_path,
                    });
                }
            }
        }
        Ok(all)
    }

    /// Fetch updated issues from specific external projects.
    /// Used to sync state for issues we already track that live outside
    /// the tracking namespace (e.g. issues assigned to team members in
    /// other repos that may later be reassigned or closed).
    pub async fn fetch_external_project_issues(
        &self,
        projects: &[String],
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedIssue>> {
        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        for project in projects {
            let issues = self
                .graphql_list_work_items(project, None, None, updated_after)
                .await?;
            for issue in issues {
                let project_path = issue
                    .reference
                    .as_deref()
                    .map_or_else(|| project.clone(), extract_project_from_ref);
                if seen_ids.insert(issue.id.clone()) {
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
        state: &str,
        updated_after: Option<&str>,
    ) -> Result<Vec<TrackedIssue>> {
        let gql_state = match state {
            "opened" => serde_json::json!("opened"),
            "closed" => serde_json::json!("closed"),
            _ => serde_json::Value::Null,
        };

        let query = format!(
            "{doc}{}",
            Self::fragments(Self::ISSUE_FIELDS),
            doc = r"
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
                        ...IssueFields
                    }
                    pageInfo { hasNextPage endCursor }
                }
            }
            "
        );

        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for member in members {
            let mut cursor: Option<String> = None;
            loop {
                let variables = serde_json::json!({
                    "assigneeUsernames": vec![member],
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
                for issue in connection.nodes {
                    let project_path = issue
                        .reference
                        .as_deref()
                        .map(extract_project_from_ref)
                        .unwrap_or_default();

                    // Skip tracking project issues and duplicates
                    if self.config.is_tracking_project(&project_path)
                        || !seen_ids.insert(issue.id.clone())
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

    /// The selection every merge-request query and mutation shares
    const USER_FRAGMENT: &str = r"
        fragment UserFields on User {
            id username name webUrl
        }
    ";

    /// The selection every merge-request query and mutation shares
    const MR_FIELDS: &str = r"
        fragment MrFields on MergeRequest {
            id iid title state draft
            author { ...UserFields }
            assignees { nodes { ...UserFields } }
            reviewers { nodes { ...UserFields } }
            labels { nodes { title } }
            milestone { title }
            createdAt updatedAt webUrl description
            userNotesCount
            sourceBranch targetBranch
            reference(full: true)
            diffStatsSummary { additions deletions fileCount }
            approved
            approvedBy { nodes { ...UserFields } }
            headPipeline { status }
            resolvableDiscussionsCount
            resolvedDiscussionsCount
        }
    ";

    /// The selection the root `issues` query uses, deserialized straight into
    /// [`Issue`].
    const ISSUE_FIELDS: &str = r"
        fragment IssueFields on Issue {
            id iid title state
            author { ...UserFields }
            assignees { nodes { ...UserFields } }
            labels { nodes { title } }
            milestone { title }
            createdAt updatedAt closedAt webUrl description
            userNotesCount
            reference(full: true)
            status { name category }
            iteration { id title startDate dueDate state }
            weight
        }
    ";

    /// The selection both work-item documents share — the `namespace.workItems`
    /// list query and the `workItemUpdate` mutation. Widgets arrive as a
    /// heterogeneous array, so this path still needs `GqlWorkItem` and
    /// `From<GqlWorkItem> for Issue` to fold into the flat [`Issue`] shape.
    const WORK_ITEM_FIELDS: &str = r"
        fragment WorkItemFields on WorkItem {
            id iid title state
            author { ...UserFields }
            createdAt updatedAt closedAt webUrl
            reference(full: true)
            namespace { fullPath }
            widgets(onlyTypes: [STATUS, ASSIGNEES, LABELS, MILESTONE, DESCRIPTION, ITERATION, WEIGHT]) {
                ... on WorkItemWidgetAssignees {
                    assignees { nodes { ...UserFields } }
                }
                ... on WorkItemWidgetLabels {
                    labels { nodes { title } }
                }
                ... on WorkItemWidgetMilestone {
                    milestone { title }
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
    ";

    /// The fragment text to append to a document spreading `fields`, together
    /// with the `UserFields` fragment every one of them spreads in turn.
    fn fragments(fields: &str) -> String {
        format!("{fields}{}", Self::USER_FRAGMENT)
    }

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
                        nodes {{ ...MrFields }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}
            {fragments}
            ",
            page_size = Self::MR_PAGE_SIZE,
            fragments = Self::fragments(Self::MR_FIELDS)
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
            all.extend(connection.nodes);

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
                let project_path = mr
                    .reference
                    .as_deref()
                    .map_or_else(|| project.clone(), extract_project_from_ref);
                if seen_ids.insert(mr.id.clone()) {
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
                        nodes {{ ...MrFields }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}
            {fragments}
            ",
            page_size = Self::MR_PAGE_SIZE,
            fragments = Self::fragments(Self::MR_FIELDS)
        );

        let reviewer_query = format!(
            r"
            query($username: String!, $state: MergeRequestState, $after: String, $updatedAfter: Time) {{
                user(username: $username) {{
                    reviewRequestedMergeRequests(state: $state, after: $after, updatedAfter: $updatedAfter, first: {page_size}, sort: UPDATED_DESC) {{
                        nodes {{ ...MrFields }}
                        pageInfo {{ hasNextPage endCursor }}
                    }}
                }}
            }}
            {fragments}
            ",
            page_size = Self::MR_PAGE_SIZE,
            fragments = Self::fragments(Self::MR_FIELDS)
        );

        let mut all = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        let queries = [("assigned", &assigned_query), ("reviewer", &reviewer_query)];
        tracing::info!(
            members = members.len(),
            updated_after = ?updated_after,
            "fetch_external_mrs start"
        );
        let overall = std::time::Instant::now();
        for member in members {
            for (kind, query) in &queries {
                let t = std::time::Instant::now();
                let page = self
                    .fetch_user_mrs_page(query, member, &gql_state, updated_after)
                    .await;
                match page {
                    Ok(items) => {
                        tracing::debug!(
                            member = %member,
                            kind = %kind,
                            count = items.len(),
                            elapsed_ms = t.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                            "fetch_user_mrs_page ✓"
                        );
                        for item in items {
                            if !self.config.is_tracking_project(&item.project_path)
                                && seen_ids.insert(item.mr.id.clone())
                            {
                                all.push(item);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            member = %member,
                            kind = %kind,
                            error = ?e,
                            elapsed_ms = t.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                            "fetch_user_mrs_page ✗"
                        );
                        return Err(e);
                    }
                }
            }
        }
        tracing::info!(
            total = all.len(),
            elapsed_ms = overall.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            "fetch_external_mrs done"
        );
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

            for mr in user.mrs.nodes {
                let project_path = mr
                    .reference
                    .as_deref()
                    .map(extract_project_from_ref)
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
        items: Vec<(String, String, String)>, // (namespace, iid_str, issue_id)
    ) -> Result<std::collections::HashMap<String, DateTime<Utc>>> {
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

        let mut map: HashMap<String, DateTime<Utc>> = HashMap::new();
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
