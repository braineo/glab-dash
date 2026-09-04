//! Merge request reads and writes.
//!
//! Listing and the metadata mutations go over GraphQL; approve and merge have
//! no GraphQL equivalent glab-dash can rely on and go over REST.

use anyhow::{Context, Result};
use reqwest::Method;
use serde::de::IgnoredAny;
use serde_json::Value;
use strum::IntoStaticStr;

use glab_core::domain::MergeRequest;
use urlencoding::encode;

use crate::client::{GitLabClient, get_mutation_payload};
use crate::query::{MR_FIELDS, MR_PAGE_SIZE, document};
use crate::wire::{GqlProjectMrs, GqlUserMrs};

/// The states a merge-request list query can filter on. `None` asks for every
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum MrState {
    Opened,
    Merged,
    Closed,
}

/// Which of a user's merge requests to list: the ones they are assigned, or the
/// ones they were asked to review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserMrRole {
    Assigned,
    Reviewer,
}

impl UserMrRole {
    /// The `User` connection this role reads.
    fn field(self) -> &'static str {
        match self {
            UserMrRole::Assigned => "assignedMergeRequests",
            UserMrRole::Reviewer => "reviewRequestedMergeRequests",
        }
    }
}

impl GitLabClient {
    /// List the merge requests of each project in `projects`, deduplicated by
    /// id.
    pub async fn list_project_mrs(
        &self,
        projects: &[String],
        state: Option<MrState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<MergeRequest>> {
        let query = document(
            &format!(
                r"
                query listProjectMrs($projectPath: ID!, $state: MergeRequestState, $updatedAfter: Time, $after: String) {{
                    project(fullPath: $projectPath) {{
                        mergeRequests(
                            state: $state
                            updatedAfter: $updatedAfter
                            after: $after
                            first: {MR_PAGE_SIZE}
                            sort: UPDATED_DESC
                        ) {{
                            nodes {{ ...MrFields }}
                            pageInfo {{ hasNextPage endCursor }}
                        }}
                    }}
                }}
                "
            ),
            MR_FIELDS,
        );

        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for project in projects {
            let mrs = self
                .paginate::<MergeRequest, GqlProjectMrs>("listProjectMrs", &query, |after| {
                    serde_json::json!({
                        "projectPath": project,
                        "state": state_value(state),
                        "updatedAfter": updated_after,
                        "after": after,
                    })
                })
                .await?;
            all.extend(mrs.into_iter().filter(|m| seen.insert(m.id.clone())));
        }
        Ok(all)
    }

    /// List the merge requests each of `members` is assigned or was asked to
    /// review, anywhere on the instance, deduplicated by id.
    ///
    /// This is the slowest call in a refresh: two queries per member, each
    /// paginated, so it traces per-member timings at debug level.
    pub async fn list_user_mrs(
        &self,
        members: &[String],
        state: Option<MrState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<MergeRequest>> {
        tracing::info!(
            members = members.len(),
            ?updated_after,
            "list_user_mrs start"
        );
        let overall = std::time::Instant::now();

        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for member in members {
            for role in [UserMrRole::Assigned, UserMrRole::Reviewer] {
                let started = std::time::Instant::now();
                let mrs = self.user_mrs(member, role, state, updated_after).await;
                let elapsed_ms = started.elapsed().as_millis();
                match mrs {
                    Ok(mrs) => {
                        tracing::debug!(
                            member,
                            role = role.field(),
                            count = mrs.len(),
                            elapsed_ms,
                            "user_mrs ✓"
                        );
                        all.extend(mrs.into_iter().filter(|m| seen.insert(m.id.clone())));
                    }
                    Err(e) => {
                        tracing::warn!(member, role = role.field(), error = ?e, elapsed_ms, "user_mrs ✗");
                        return Err(e);
                    }
                }
            }
        }

        tracing::info!(
            total = all.len(),
            elapsed_ms = overall.elapsed().as_millis(),
            "list_user_mrs done"
        );
        Ok(all)
    }

    /// Walk one user's merge requests in one role to the end.
    async fn user_mrs(
        &self,
        member: &str,
        role: UserMrRole,
        state: Option<MrState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<MergeRequest>> {
        let query = document(
            &format!(
                r"
                query listUserMrs($username: String!, $state: MergeRequestState, $after: String, $updatedAfter: Time) {{
                    user(username: $username) {{
                        {field}(state: $state, after: $after, updatedAfter: $updatedAfter, first: {MR_PAGE_SIZE}, sort: UPDATED_DESC) {{
                            nodes {{ ...MrFields }}
                            pageInfo {{ hasNextPage endCursor }}
                        }}
                    }}
                }}
                ",
                field = role.field(),
            ),
            MR_FIELDS,
        );

        self.paginate::<MergeRequest, GqlUserMrs>("listUserMrs", &query, |after| {
            serde_json::json!({
                "username": member,
                "state": state_value(state),
                "after": after,
                "updatedAfter": updated_after,
            })
        })
        .await
    }

    /// Close the merge request `iid` in `project`.
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

    /// Replace the assignees of the merge request `iid` in `project`.
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

    /// Replace the labels of the merge request `iid` in `project`. `label_ids`
    /// are REST numeric label ids, which the mutation wants as global ids.
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

    /// Run `mutation` against the merge request `iid` in `project` and read the
    /// merge request back. `input` carries the fields the mutation changes; the
    /// project path and iid that address it are filled in here.
    async fn mr_mutation(
        &self,
        mutation: &'static str,
        input_type: &str,
        project: &str,
        iid: &str,
        mut input: Value,
    ) -> Result<MergeRequest> {
        input["projectPath"] = serde_json::json!(project);
        input["iid"] = serde_json::json!(iid);

        let query = document(
            &format!(
                r"
                mutation {mutation}($input: {input_type}!) {{
                    {mutation}(input: $input) {{
                        errors
                        mergeRequest {{ ...MrFields }}
                    }}
                }}
                "
            ),
            MR_FIELDS,
        );

        let json = self
            .graphql(mutation, &query, serde_json::json!({ "input": input }))
            .await?;
        let mr = get_mutation_payload(&json, mutation)?
            .get("mergeRequest")
            .cloned()
            .unwrap_or(Value::Null);
        serde_json::from_value(mr)
            .with_context(|| format!("failed to deserialize {mutation} response"))
    }

    /// Approve the merge request `iid` in `project`.
    pub async fn approve_mr(&self, project: &str, iid: &str) -> Result<()> {
        let path = format!("/projects/{}/merge_requests/{iid}/approve", encode(project));
        Self::send::<IgnoredAny>(self.rest(Method::POST, &path))
            .await
            .map(drop)
    }

    /// Merge the merge request `iid` in `project`, removing its source branch.
    pub async fn merge_mr(&self, project: &str, iid: &str) -> Result<()> {
        let path = format!("/projects/{}/merge_requests/{iid}/merge", encode(project));
        let request = self
            .rest(Method::PUT, &path)
            .json(&serde_json::json!({ "should_remove_source_branch": true }));
        Self::send::<IgnoredAny>(request).await.map(drop)
    }
}

/// A state filter as the `MergeRequestState` variable, `null` for every state.
fn state_value(state: Option<MrState>) -> Value {
    state.map_or(Value::Null, |s| Value::from(<&'static str>::from(s)))
}
