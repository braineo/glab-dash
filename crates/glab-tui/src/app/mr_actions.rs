//! What a focused merge request answers to differently from an issue; the rest
//! is in [`item_actions`](super::item_actions).

use glab_core::domain::{Item, ItemKind, MergeRequest, ProjectLabel, User};

use crate::cmd::{Cmd, EventResult};
use crate::keybindings::KeyAction;

use super::item_actions;
use super::{AppCtx, AppData, Overlay, UiState};

use crate::binding_group;

binding_group! {
    pub MR_ACTION_GROUP: "MR Actions" {
        ('A') => Approve | "A" "Approve MR",
        ('M') => Merge | "M" "Merge MR",
        ('x') => ToggleState | "x" "Close MR",
        ('l') => EditLabels | "l" "Set labels",
        ('a') => EditAssignee | "a" "Set assignee",
        ('c') => Comment | "c" "Add comment",
        ('o') => OpenBrowser | "o" "Open in browser",
    }
}

pub trait MrActions {
    fn handle_action_key(
        &self,
        action: KeyAction,
        ctx: &AppCtx,
        data: &AppData,
        ui: &mut UiState,
    ) -> EventResult;
    fn update_labels(
        &mut self,
        labels: &[String],
        all_labels: &[ProjectLabel],
        ctx: &AppCtx,
        ui: &mut UiState,
    );
    fn update_assignee(&mut self, username: &str, ctx: &AppCtx, ui: &mut UiState);
    fn resolve_thread(&self, ctx: &AppCtx, ui: &mut UiState);
}

impl MrActions for MergeRequest {
    fn handle_action_key(
        &self,
        action: KeyAction,
        ctx: &AppCtx,
        data: &AppData,
        ui: &mut UiState,
    ) -> EventResult {
        match action {
            KeyAction::ToggleState => {
                let project = self.project_path().to_string();
                let iid = self.iid.clone();
                ui.overlay = Overlay::Confirm {
                    title: "Close MR".to_string(),
                    message: format!("Close MR !{iid}?"),
                    on_accept: Some(Box::new(move |app| {
                        if let Some(pos) = app
                            .data
                            .mrs
                            .iter()
                            .position(|m| m.project_path() == project && m.iid == iid)
                        {
                            app.data.mrs[pos].state = "closed".to_string();
                            app.data.mrs[pos].updated_at = chrono::Utc::now();
                            app.ui.dirty.mrs = true;
                            app.ui.pending_cmds.push(Cmd::PersistMrs);
                        }
                        app.ui.pending_cmds.push(Cmd::SpawnCloseMr { project, iid });
                    })),
                };
            }
            KeyAction::ResolveThread => {
                self.resolve_thread(ctx, ui);
            }
            KeyAction::Approve => {
                let project = self.project_path().to_string();
                let iid = self.iid.clone();
                ui.overlay = Overlay::Confirm {
                    title: "Approve MR".to_string(),
                    message: format!("Approve MR !{iid}?"),
                    on_accept: Some(Box::new(move |app| {
                        app.ui
                            .pending_cmds
                            .push(Cmd::SpawnApproveMr { project, iid });
                    })),
                };
            }
            KeyAction::Merge => {
                let project = self.project_path().to_string();
                let iid = self.iid.clone();
                ui.overlay = Overlay::Confirm {
                    title: "Merge MR".to_string(),
                    message: format!("Merge MR !{iid}?"),
                    on_accept: Some(Box::new(move |app| {
                        if let Some(pos) = app
                            .data
                            .mrs
                            .iter()
                            .position(|m| m.project_path() == project && m.iid == iid)
                        {
                            app.data.mrs[pos].state = "merged".to_string();
                            app.data.mrs[pos].updated_at = chrono::Utc::now();
                            app.ui.dirty.mrs = true;
                            app.ui.pending_cmds.push(Cmd::PersistMrs);
                        }
                        app.ui.pending_cmds.push(Cmd::SpawnMergeMr { project, iid });
                    })),
                };
            }
            _ => return item_actions::handle_key(action, self, ctx, data, ui),
        }
        EventResult::Consumed
    }

    fn update_labels(
        &mut self,
        labels: &[String],
        all_labels: &[ProjectLabel],
        ctx: &AppCtx,
        ui: &mut UiState,
    ) {
        self.labels = labels.to_vec();
        let project = self.project_path().to_string();
        let iid = self.iid.clone();
        // The mutation takes label GIDs; a title with no match is dropped.
        let label_ids: Vec<u64> = labels
            .iter()
            .filter_map(|name| all_labels.iter().find(|l| l.name == *name).map(|l| l.id))
            .collect();
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.set_mr_labels(&project, &iid, &label_ids).await;
            let _ = tx.send(super::AsyncMsg::MrUpdated(result));
        });
        ui.dirty.mrs = true;
    }

    /// `mergeRequestSetAssignees` takes usernames, so no lookup is needed.
    fn update_assignee(&mut self, username: &str, ctx: &AppCtx, ui: &mut UiState) {
        self.assignees = vec![User {
            id: String::new(),
            username: username.to_string(),
        }];

        let project = self.project_path().to_string();
        let iid = self.iid.clone();
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let usernames = vec![username.to_string()];
        tokio::spawn(async move {
            let result = client.set_mr_assignees(&project, &iid, &usernames).await;
            let _ = tx.send(super::AsyncMsg::MrUpdated(result));
        });
        ui.dirty.mrs = true;
    }

    /// Re-lists the threads afterwards, so the view shows what the server
    /// settled on.
    fn resolve_thread(&self, ctx: &AppCtx, ui: &mut UiState) {
        let Some(thread) = ui
            .views
            .mr_detail
            .conversation
            .thread_at_cursor(&ui.views.mr_detail.body)
        else {
            return;
        };
        if !thread.resolvable() {
            ui.error = Some("That thread cannot be resolved".to_string());
            return;
        }
        let discussion = thread.id.clone();
        let resolved = !thread.resolved();

        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let project = self.project_path().to_string();
        let iid = self.iid.clone();
        ui.loading = true;
        tokio::spawn(async move {
            if let Err(e) = client
                .resolve_discussion(&project, &iid, &discussion, resolved)
                .await
            {
                let _ = tx.send(super::AsyncMsg::ActionDone(Err(e)));
                return;
            }
            let discussions = client
                .list_discussions(ItemKind::MergeRequest, &project, &iid)
                .await;
            let _ = tx.send(super::AsyncMsg::DiscussionsLoaded(discussions));
        });
    }
}
