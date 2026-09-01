//! What the planning views need beyond the issues themselves: a group's
//! iterations, the statuses a project's issues may hold, and when an issue
//! entered the iteration it is in.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;

use glab_core::domain::{Iteration, WorkItemStatus};

use crate::client::GitLabClient;
use crate::wire::{GqlGroupIterations, GqlResponse, GqlStatusesData, GqlWorkItemNotes};

/// How many activity-note queries run at once in
/// [`GitLabClient::fetch_iteration_added_dates_batch`]. One request per issue in
/// an iteration adds up, and GitLab rate-limits a burst.
const NOTES_CONCURRENCY: usize = 5;

impl GitLabClient {
    /// List the iterations of the group `group_path`, in cadence and due-date
    /// order.
    pub async fn list_group_iterations(&self, group_path: &str) -> Result<Vec<Iteration>> {
        let query = r"
            query listIterations($path: ID!, $after: String) {
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
        self.paginate::<Iteration, GqlGroupIterations>(
            "listIterations",
            query,
            |after| serde_json::json!({ "path": group_path, "after": after }),
        )
        .await
    }

    /// List the statuses an issue in `project` may be set to, in the order the
    /// project defines them.
    ///
    /// Returns an empty list on an instance or tier where issues carry no
    /// status widget.
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
        let json = self
            .graphql(
                "fetchStatuses",
                query,
                serde_json::json!({ "path": project }),
            )
            .await?;
        let resp: GqlResponse<GqlStatusesData> =
            serde_json::from_value(json).context("failed to deserialize work item statuses")?;

        let Some(namespace) = resp.data.namespace else {
            return Ok(Vec::new());
        };
        // Only the status widget definition carries `allowedStatuses`; the rest
        // of the array is every other widget the issue type supports.
        let statuses = namespace
            .work_item_types
            .nodes
            .into_iter()
            .flat_map(|t| t.widget_definitions)
            .filter_map(|d| d.allowed_statuses)
            .find(|s| !s.is_empty())
            .unwrap_or_default();

        Ok(statuses.into_iter().map(WorkItemStatus::from).collect())
    }

    /// When the work item `iid` under `namespace` was last added to an
    /// iteration, or `None` when its activity records no iteration change.
    ///
    /// GitLab exposes no field for this, so it is read from the activity notes:
    /// the most recent system note whose icon is `iteration`.
    pub async fn fetch_work_item_iteration_added_at(
        &self,
        namespace: &str,
        iid: &str,
    ) -> Result<Option<DateTime<Utc>>> {
        let query = r"
            query workItemActivity($fullPath: ID!, $iid: String!) {
                workspace: namespace(fullPath: $fullPath) {
                    workItem(iid: $iid) {
                        widgets(onlyTypes: [NOTES]) {
                            ... on WorkItemWidgetNotes {
                                discussions(first: 100, filter: ONLY_ACTIVITY) {
                                    nodes {
                                        notes {
                                            nodes { system systemNoteIconName createdAt }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        ";
        let json = self
            .graphql(
                "workItemActivity",
                query,
                serde_json::json!({ "fullPath": namespace, "iid": iid }),
            )
            .await?;
        let resp: GqlResponse<GqlWorkItemNotes> =
            serde_json::from_value(json).context("failed to deserialize work item activity")?;

        let Some(work_item) = resp.data.workspace.and_then(|w| w.work_item) else {
            return Ok(None);
        };

        Ok(work_item
            .widgets
            .into_iter()
            .filter_map(|w| w.discussions)
            .flat_map(|d| d.nodes)
            .flat_map(|d| d.notes.nodes)
            .filter(|n| n.system && n.icon.as_deref() == Some("iteration"))
            .map(|n| n.created_at.with_timezone(&Utc))
            .max())
    }

    /// Read the "added to iteration" timestamp for many work items at once,
    /// keyed by the issue id each was requested under.
    ///
    /// Each `items` entry is the namespace to look the work item up in, its iid,
    /// and the issue id to key the result by. An item whose lookup fails or
    /// records no iteration change is absent from the map: the caller uses this
    /// to shade a planning view, and one unreadable issue should not fail the
    /// batch.
    pub async fn fetch_iteration_added_dates_batch(
        &self,
        items: Vec<(String, String, String)>,
    ) -> Result<HashMap<String, DateTime<Utc>>> {
        let sem = Arc::new(tokio::sync::Semaphore::new(NOTES_CONCURRENCY));
        let mut handles = Vec::with_capacity(items.len());

        for (namespace, iid, issue_id) in items {
            let client = self.clone();
            let sem = Arc::clone(&sem);
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await;
                let added_at = client
                    .fetch_work_item_iteration_added_at(&namespace, &iid)
                    .await;
                if let Err(e) = &added_at {
                    tracing::debug!(issue_id, error = ?e, "iteration added-at lookup ✗");
                }
                (issue_id, added_at)
            }));
        }

        let mut map = HashMap::new();
        for handle in handles {
            if let Ok((issue_id, Ok(Some(added_at)))) = handle.await {
                map.insert(issue_id, added_at);
            }
        }
        Ok(map)
    }
}
