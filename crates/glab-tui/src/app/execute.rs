use anyhow::Result;
use glab_api::GitLabClient;
use glab_core::domain::ItemKind;

use crate::cmd::Cmd;

use super::{App, AsyncMsg, FetchState, ViewState};

impl App {
    pub(super) fn execute_pending_cmds(&mut self) {
        let cmds = std::mem::take(&mut self.ui.pending_cmds);
        for cmd in cmds {
            self.execute_cmd(cmd);
        }
    }

    fn execute_cmd(&mut self, cmd: Cmd) {
        match cmd {
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

                let team = self
                    .ui
                    .active_team
                    .and_then(|i| self.ctx.config.teams.get(i))
                    .map(|t| t.name.clone());
                let _ = self.ctx.db.set_kv("active_team", &team);
                let _ = self
                    .ctx
                    .db
                    .set_kv("theme", &crate::ui::styles::theme_name().to_string());
            }
            Cmd::PersistUnplannedWork => {
                let _ = self
                    .ctx
                    .db
                    .set_kv("unplanned_work_dates", &self.data.unplanned_work_cache);
            }
            Cmd::PersistLabelUsage => {
                let _ = self.ctx.db.set_kv("label_usage", &self.data.label_usage);
            }
            Cmd::PersistLastFetchedAt(ts) => {
                let _ = self.ctx.db.set_kv("last_fetched_at", &ts);
            }

            Cmd::FetchAll => self.fetch_all(),
            Cmd::FetchAllFull => {
                if !self.fetch_in_flight() {
                    self.ui.last_fetched_at = None;
                    self.data.unplanned_work_state = FetchState::Idle;
                    self.fetch_all();
                }
            }
            Cmd::FetchHealthData => self.maybe_fetch_health_data(),

            Cmd::SpawnCloseIssue { issue_id } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue(&issue_id, serde_json::json!({"stateEvent": "CLOSE"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result));
                });
            }
            Cmd::SpawnReopenIssue { issue_id } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue(&issue_id, serde_json::json!({"stateEvent": "REOPEN"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result));
                });
            }
            Cmd::SpawnCloseMr { project, iid } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client.close_mr(&project, &iid).await;
                    let _ = tx.send(AsyncMsg::MrUpdated(result));
                });
            }
            Cmd::SpawnApproveMr { project, iid } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .approve_mr(&project, &iid)
                        .await
                        .map(|()| format!("Approved !{iid}"));
                    let _ = tx.send(AsyncMsg::ActionDone(result));
                });
            }
            Cmd::SpawnMergeMr { project, iid } => {
                let client = self.ctx.client.clone();
                let tx = self.ctx.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .merge_mr(&project, &iid)
                        .await
                        .map(|()| format!("Merged !{iid}"));
                    let _ = tx.send(AsyncMsg::ActionDone(result));
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
                        .update_issue_iteration(&issue_id, target_gid.as_deref())
                        .await;
                    let _ = tx.send(AsyncMsg::IterationUpdated(result, issue_id, old_iteration));
                });
            }
            // GitLab owns the link ids, so a write re-reads rather than
            // patching.
            Cmd::FetchRelated { kind, gid } => {
                self.refresh_related(vec![(kind, gid)], |_, _| async { Ok(()) });
            }
            Cmd::AddLink {
                gid,
                target_gid,
                relation,
            } => {
                self.refresh_related(vec![(ItemKind::Issue, gid)], move |client, gid| async move {
                    client.add_link(&gid, &target_gid, relation).await
                });
            }
            // The line lands in the merge request, but the issue's own list is
            // GitLab's reading of that same line, so both sides re-read.
            Cmd::MentionInMr {
                gid,
                target_gid,
                relation,
            } => {
                let reads = vec![
                    (ItemKind::MergeRequest, gid),
                    (ItemKind::Issue, target_gid.clone()),
                ];
                self.refresh_related(reads, move |client, mr_gid| async move {
                    client.mention_in_mr(&mr_gid, &target_gid, relation).await
                });
            }
            Cmd::RemoveLink { gid, target_gid } => {
                self.refresh_related(
                    vec![(ItemKind::Issue, gid)],
                    move |client, gid| async move { client.unlink(&gid, &target_gid).await },
                );
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
                        .update_issue_status(&issue_id, &status_id)
                        .await
                        .map(|()| (project, iid, status_display));
                    let _ = tx.send(AsyncMsg::IssueStatusUpdated(result));
                });
            }
        }
    }

    /// Re-reads every item in `reads`, not just the one written: one write can
    /// change both sides.  A plain read passes a `write` that does nothing.
    fn refresh_related<F, Fut>(&self, reads: Vec<(ItemKind, String)>, write: F)
    where
        F: FnOnce(GitLabClient, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send,
    {
        let client = self.ctx.client.clone();
        let tx = self.ctx.async_tx.clone();
        tokio::spawn(async move {
            let Some((_, written)) = reads.first().cloned() else {
                return;
            };
            if let Err(e) = write(client.clone(), written.clone()).await {
                let _ = tx.send(AsyncMsg::RelatedLoaded(Err(e), written));
                return;
            }
            for (kind, gid) in reads {
                let result = client.list_related(kind, &gid).await;
                let _ = tx.send(AsyncMsg::RelatedLoaded(result, gid));
            }
        });
    }
}
