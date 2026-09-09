//! Fetch-related methods: API calls, incremental fetch helpers, health data.

use std::time::{SystemTime, UNIX_EPOCH};

use glab_api::Issuable;

use crate::cmd::Cmd;

use super::{App, AsyncMsg, FetchState};

impl App {
    pub fn fetch_all(&mut self) {
        self.ui.fetch_started_at = Some(Self::now_millis());
        // Candidate incremental cursor for this cycle. It is only promoted to
        // `last_fetched_at` once every leg has succeeded — see
        // `record_fetch_done`.
        self.ui.fetch_pending_at = Some(Self::now_secs());
        self.ui.fetch_legs_left = 2; // issues + MRs
        self.fetch_issues();
        self.fetch_mrs();
        self.fetch_labels();
        self.fetch_iterations();
        self.fetch_statuses_for_board();
    }

    /// Fetch work item statuses for each tracking project (for the iteration board).
    fn fetch_statuses_for_board(&self) {
        for project in self.ctx.config.all_tracking_projects() {
            if self.data.work_item_statuses.contains_key(&project) {
                continue; // already cached
            }
            let client = self.ctx.client.clone();
            let tx = self.ctx.async_tx.clone();
            tokio::spawn(async move {
                let result = client.fetch_work_item_statuses(&project).await;
                // Reuse StatusesLoaded with sentinel values (issue_id=0, empty
                // iid) to indicate this is a background fetch, not a chord
                // popup trigger.
                let _ = tx.send(AsyncMsg::StatusesLoaded(
                    result,
                    project,
                    String::new(),
                    String::new(),
                    false,
                ));
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

    /// Record fetch duration and, once every leg has succeeded, advance the
    /// incremental cursor. Called by each data handler; the last one to arrive
    /// captures the total wall-clock time from `fetch_all()`.
    ///
    /// The cursor is the fetch *start* time and only moves when the whole cycle
    /// succeeded: advancing it after a failed or timed-out request would make
    /// the next incremental fetch skip everything the failure missed.
    pub(super) fn record_fetch_done(&mut self, ok: bool) {
        self.ui.loading = false;
        if let Some(started) = self.ui.fetch_started_at {
            self.ui.last_fetch_ms = Some(Self::now_millis().saturating_sub(started));
        }
        if let Some(ts) = commit_cursor(
            &mut self.ui.fetch_pending_at,
            &mut self.ui.fetch_legs_left,
            ok,
        ) {
            self.ui.last_fetched_at = Some(ts);
            self.ui.pending_cmds.push(Cmd::PersistLastFetchedAt(ts));
        }
    }

    fn fetch_issues(&self) {
        let client = self.ctx.client.clone();
        let tx = self.ctx.async_tx.clone();
        let updated_after = self.ui.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        let members = self.ctx.config.all_members();
        let tracking_projects = self.ctx.config.all_tracking_projects();
        let config = self.ctx.config.clone();

        // Collect external projects that have open issues we track, so we can
        // detect state changes (closed, reassigned) even if those issues are
        // no longer assigned to a team member.
        let tracked_ids: std::collections::HashSet<String> =
            self.data.issues.iter().map(|i| i.id.clone()).collect();
        let external_projects: Vec<String> = self
            .data
            .issues
            .iter()
            .filter(|i| !self.ctx.config.is_tracking_project(i.project_path()))
            .map(|i| i.project_path().to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        tracing::info!(
            incremental,
            members = members.len(),
            tracking_projects = tracking_projects.len(),
            external_projects = external_projects.len(),
            tracked = tracked_ids.len(),
            updated_after = ?updated_after,
            "fetch_issues spawn"
        );
        tokio::spawn(async move {
            let ua = updated_after.as_deref();
            let t0 = std::time::Instant::now();
            let (tracking, assigned, external) = tokio::join!(
                client.list_namespace_issues(&tracking_projects, None, ua),
                client.list_assigned_issues(&members, None, ua),
                client.list_namespace_issues(&external_projects, None, ua),
            );
            let join_ms = t0.elapsed().as_millis(); // all three legs, not each
            for (op, res) in [
                ("namespace(tracking)", tracking.as_ref()),
                ("assigned", assigned.as_ref()),
                ("namespace(external)", external.as_ref()),
            ] {
                match res {
                    Ok(v) => tracing::info!(op, count = v.len(), join_ms, "issues leg ✓"),
                    Err(e) => tracing::warn!(op, error = ?e, join_ms, "issues leg ✗"),
                }
            }
            let result = match (tracking, assigned, external) {
                (Ok(mut t), Ok(a), Ok(ext)) => {
                    let mut seen: std::collections::HashSet<String> =
                        t.iter().map(|i| i.id.clone()).collect();
                    // The assigned query is instance-wide; issues inside a
                    // tracking namespace already came from the walk above.
                    t.extend(a.into_iter().filter(|i| {
                        !config.is_tracking_project(i.project_path()) && seen.insert(i.id.clone())
                    }));
                    // Only merge external issues we already track — don't
                    // pull in new issues from those projects.
                    t.extend(
                        ext.into_iter()
                            .filter(|i| tracked_ids.contains(&i.id) && seen.insert(i.id.clone())),
                    );
                    Ok(t)
                }
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
            };
            if let Err(e) = tx.send(AsyncMsg::IssuesLoaded(result, incremental)) {
                tracing::error!(error = %e, "failed to send IssuesLoaded — receiver dropped");
            }
        });
    }

    fn fetch_mrs(&self) {
        let client = self.ctx.client.clone();
        let members = self.ctx.config.all_members();
        let tracking_projects = self.ctx.config.all_tracking_projects();
        let config = self.ctx.config.clone();
        let tx = self.ctx.async_tx.clone();
        let updated_after = self.ui.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        tracing::info!(
            incremental,
            members = members.len(),
            updated_after = ?updated_after,
            "fetch_mrs spawn"
        );
        tokio::spawn(async move {
            let ua = updated_after.as_deref();
            let t0 = std::time::Instant::now();
            let tracking = client.list_project_mrs(&tracking_projects, None, ua).await;
            match &tracking {
                Ok(t) => tracing::info!(
                    count = t.len(),
                    elapsed_ms = t0.elapsed().as_millis(),
                    "list_project_mrs ✓"
                ),
                Err(e) => tracing::warn!(
                    error = ?e,
                    elapsed_ms = t0.elapsed().as_millis(),
                    "list_project_mrs ✗"
                ),
            }
            let t1 = std::time::Instant::now();
            // External MRs enter the cache only via team-member assignment, so
            // we only want currently-open MRs. Fetching `"all"` paginates
            // through every merged/closed MR ever assigned to each member —
            // that's tens of thousands of requests for long-tenured teams and
            // causes the MR list to appear frozen on load.
            let external = client
                .list_user_mrs(&members, Some(glab_api::MrState::Opened), ua)
                .await
                // A user's MRs are instance-wide; the ones inside a tracking
                // project already came from the per-project walk above.
                .map(|mrs| {
                    mrs.into_iter()
                        .filter(|m| !config.is_tracking_project(m.project_path()))
                        .collect::<Vec<_>>()
                });
            match &external {
                Ok(e) => tracing::info!(
                    count = e.len(),
                    elapsed_ms = t1.elapsed().as_millis(),
                    "list_user_mrs ✓"
                ),
                Err(e) => tracing::warn!(
                    error = ?e,
                    elapsed_ms = t1.elapsed().as_millis(),
                    "list_user_mrs ✗"
                ),
            }
            let result = match (tracking, external) {
                (Ok(t), Ok(e)) => Ok((t, e)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            if let Err(e) = tx.send(AsyncMsg::MrsLoaded(result, incremental)) {
                tracing::error!(error = %e, "failed to send MrsLoaded — receiver dropped");
            }
        });
    }

    fn fetch_labels(&self) {
        let client = self.ctx.client.clone();
        let projects = self.ctx.config.all_tracking_projects();
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

    pub(super) fn fetch_notes_for_issue(&self, project: &str, iid: &str) {
        let client = self.ctx.client.clone();
        let project = project.to_string();
        let iid = iid.to_string();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_discussions(Issuable::Issue, &project, &iid)
                .await;
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(result));
        });
    }

    pub(super) fn fetch_notes_for_mr(&self, project: &str, iid: &str) {
        let client = self.ctx.client.clone();
        let project = project.to_string();
        let iid = iid.to_string();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_discussions(Issuable::MergeRequest, &project, &iid)
                .await;
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(result));
        });
    }

    pub(super) fn fetch_iterations(&self) {
        let client = self.ctx.client.clone();
        let tx = self.ctx.async_tx.clone();
        // Each team's board reads its own group cadence.
        let group = self
            .ctx
            .config
            .team_tracking_group(self.ui.active_team)
            .to_string();
        tokio::spawn(async move {
            let result = client.list_group_iterations(&group).await;
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
        let items: Vec<(String, String, String)> = self
            .data
            .issues
            .iter()
            .filter(|i| {
                i.iteration.as_ref().is_some_and(|it| it.id == current_id)
                    && !self.data.unplanned_work_cache.contains_key(&i.id)
            })
            .map(|i| {
                // Derive namespace from project_path (same as the tracking project ancestor)
                let namespace = self
                    .ctx
                    .config
                    .team_tracking_projects(self.ui.active_team)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| i.project_path().to_string());
                (namespace, i.iid.clone(), i.id.clone())
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

/// Account for one finished fetch leg, returning the cursor to commit once the
/// whole cycle has succeeded. A failure drops the candidate entirely, so the
/// next fetch re-reads the window the failed request never delivered.
fn commit_cursor(pending: &mut Option<u64>, legs_left: &mut u8, ok: bool) -> Option<u64> {
    if !ok {
        tracing::warn!("fetch leg failed — holding incremental cursor back");
        *pending = None;
        return None;
    }
    *legs_left = legs_left.saturating_sub(1);
    if *legs_left == 0 {
        pending.take()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::commit_cursor;

    /// One leg per call, in the order given; returns the committed cursor.
    fn cycle(legs: &[bool]) -> Option<u64> {
        let mut pending = Some(100);
        let mut left = 2;
        legs.iter()
            .filter_map(|ok| commit_cursor(&mut pending, &mut left, *ok))
            .last()
    }

    #[test]
    fn cursor_commits_only_when_every_leg_succeeds() {
        assert_eq!(cycle(&[true, true]), Some(100));
        assert_eq!(cycle(&[true]), None, "still one leg outstanding");
        assert_eq!(cycle(&[false, true]), None, "earlier failure lost data");
        assert_eq!(cycle(&[true, false]), None, "later failure lost data");
        assert_eq!(cycle(&[false, false]), None);
    }
}
