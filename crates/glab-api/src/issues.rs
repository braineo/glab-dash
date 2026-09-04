//! Issue reads and writes, all over GraphQL.
//!
//! GitLab exposes an issue two ways and glab-dash uses both: `namespace.workItems`
//! walks a namespace and its descendants, and the root `issues` query finds
//! issues by assignee anywhere on the instance. The two report the same issue
//! under different global ids, which `glab_core::de::work_item_gid` normalizes
//! so the results can be merged.

use anyhow::{Context, Result};
use serde_json::Value;
use strum::IntoStaticStr;

use glab_core::domain::Issue;

use crate::client::{GitLabClient, get_mutation_payload};
use crate::query::{ISSUE_FIELDS, WORK_ITEM_FIELDS, document};
use crate::wire::{GqlNamespaceWorkItems, GqlRootIssues, GqlWorkItem};

/// The states an issue list query can filter on. `None` asks for every state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum IssueState {
    Opened,
    Closed,
}

impl GitLabClient {
    /// List the issues under each of `namespaces` and its descendant projects,
    /// deduplicated by id across namespaces.
    ///
    /// `updated_after` is an ISO 8601 timestamp restricting the walk to issues
    /// touched since — the incremental refresh — and `None` walks all of them.
    pub async fn list_namespace_issues(
        &self,
        namespaces: &[String],
        state: Option<IssueState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let query = document(
            r"
            query listWorkItems($path: ID!, $state: IssuableState, $updatedAfter: Time, $after: String) {
                namespace(fullPath: $path) {
                    workItems(
                        includeDescendants: true
                        types: [ISSUE]
                        state: $state
                        updatedAfter: $updatedAfter
                        after: $after
                        first: 100
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

        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for namespace in namespaces {
            let issues = self
                .paginate::<Issue, GqlNamespaceWorkItems>("listWorkItems", &query, |after| {
                    serde_json::json!({
                        "path": namespace,
                        "state": state_value(state),
                        "updatedAfter": updated_after,
                        "after": after,
                    })
                })
                .await?;
            all.extend(issues.into_iter().filter(|i| seen.insert(i.id.clone())));
        }
        Ok(all)
    }

    /// List the issues assigned to any of `members`, anywhere on the instance,
    /// deduplicated by id.
    ///
    /// Unlike [`list_namespace_issues`](Self::list_namespace_issues) this is not
    /// scoped to a namespace, so the caller decides which projects' results to
    /// keep.
    pub async fn list_assigned_issues(
        &self,
        members: &[String],
        state: Option<IssueState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let query = document(
            r"
            query listAssignedIssues($assigneeUsernames: [String!], $state: IssuableState, $types: [IssueType!], $after: String, $updatedAfter: Time) {
                issues(
                    assigneeUsernames: $assigneeUsernames
                    state: $state
                    types: $types
                    after: $after
                    updatedAfter: $updatedAfter
                    first: 100
                    sort: UPDATED_DESC
                ) {
                    nodes { ...IssueFields }
                    pageInfo { hasNextPage endCursor }
                }
            }
            ",
            ISSUE_FIELDS,
        );

        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for member in members {
            let issues = self
                .paginate::<Issue, GqlRootIssues>("listAssignedIssues", &query, |after| {
                    serde_json::json!({
                        "assigneeUsernames": [member],
                        "state": state_value(state),
                        "types": ["ISSUE"],
                        "after": after,
                        "updatedAfter": updated_after,
                    })
                })
                .await?;
            all.extend(issues.into_iter().filter(|i| seen.insert(i.id.clone())));
        }
        Ok(all)
    }

    /// Apply `input` to the work item `gid` through the `workItemUpdate`
    /// mutation and return the issue as it stands after the write.
    ///
    /// `input` carries the widget fields to change (`assigneesWidget`,
    /// `labelsWidget`, `stateEvent`); the id is filled in here.
    pub async fn update_issue(&self, gid: &str, input: Value) -> Result<Issue> {
        let json = self
            .update_work_item(input_with_id(input, gid), true)
            .await?;
        let work_item = get_mutation_payload(&json, "workItemUpdate")?
            .get("workItem")
            .context("missing workItem in mutation response")?;
        let work_item: GqlWorkItem = serde_json::from_value(work_item.clone())
            .context("failed to deserialize workItem from mutation response")?;
        Ok(Issue::from(work_item))
    }

    /// Move the work item `gid` to `status_id`, one of the ids
    /// [`fetch_work_item_statuses`](Self::fetch_work_item_statuses) returned.
    pub async fn update_issue_status(&self, gid: &str, status_id: &str) -> Result<()> {
        let input = serde_json::json!({ "statusWidget": { "status": status_id } });
        let json = self
            .update_work_item(input_with_id(input, gid), false)
            .await?;
        get_mutation_payload(&json, "workItemUpdate")?;
        Ok(())
    }

    /// Move the work item `gid` into iteration `iteration_gid`, or out of every
    /// iteration when it is `None`.
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

    /// Run `workItemUpdate` with `input`. `read_back` selects the updated work
    /// item in the response; a mutation whose result the caller discards skips
    /// it, keeping the document — and GitLab's complexity budget for it — small.
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
        self.graphql(
            "workItemUpdate",
            &query,
            serde_json::json!({ "input": input }),
        )
        .await
    }
}

/// `input` with the work item's global id filled in.
fn input_with_id(mut input: Value, gid: &str) -> Value {
    input["id"] = serde_json::json!(gid);
    input
}

/// A state filter as the `IssuableState` variable, `null` for every state.
fn state_value(state: Option<IssueState>) -> Value {
    state.map_or(Value::Null, |s| Value::from(<&'static str>::from(s)))
}
