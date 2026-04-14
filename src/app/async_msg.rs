//! TEA handle phase for async messages: process results from background tasks.

use crate::cmd::Cmd;
use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};

use super::{App, AsyncMsg, FetchState, View};

impl App {
    pub(super) fn handle_async_msg(&mut self, msg: AsyncMsg) {
        match msg {
            AsyncMsg::IssuesLoaded(result, incremental) => match result {
                Ok(issues) => {
                    self.merge_issues(issues, incremental);
                    // Snapshot all issues (open + closed) for DB persistence
                    // before filtering to open-only in memory.
                    self.ui
                        .pending_cmds
                        .push(Cmd::PersistIssuesFull(self.data.issues.clone()));
                    self.data.issues.retain(|i| i.issue.state == "opened");
                    let now = Self::now_secs();
                    self.ui.last_fetched_at = Some(now);
                    self.ui.pending_cmds.push(Cmd::PersistLastFetchedAt(now));
                    self.ui.error = None;
                    self.record_fetch_done();
                    self.ui.dirty.issues = true;
                    self.ui.pending_cmds.push(Cmd::FetchHealthData);
                }
                Err(e) => {
                    self.record_fetch_done();
                    self.show_error(format!("Issues: {e:#}"));
                }
            },
            AsyncMsg::MrsLoaded(result, incremental) => match result {
                Ok((tracking, external)) => {
                    let mrs: Vec<_> = tracking.into_iter().chain(external).collect();
                    self.merge_mrs(mrs, incremental);
                    // Snapshot all MRs (open + closed) for DB persistence
                    // before filtering to open-only in memory.
                    self.ui
                        .pending_cmds
                        .push(Cmd::PersistMrsFull(self.data.mrs.clone()));
                    self.data.mrs.retain(|m| m.mr.state == "opened");
                    let now = Self::now_secs();
                    self.ui.last_fetched_at = Some(now);
                    self.ui.pending_cmds.push(Cmd::PersistLastFetchedAt(now));
                    self.record_fetch_done();
                    self.ui.error = None;
                    self.ui.dirty.mrs = true;
                }
                Err(e) => {
                    self.record_fetch_done();
                    self.show_error(format!("MRs: {e:#}"));
                }
            },
            AsyncMsg::DiscussionsLoaded(result) => {
                self.ui.loading = false;
                match result {
                    Ok(discussions) => {
                        if self.ui.view == View::IssueDetail {
                            self.ui.views.issue_detail.discussions = discussions;
                            self.ui.views.issue_detail.loading_notes = false;
                        } else if self.ui.view == View::MrDetail {
                            self.ui.views.mr_detail.discussions = discussions;
                            self.ui.views.mr_detail.loading_notes = false;
                        }
                    }
                    Err(e) => {
                        self.show_error(format!("Notes: {e:#}"));
                    }
                }
            }
            AsyncMsg::ActionDone(result) => {
                self.ui.loading = false;
                match result {
                    Ok(_msg) => {
                        self.ui.error = None;
                        self.ui.pending_cmds.push(Cmd::FetchAll);
                    }
                    Err(e) => {
                        self.show_error(format!("{e:#}"));
                    }
                }
            }
            AsyncMsg::IssueUpdated(result) => {
                self.ui.loading = false;
                match result {
                    Ok(issue) => {
                        if let Some(pos) =
                            self.data.issues.iter().position(|e| e.issue.id == issue.id)
                        {
                            self.data.issues[pos].issue = issue;
                        }
                        self.ui.error = None;
                        self.ui.dirty.issues = true;
                        self.ui.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::MrUpdated(result, project_path) => {
                self.ui.loading = false;
                match result {
                    Ok(mr) => {
                        if let Some(pos) = self
                            .data
                            .mrs
                            .iter()
                            .position(|e| e.mr.iid == mr.iid && e.project_path == project_path)
                        {
                            self.data.mrs[pos].mr = mr;
                        }
                        self.ui.error = None;
                        self.ui.dirty.mrs = true;
                        self.ui.pending_cmds.push(Cmd::PersistMrs);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::IssueStatusUpdated(result) => {
                self.ui.loading = false;
                match result {
                    Ok((project_path, iid, status_name)) => {
                        if let Some(pos) = self
                            .data
                            .issues
                            .iter()
                            .position(|e| e.issue.iid == iid && e.project_path == project_path)
                        {
                            self.data.issues[pos].issue.custom_status = Some(status_name);
                        }
                        self.ui.error = None;
                        self.ui.dirty.issues = true;
                        self.ui.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::LabelsLoaded(result) => {
                if let Ok(labels) = result {
                    self.data.labels = labels;
                    self.ui.dirty.labels = true;
                    self.ui.pending_cmds.push(Cmd::PersistLabels);
                }
            }
            AsyncMsg::StatusesLoaded(result, project, issue_id, iid, close_only) => {
                self.ui.loading = false;
                let is_background = issue_id == 0 && iid == 0;
                match result {
                    Ok(statuses) => {
                        if statuses.is_empty() && !is_background {
                            // No custom statuses — fall back to open/close toggle
                            let item_state = self
                                .ui
                                .views
                                .issue_list
                                .selected_issue(&self.data.issues)
                                .or_else(|| self.current_detail_issue())
                                .map_or("opened".to_string(), |i| i.issue.state.clone());
                            TrackedIssue::show_close_reopen_confirm(
                                issue_id,
                                iid,
                                &item_state,
                                &mut self.ui,
                            );
                        } else if !statuses.is_empty() {
                            self.data
                                .work_item_statuses
                                .insert(project.clone(), statuses);
                            self.ui.dirty.statuses = true;
                            self.ui.pending_cmds.push(Cmd::PersistStatuses {
                                project: project.clone(),
                            });
                            if !is_background
                                && let Some(statuses) = self.data.work_item_statuses.get(&project)
                            {
                                TrackedIssue::build_status_chord(
                                    &project,
                                    issue_id,
                                    iid,
                                    close_only,
                                    statuses,
                                    &self.data,
                                    &mut self.ui,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        if !is_background {
                            self.show_error(format!("Statuses: {e:#}"));
                        }
                    }
                }
            }
            AsyncMsg::IterationsLoaded(result) => match result {
                Ok(iters) => {
                    self.data.iterations = iters;
                    self.classify_iterations();
                    self.ui.dirty.iterations = true;
                    self.ui.pending_cmds.push(Cmd::PersistIterations);
                    self.ui.pending_cmds.push(Cmd::FetchHealthData);
                }
                Err(e) => {
                    self.show_error(format!("Iterations: {e:#}"));
                }
            },
            AsyncMsg::IterationUpdated(result, issue_id, old_iteration) => {
                self.ui.loading = false;
                match result {
                    Ok(()) => {
                        self.ui.error = None;
                        self.ui.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => {
                        // Revert the optimistic update
                        if let Some(pos) =
                            self.data.issues.iter().position(|i| i.issue.id == issue_id)
                        {
                            self.data.issues[pos].issue.iteration = old_iteration;
                            self.ui.dirty.issues = true;
                        }
                        self.show_error(format!("Move iteration: {e:#}"));
                    }
                }
            }
            AsyncMsg::UnplannedWorkLoaded(result) => {
                if let Ok(dates) = result {
                    self.data.unplanned_work_cache.extend(dates);
                }
                self.data.unplanned_work_state = FetchState::Done;
                // Unplanned work affects health computation, use issues dirty flag
                self.ui.dirty.issues = true;
                self.ui.pending_cmds.push(Cmd::PersistUnplannedWork);
            }
        }
    }

    /// Merge incoming issues into `self.data.issues`, preserving newer cached entries.
    fn merge_issues(&mut self, issues: Vec<TrackedIssue>, incremental: bool) {
        if incremental {
            for item in issues {
                if let Some(pos) = self
                    .data
                    .issues
                    .iter()
                    .position(|i| i.issue.id == item.issue.id)
                {
                    if self.data.issues[pos].issue.updated_at <= item.issue.updated_at {
                        self.data.issues[pos] = item;
                    }
                } else {
                    self.data.issues.push(item);
                }
            }
        } else {
            let mut new_issues = issues;
            for new_iss in &mut new_issues {
                if let Some(pos) = self
                    .data
                    .issues
                    .iter()
                    .position(|i| i.issue.id == new_iss.issue.id)
                {
                    let old_iss = &self.data.issues[pos];
                    if old_iss.issue.updated_at > new_iss.issue.updated_at {
                        *new_iss = old_iss.clone();
                    }
                }
            }
            self.data.issues = new_issues;
        }
    }

    /// Merge incoming MRs into `self.data.mrs`, preserving newer cached entries.
    /// Uses second precision: GraphQL truncates sub-second timestamps.
    fn merge_mrs(&mut self, mrs: Vec<TrackedMergeRequest>, incremental: bool) {
        if incremental {
            for item in mrs {
                if let Some(pos) = self.data.mrs.iter().position(|m| m.mr.id == item.mr.id) {
                    let old_secs = self.data.mrs[pos].mr.updated_at.timestamp();
                    let new_secs = item.mr.updated_at.timestamp();
                    if old_secs <= new_secs {
                        self.data.mrs[pos] = item;
                    }
                } else {
                    self.data.mrs.push(item);
                }
            }
        } else {
            let mut new_mrs = mrs;
            for new_mr in &mut new_mrs {
                if let Some(pos) = self.data.mrs.iter().position(|m| m.mr.id == new_mr.mr.id) {
                    let old_mr = &self.data.mrs[pos];
                    let old_secs = old_mr.mr.updated_at.timestamp();
                    let new_secs = new_mr.mr.updated_at.timestamp();
                    if old_secs > new_secs {
                        *new_mr = old_mr.clone();
                    }
                }
            }
            self.data.mrs = new_mrs;
        }
    }
}
