//! Fetch-related methods: API calls, incremental fetch helpers, health data.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{App, AsyncMsg, FetchState};

impl App {
    pub fn fetch_all(&self) {
        self.fetch_issues();
        self.fetch_mrs();
        self.fetch_labels();
        self.fetch_iterations();
        self.fetch_statuses_for_board();
    }

    /// Fetch work item statuses for each tracking project (for the iteration board).
    fn fetch_statuses_for_board(&self) {
        for project in &self.ctx.config.tracking_projects {
            if self.data.work_item_statuses.contains_key(project) {
                continue; // already cached
            }
            let client = self.ctx.client.clone();
            let tx = self.ctx.async_tx.clone();
            let project = project.clone();
            tokio::spawn(async move {
                let result = client.fetch_work_item_statuses(&project).await;
                // Reuse StatusesLoaded with sentinel values (issue_id=0, iid=0)
                // to indicate this is a background fetch, not a chord popup trigger.
                let _ = tx.send(AsyncMsg::StatusesLoaded(result, project, 0, 0, false));
            });
        }
    }

    /// Convert a unix timestamp to ISO 8601 for the GitLab API, with 60s safety buffer.
    pub(super) fn updated_after_param(ts: u64) -> String {
        let buffered = ts.saturating_sub(60);
        chrono::DateTime::from_timestamp(i64::try_from(buffered).unwrap_or(i64::MAX), 0)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }

    pub(super) fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    /// Record fetch duration. Called by each data handler; the last one to arrive
    /// captures the total wall-clock time from `fetch_all()`.
    pub(super) fn record_fetch_done(&mut self) {
        self.ui.loading = false;
        if let Some(started) = self.ui.fetch_started_at {
            self.ui.last_fetch_ms = Some(Self::now_millis().saturating_sub(started));
        }
    }

    fn fetch_issues(&self) {
        let client = self.ctx.client.clone();
        let tx = self.ctx.async_tx.clone();
        let updated_after = self.ui.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        let members = self.ctx.config.all_members();
        tokio::spawn(async move {
            let ua = updated_after.as_deref();
            let (tracking, assigned) = tokio::join!(
                client.fetch_tracking_issues(None, ua),
                client.fetch_assigned_issues(&members, None, ua),
            );
            let result = match (tracking, assigned) {
                (Ok(mut t), Ok(a)) => {
                    let existing: std::collections::HashSet<u64> =
                        t.iter().map(|i| i.issue.id).collect();
                    t.extend(a.into_iter().filter(|i| !existing.contains(&i.issue.id)));
                    Ok(t)
                }
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::IssuesLoaded(result, incremental));
        });
    }

    fn fetch_mrs(&self) {
        let client = self.ctx.client.clone();
        let members = self.ctx.config.all_members();
        let tx = self.ctx.async_tx.clone();
        let updated_after = self.ui.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        tokio::spawn(async move {
            let ua = updated_after.as_deref();
            let tracking = client.fetch_tracking_mrs("all", ua).await;
            let external = client.fetch_external_mrs(&members, "all", ua).await;
            let result = match (tracking, external) {
                (Ok(t), Ok(e)) => Ok((t, e)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::MrsLoaded(result, incremental));
        });
    }

    fn fetch_labels(&self) {
        let client = self.ctx.client.clone();
        let projects = self.ctx.config.tracking_projects.clone();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let mut all_labels = Vec::new();
            let mut seen_ids = std::collections::HashSet::new();
            for project in &projects {
                if let Ok(labels) = client.list_project_labels(project).await {
                    for label in labels {
                        if seen_ids.insert(label.id) {
                            all_labels.push(label);
                        }
                    }
                }
            }
            let _ = tx.send(AsyncMsg::LabelsLoaded(Ok(all_labels)));
        });
    }

    pub(super) fn fetch_notes_for_issue(&self, project: &str, iid: u64) {
        let client = self.ctx.client.clone();
        let project = project.to_string();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_issue_discussions(&project, iid).await;
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(result));
        });
    }

    pub(super) fn fetch_notes_for_mr(&self, project: &str, iid: u64) {
        let client = self.ctx.client.clone();
        let project = project.to_string();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_mr_discussions(&project, iid).await;
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(result));
        });
    }

    fn fetch_iterations(&self) {
        let client = self.ctx.client.clone();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.fetch_iterations().await;
            let _ = tx.send(AsyncMsg::IterationsLoaded(result));
        });
    }

    /// Fetch "added to iteration" dates for unplanned work detection.
    pub(super) fn fetch_unplanned_work_data(&mut self) {
        let Some(current_iter) = self.ui.views.planning.current_iteration.as_ref() else {
            return;
        };
        let current_id = current_iter.id.clone();

        // Collect issues in the current iteration that we haven't cached yet
        let items: Vec<(String, String, u64)> = self
            .data.issues
            .iter()
            .filter(|i| {
                i.issue
                    .iteration
                    .as_ref()
                    .is_some_and(|it| it.id == current_id)
                    && !self.data.unplanned_work_cache.contains_key(&i.issue.id)
            })
            .map(|i| {
                // Derive namespace from project_path (same as the tracking project ancestor)
                let namespace = self
                    .ctx.config
                    .tracking_projects
                    .first()
                    .cloned()
                    .unwrap_or_else(|| i.project_path.clone());
                (namespace, i.issue.iid.to_string(), i.issue.id)
            })
            .collect();

        if items.is_empty() {
            self.data.unplanned_work_state = FetchState::Done;
            self.compute_iteration_health();
            return;
        }

        self.data.unplanned_work_state = FetchState::InFlight;

        let client = self.ctx.client.clone();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.fetch_iteration_added_dates_batch(items).await;
            let _ = tx.send(AsyncMsg::UnplannedWorkLoaded(result));
        });
    }

    /// Trigger unplanned work fetch if conditions are met.
    pub(super) fn maybe_fetch_health_data(&mut self) {
        if self.ui.views.planning.current_iteration.is_none() {
            return;
        }
        if self.data.unplanned_work_state != FetchState::InFlight {
            self.fetch_unplanned_work_data();
        }
    }
}
