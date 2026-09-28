//! `namespace.workItems` walks a namespace and its descendants; the root
//! `issues` query finds issues by assignee anywhere on the instance.  They
//! report the same issue under different global ids, which
//! `glab_core::de::work_item_gid` normalizes.

use anyhow::{Context, Result};
use serde_json::Value;
use strum::IntoStaticStr;

use glab_core::domain::Issue;

use crate::client::{GitLabClient, PAGE_SIZE, document, get_mutation_payload, join_walks};
use crate::wire::{RootIssuesQuery, WorkItem, WorkItemsQuery};

/// Deserialized straight into [`glab_core::domain::Issue`].
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

const WORK_ITEM_FIELDS: &str = r"
    fragment WorkItemFields on WorkItem {
        id iid title state
        author { ...UserFields }
        createdAt updatedAt closedAt webUrl
        reference(full: true)
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

/// `None` asks for every state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum IssueState {
    Opened,
    Closed,
}

impl GitLabClient {
    /// Walks each namespace's descendant projects too.  `updated_after` is an
    /// ISO 8601 timestamp restricting the walk to issues touched since.
    pub async fn list_namespace_issues(
        &self,
        namespaces: &[String],
        state: Option<IssueState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let query = document(
            r"
            query listWorkItems($path: ID!, $state: IssuableState, $updatedAfter: Time, $after: String, $first: Int) {
                namespace(fullPath: $path) {
                    workItems(
                        includeDescendants: true
                        types: [ISSUE]
                        state: $state
                        updatedAfter: $updatedAfter
                        after: $after
                        first: $first
                        sort: UPDATED_DESC
                    ) {
                        nodes { ...WorkItemFields }
                        pageInfo { hasNextPage endCursor }
                    }
                }
            }
            ",
            WORK_ITEM_FIELDS,
        );

        let mut set = tokio::task::JoinSet::new();
        for (idx, namespace) in namespaces.iter().enumerate() {
            let client = self.clone();
            let query = query.clone();
            let namespace = namespace.clone();
            let updated_after = updated_after.map(str::to_string);
            set.spawn(async move {
                let issues = client
                    .paginate::<Issue, WorkItemsQuery>("listWorkItems", &query, |after| {
                        serde_json::json!({
                            "path": namespace,
                            "state": state_value(state),
                            "updatedAfter": updated_after,
                            "after": after,
                            "first": PAGE_SIZE,
                        })
                    })
                    .await;
                (idx, issues)
            });
        }
        join_walks(set, |i| &i.id).await
    }

    /// Instance-wide, so the caller decides which projects' results to keep.
    pub async fn list_assigned_issues(
        &self,
        members: &[String],
        state: Option<IssueState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let query = document(
            r"
            query listAssignedIssues($assigneeUsernames: [String!], $state: IssuableState, $types: [IssueType!], $after: String, $updatedAfter: Time, $first: Int) {
                issues(
                    assigneeUsernames: $assigneeUsernames
                    state: $state
                    types: $types
                    after: $after
                    updatedAfter: $updatedAfter
                    first: $first
                    sort: UPDATED_DESC
                ) {
                    nodes { ...IssueFields }
                    pageInfo { hasNextPage endCursor }
                }
            }
            ",
            ISSUE_FIELDS,
        );

        let mut set = tokio::task::JoinSet::new();
        for (idx, member) in members.iter().enumerate() {
            let client = self.clone();
            let query = query.clone();
            let member = member.clone();
            let updated_after = updated_after.map(str::to_string);
            set.spawn(async move {
                let issues = client
                    .paginate::<Issue, RootIssuesQuery>("listAssignedIssues", &query, |after| {
                        serde_json::json!({
                            "assigneeUsernames": [member],
                            "state": state_value(state),
                            "types": ["ISSUE"],
                            "after": after,
                            "updatedAfter": updated_after,
                            "first": PAGE_SIZE,
                        })
                    })
                    .await;
                (idx, issues)
            });
        }
        join_walks(set, |i| &i.id).await
    }

    /// `input` carries the widget fields to change (`assigneesWidget`,
    /// `labelsWidget`, `stateEvent`); the id is filled in here.
    pub async fn update_issue(&self, gid: &str, input: Value) -> Result<Issue> {
        let json = self
            .update_work_item(input_with_id(input, gid), true)
            .await?;
        let work_item = get_mutation_payload(&json, "workItemUpdate")?
            .get("workItem")
            .context("missing workItem in mutation response")?;
        let work_item: WorkItem = serde_json::from_value(work_item.clone())
            .context("failed to deserialize workItem from mutation response")?;
        Ok(Issue::from(work_item))
    }

    /// `status_id` is one of the ids
    /// [`fetch_work_item_statuses`](Self::fetch_work_item_statuses) returned.
    pub async fn update_issue_status(&self, gid: &str, status_id: &str) -> Result<()> {
        let input = serde_json::json!({ "statusWidget": { "status": status_id } });
        let json = self
            .update_work_item(input_with_id(input, gid), false)
            .await?;
        get_mutation_payload(&json, "workItemUpdate")?;
        Ok(())
    }

    /// `None` moves it out of every iteration.
    pub async fn update_issue_iteration(
        &self,
        gid: &str,
        iteration_gid: Option<&str>,
    ) -> Result<()> {
        let input = serde_json::json!({ "iterationWidget": { "iterationId": iteration_gid } });
        let json = self
            .update_work_item(input_with_id(input, gid), false)
            .await?;
        get_mutation_payload(&json, "workItemUpdate")?;
        Ok(())
    }

    /// `read_back` selects the updated work item in the response, which costs
    /// GitLab complexity budget, so a caller discarding it leaves it off.
    async fn update_work_item(&self, input: Value, read_back: bool) -> Result<Value> {
        let selection = if read_back {
            "workItem { ...WorkItemFields }"
        } else {
            ""
        };
        let doc = format!(
            r"
            mutation workItemUpdate($input: WorkItemUpdateInput!) {{
                workItemUpdate(input: $input) {{
                    errors
                    {selection}
                }}
            }}
            "
        );
        let query = if read_back {
            document(&doc, WORK_ITEM_FIELDS)
        } else {
            doc
        };
        self.graphql_once(
            "workItemUpdate",
            &query,
            serde_json::json!({ "input": input }),
        )
        .await
    }
}

fn input_with_id(mut input: Value, gid: &str) -> Value {
    input["id"] = serde_json::json!(gid);
    input
}

fn state_value(state: Option<IssueState>) -> Value {
    state.map_or(Value::Null, |s| Value::from(<&'static str>::from(s)))
}
