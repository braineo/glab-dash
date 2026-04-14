//! TEA execute phase: drain pending Cmds and perform I/O side-effects.

use crate::cmd::Cmd;
use crate::db::ViewState;

use super::{App, AsyncMsg, FetchState};

impl App {
    /// Drain `pending_cmds` and execute each side-effect.
    pub(super) fn execute_pending_cmds(&mut self) {
        let cmds = std::mem::take(&mut self.ui.pending_cmds);
        for cmd in cmds {
            self.execute_cmd(cmd);
        }
    }

    fn execute_cmd(&mut self, cmd: Cmd) {
        match cmd {
            // ── Persistence (targeted SQLite writes) ─────────────────
            Cmd::PersistIssues => {
                let _ = self.ctx.db.upsert_issues(&self.data.issues);
            }
            Cmd::PersistIssuesFull(ref issues) => {
                let _ = self.ctx.db.upsert_issues(issues);
            }
            Cmd::PersistMrs => {
                let _ = self.ctx.db.upsert_mrs(&self.data.mrs);
            }
            Cmd::PersistMrsFull(ref mrs) => {
                let _ = self.ctx.db.upsert_mrs(mrs);
            }
            Cmd::PersistLabels => {
                let _ = self.ctx.db.upsert_labels(&self.data.labels);
            }
            Cmd::PersistIterations => {
                let _ = self.ctx.db.upsert_iterations(&self.data.iterations);
            }
            Cmd::PersistStatuses { ref project } => {
                if let Some(statuses) = self.data.work_item_statuses.get(project) {
                    let _ = self.ctx.db.set_work_item_statuses(project, statuses);
                }
            }
            Cmd::PersistViewState => {
                let ivs = ViewState {
                    conditions: self.ui.views.issue_list.filter.conditions.clone(),
                    sort_specs: self.ui.views.issue_list.filter.sort_specs.clone(),
                    fuzzy_query: self.ui.views.issue_list.filter.fuzzy_query.clone(),
                };
                let mvs = ViewState {
                    conditions: self.ui.views.mr_list.filter.conditions.clone(),
                    sort_specs: self.ui.views.mr_list.filter.sort_specs.clone(),
                    fuzzy_query: self.ui.views.mr_list.filter.fuzzy_query.clone(),
                };
                let _ = self.ctx.db.set_kv("issue_view_state", &ivs);
                let _ = self.ctx.db.set_kv("mr_view_state", &mvs);
            }
            Cmd::PersistUnplannedWork => {
                let _ = self
                    .ctx.db
                    .set_kv("unplanned_work_dates", &self.data.unplanned_work_cache);
            }
            Cmd::PersistLabelUsage => {
                let _ = self.ctx.db.set_kv("label_usage", &self.data.label_usage);
            }
            Cmd::PersistLastFetchedAt(ts) => {
                let _ = self.ctx.db.set_kv("last_fetched_at", &ts);
            }

            // ── API fetches ──────────────────────────────────────────
            Cmd::FetchAll => {
                self.ui.fetch_started_at = Some(Self::now_millis());
                self.fetch_all();
            }
            Cmd::FetchAllFull => {
                self.ui.last_fetched_at = None;
                self.data.unplanned_work_state = FetchState::Idle;
                self.ui.fetch_started_at = Some(Self::now_millis());
                self.fetch_all();
            }
            Cmd::FetchHealthData => self.maybe_fetch_health_data(),

            // ── API mutations ────────────────────────────────────────
            Cmd::SpawnCloseIssue { issue_id } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue(issue_id, serde_json::json!({"stateEvent": "CLOSE"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result));
                });
            }
            Cmd::SpawnReopenIssue { issue_id } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue(issue_id, serde_json::json!({"stateEvent": "REOPEN"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result));
                });
            }
            Cmd::SpawnCloseMr { project, iid } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_mr(&project, iid, serde_json::json!({"state_event": "close"}))
                        .await;
                    let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                });
            }
            Cmd::SpawnApproveMr { project, iid } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .approve_mr(&project, iid)
                        .await
                        .map(|()| format!("Approved !{iid}"));
                    let _ = tx.send(AsyncMsg::ActionDone(result));
                });
            }
            Cmd::SpawnMergeMr { project, iid } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client.merge_mr(&project, iid).await;
                    let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                });
            }
            Cmd::SpawnMoveIteration {
                issue_id,
                target_gid,
                old_iteration,
            } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue_iteration(issue_id, target_gid.as_deref())
                        .await;
                    let _ = tx.send(AsyncMsg::IterationUpdated(result, issue_id, old_iteration));
                });
            }
            Cmd::SpawnSetStatus {
                project,
                issue_id,
                iid,
                status_id,
                status_display,
            } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue_status(issue_id, &status_id)
                        .await
                        .map(|()| (project, iid, status_display));
                    let _ = tx.send(AsyncMsg::IssueStatusUpdated(result));
                });
            }
        }
    }
}
