//! TEA handle phase for async messages: process results from background tasks.

use crate::cmd::Cmd;
use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};

use super::{App, AsyncMsg, ConfirmAction, FetchState, Overlay, View};

impl App {
    pub(super) fn handle_async_msg(&mut self, msg: AsyncMsg) {
        match msg {
            AsyncMsg::IssuesLoaded(result, incremental) => match result {
                Ok(issues) => {
                    self.merge_issues(issues, incremental);
                    // Snapshot all issues (open + closed) for DB persistence
                    // before filtering to open-only in memory.
                    self.pending_cmds
                        .push(Cmd::PersistIssuesFull(self.issues.clone()));
                    self.issues.retain(|i| i.issue.state == "opened");
                    let now = Self::now_secs();
                    self.last_fetched_at = Some(now);
                    self.pending_cmds.push(Cmd::PersistLastFetchedAt(now));
                    self.error = None;
                    self.record_fetch_done();
                    self.dirty.issues = true;
                    self.pending_cmds.push(Cmd::FetchHealthData);
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
                    self.pending_cmds
                        .push(Cmd::PersistMrsFull(self.mrs.clone()));
                    self.mrs.retain(|m| m.mr.state == "opened");
                    let now = Self::now_secs();
                    self.last_fetched_at = Some(now);
                    self.pending_cmds.push(Cmd::PersistLastFetchedAt(now));
                    self.record_fetch_done();
                    self.error = None;
                    self.dirty.mrs = true;
                }
                Err(e) => {
                    self.record_fetch_done();
                    self.show_error(format!("MRs: {e:#}"));
                }
            },
            AsyncMsg::DiscussionsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(discussions) => {
                        if self.view == View::IssueDetail {
                            self.views.issue_detail.discussions = discussions;
                            self.views.issue_detail.loading_notes = false;
                        } else if self.view == View::MrDetail {
                            self.views.mr_detail.discussions = discussions;
                            self.views.mr_detail.loading_notes = false;
                        }
                    }
                    Err(e) => {
                        self.show_error(format!("Notes: {e:#}"));
                    }
                }
            }
            AsyncMsg::ActionDone(result) => {
                self.loading = false;
                match result {
                    Ok(_msg) => {
                        self.error = None;
                        self.pending_cmds.push(Cmd::FetchAll);
                    }
                    Err(e) => {
                        self.show_error(format!("{e:#}"));
                    }
                }
            }
            AsyncMsg::IssueUpdated(result) => {
                self.loading = false;
                match result {
                    Ok(issue) => {
                        if let Some(pos) = self.issues.iter().position(|e| e.issue.id == issue.id) {
                            self.issues[pos].issue = issue;
                        }
                        self.error = None;
                        self.dirty.issues = true;
                        self.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::MrUpdated(result, project_path) => {
                self.loading = false;
                match result {
                    Ok(mr) => {
                        if let Some(pos) = self
                            .mrs
                            .iter()
                            .position(|e| e.mr.iid == mr.iid && e.project_path == project_path)
                        {
                            self.mrs[pos].mr = mr;
                        }
                        self.error = None;
                        self.dirty.mrs = true;
                        self.pending_cmds.push(Cmd::PersistMrs);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::IssueStatusUpdated(result) => {
                self.loading = false;
                match result {
                    Ok((project_path, iid, status_name)) => {
                        if let Some(pos) = self
                            .issues
                            .iter()
                            .position(|e| e.issue.iid == iid && e.project_path == project_path)
                        {
                            self.issues[pos].issue.custom_status = Some(status_name);
                        }
                        self.error = None;
                        self.dirty.issues = true;
                        self.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::LabelsLoaded(result) => {
                if let Ok(labels) = result {
                    self.labels = labels;
                    self.dirty.labels = true;
                    self.pending_cmds.push(Cmd::PersistLabels);
                }
            }
            AsyncMsg::StatusesLoaded(result, project, issue_id, iid, close_only) => {
                self.loading = false;
                let is_background = issue_id == 0 && iid == 0;
                match result {
                    Ok(statuses) => {
                        if statuses.is_empty() && !is_background {
                            // No custom statuses — fall back to open/close toggle
                            let item_state = self
                                .views.issue_list
                                .selected_issue(&self.issues)
                                .or_else(|| self.current_detail_issue())
                                .map_or("opened", |i| i.issue.state.as_str());
                            let action = if item_state == "opened" {
                                ConfirmAction::CloseIssue(issue_id, iid)
                            } else {
                                ConfirmAction::ReopenIssue(issue_id, iid)
                            };
                            self.overlay = Overlay::Confirm(action);
                        } else if !statuses.is_empty() {
                            self.work_item_statuses.insert(project.clone(), statuses);
                            self.dirty.statuses = true;
                            self.pending_cmds.push(Cmd::PersistStatuses {
                                project: project.clone(),
                            });
                            if !is_background {
                                self.show_status_chord(&project, issue_id, iid, close_only);
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
                    self.iterations = iters;
                    self.classify_iterations();
                    self.dirty.iterations = true;
                    self.pending_cmds.push(Cmd::PersistIterations);
                    self.pending_cmds.push(Cmd::FetchHealthData);
                }
                Err(e) => {
                    self.show_error(format!("Iterations: {e:#}"));
                }
            },
            AsyncMsg::IterationUpdated(result, issue_id, old_iteration) => {
                self.loading = false;
                match result {
                    Ok(()) => {
                        self.error = None;
                        self.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => {
                        // Revert the optimistic update
                        if let Some(pos) = self.issues.iter().position(|i| i.issue.id == issue_id) {
                            self.issues[pos].issue.iteration = old_iteration;
                            self.dirty.issues = true;
                        }
                        self.show_error(format!("Move iteration: {e:#}"));
                    }
                }
            }
            AsyncMsg::UnplannedWorkLoaded(result) => {
                if let Ok(dates) = result {
                    self.unplanned_work_cache.extend(dates);
                }
                self.unplanned_work_state = FetchState::Done;
                // Unplanned work affects health computation, use issues dirty flag
                self.dirty.issues = true;
                self.pending_cmds.push(Cmd::PersistUnplannedWork);
            }
        }
    }

    /// Merge incoming issues into `self.issues`, preserving newer cached entries.
    fn merge_issues(&mut self, issues: Vec<TrackedIssue>, incremental: bool) {
        if incremental {
            for item in issues {
                if let Some(pos) = self.issues.iter().position(|i| i.issue.id == item.issue.id) {
                    if self.issues[pos].issue.updated_at <= item.issue.updated_at {
                        self.issues[pos] = item;
                    }
                } else {
                    self.issues.push(item);
                }
            }
        } else {
            let mut new_issues = issues;
            for new_iss in &mut new_issues {
                if let Some(pos) = self
                    .issues
                    .iter()
                    .position(|i| i.issue.id == new_iss.issue.id)
                {
                    let old_iss = &self.issues[pos];
                    if old_iss.issue.updated_at > new_iss.issue.updated_at {
                        *new_iss = old_iss.clone();
                    }
                }
            }
            self.issues = new_issues;
        }
    }

    /// Merge incoming MRs into `self.mrs`, preserving newer cached entries.
    /// Uses second precision: GraphQL truncates sub-second timestamps.
    fn merge_mrs(&mut self, mrs: Vec<TrackedMergeRequest>, incremental: bool) {
        if incremental {
            for item in mrs {
                if let Some(pos) = self.mrs.iter().position(|m| m.mr.id == item.mr.id) {
                    let old_secs = self.mrs[pos].mr.updated_at.timestamp();
                    let new_secs = item.mr.updated_at.timestamp();
                    if old_secs <= new_secs {
                        self.mrs[pos] = item;
                    }
                } else {
                    self.mrs.push(item);
                }
            }
        } else {
            let mut new_mrs = mrs;
            for new_mr in &mut new_mrs {
                if let Some(pos) = self.mrs.iter().position(|m| m.mr.id == new_mr.mr.id) {
                    let old_mr = &self.mrs[pos];
                    let old_secs = old_mr.mr.updated_at.timestamp();
                    let new_secs = new_mr.mr.updated_at.timestamp();
                    if old_secs > new_secs {
                        *new_mr = old_mr.clone();
                    }
                }
            }
            self.mrs = new_mrs;
        }
    }
}
