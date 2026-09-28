//! Approve and merge go over REST: GraphQL has no equivalent.

use anyhow::{Context, Result};
use reqwest::Method;
use serde::de::IgnoredAny;
use serde_json::Value;
use strum::IntoStaticStr;

use glab_core::domain::MergeRequest;
use urlencoding::encode;

use crate::client::{GitLabClient, PAGE_SIZE, document, get_mutation_payload, join_walks};
use crate::wire::{ProjectMrsQuery, UserMrsQuery};

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
        detailedMergeStatus
        approvedBy { nodes { ...UserFields } }
        headPipeline { status }
        resolvableDiscussionsCount
        resolvedDiscussionsCount
    }
";

/// `None` asks for every state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum MrState {
    Opened,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserMrRole {
    Authored,
    Assigned,
    Reviewer,
}

impl UserMrRole {
    fn field(self) -> &'static str {
        match self {
            UserMrRole::Authored => "authoredMergeRequests",
            UserMrRole::Assigned => "assignedMergeRequests",
            UserMrRole::Reviewer => "reviewRequestedMergeRequests",
        }
    }
}

impl GitLabClient {
    pub async fn list_project_mrs(
        &self,
        projects: &[String],
        state: Option<MrState>,
        updated_after: Option<&str>,
    ) -> Result<Vec<MergeRequest>> {
        let query = document(
            r"
            query listProjectMrs($projectPath: ID!, $state: MergeRequestState, $updatedAfter: Time, $after: String, $first: Int) {
                project(fullPath: $projectPath) {
                    mergeRequests(
                        state: $state
                        updatedAfter: $updatedAfter
                        after: $after
                        first: $first
                        sort: UPDATED_DESC
                    ) {
                        nodes { ...MrFields }
                        pageInfo { hasNextPage endCursor }
                    }
                }
            }
            ",
            MR_FIELDS,
        );

        let mut set = tokio::task::JoinSet::new();
        for (idx, project) in projects.iter().enumerate() {
            let client = self.clone();
            let query = query.clone();
            let project = project.clone();
            let updated_after = updated_after.map(str::to_string);
            set.spawn(async move {
                let mrs = client
                    .paginate::<MergeRequest, ProjectMrsQuery>("listProjectMrs", &query, |after| {
                        serde_json::json!({
                            "projectPath": project,
                            "state": state_value(state),
                            "updatedAfter": updated_after,
                            "after": after,
                            "first": PAGE_SIZE,
                        })
                    })
                    .await;
                (idx, mrs)
            });
        }
        join_walks(set, |m| &m.id).await
    }

    /// Three paginated queries per member — the slowest call in a refresh, so
    /// it traces per-member timings at debug level.
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

        // ponytail: unbounded fan-out (members × 3). Meter it with a Semaphore
        // if GitLab starts answering 429.
        let mut set = tokio::task::JoinSet::new();
        for (idx, member) in members.iter().enumerate() {
            for (role_idx, role) in [
                UserMrRole::Authored,
                UserMrRole::Assigned,
                UserMrRole::Reviewer,
            ]
            .into_iter()
            .enumerate()
            {
                let client = self.clone();
                let member = member.clone();
                let updated_after = updated_after.map(str::to_string);
                set.spawn(async move {
                    let started = std::time::Instant::now();
                    let mrs = client
                        .user_mrs(&member, role, state, updated_after.as_deref())
                        .await;
                    let elapsed_ms = started.elapsed().as_millis();
                    match &mrs {
                        Ok(mrs) => tracing::debug!(
                            member,
                            role = role.field(),
                            count = mrs.len(),
                            elapsed_ms,
                            "user_mrs ✓"
                        ),
                        Err(e) => {
                            tracing::warn!(member, role = role.field(), error = ?e, elapsed_ms, "user_mrs ✗");
                        }
                    }
                    (idx * 3 + role_idx, mrs)
                });
            }
        }
        let all = join_walks(set, |m| &m.id).await?;

        tracing::info!(
            total = all.len(),
            elapsed_ms = overall.elapsed().as_millis(),
            "list_user_mrs done"
        );
        Ok(all)
    }

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
                query listUserMrs($username: String!, $state: MergeRequestState, $after: String, $updatedAfter: Time, $first: Int) {{
                    user(username: $username) {{
                        {field}(state: $state, after: $after, updatedAfter: $updatedAfter, first: $first, sort: UPDATED_DESC) {{
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

        self.paginate::<MergeRequest, UserMrsQuery>("listUserMrs", &query, |after| {
            serde_json::json!({
                "username": member,
                "state": state_value(state),
                "after": after,
                "updatedAfter": updated_after,
                "first": PAGE_SIZE,
            })
        })
        .await
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

    /// `label_ids` are REST numeric ids; the mutation wants global ids.
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

    /// `input` carries the fields to change; the project path and iid are
    /// filled in here.
    pub(crate) async fn mr_mutation(
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
            .graphql_once(mutation, &query, serde_json::json!({ "input": input }))
            .await?;
        let mr = get_mutation_payload(&json, mutation)?
            .get("mergeRequest")
            .cloned()
            .unwrap_or(Value::Null);
        serde_json::from_value(mr)
            .with_context(|| format!("failed to deserialize {mutation} response"))
    }

    pub async fn approve_mr(&self, project: &str, iid: &str) -> Result<()> {
        let path = format!("/projects/{}/merge_requests/{iid}/approve", encode(project));
        Self::send::<IgnoredAny>(self.rest(Method::POST, &path))
            .await
            .map(drop)
    }

    /// Removes the source branch.
    pub async fn merge_mr(&self, project: &str, iid: &str) -> Result<()> {
        let path = format!("/projects/{}/merge_requests/{iid}/merge", encode(project));
        let request = self
            .rest(Method::PUT, &path)
            .json(&serde_json::json!({ "should_remove_source_branch": true }));
        Self::send::<IgnoredAny>(request).await.map(drop)
    }
}

fn state_value(state: Option<MrState>) -> Value {
    state.map_or(Value::Null, |s| Value::from(<&'static str>::from(s)))
}
