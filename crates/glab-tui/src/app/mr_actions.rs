//! Key handling for focused merge requests.
//!
//! `MergeRequest` lives in `glab-core`, so these are hung off it with the
//! [`MrActions`] extension trait rather than an inherent impl.

use glab_api::Issuable;

use crate::cmd::{Cmd, EventResult};
use crate::keybindings::KeyAction;
use crate::ui::components::{chord_popup, input::CommentInput, label_editor};
use glab_core::domain::{MergeRequest, ProjectLabel, User};

use super::{AppCtx, AppData, Overlay, UiState, View};

/// Merge-request actions that need the app's context, ui and data. Implemented
/// for `MergeRequest`, which this crate does not own.
use crate::binding_group;

binding_group! {
    /// What a focused merge request answers to, wherever the cursor is on one.
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
    /// Handle a key press against the MR-action bindings.
    fn handle_action_key(
        &self,
        action: KeyAction,
        ctx: &AppCtx,
        data: &AppData,
        ui: &mut UiState,
    ) -> EventResult;
    /// Replace the MR's labels and push the change to the API.
    fn update_labels(
        &mut self,
        labels: &[String],
        all_labels: &[ProjectLabel],
        ctx: &AppCtx,
        ui: &mut UiState,
    );
    /// Assign the MR to `username`, optimistically updating in place.
    fn update_assignee(&mut self, username: &str, ctx: &AppCtx, ui: &mut UiState);
    /// Resolve or reopen the thread the detail view's cursor is on.
    fn resolve_thread(&self, ctx: &AppCtx, ui: &mut UiState);
    /// Post `body` as a new comment or as a reply to an existing thread.
    fn submit_comment(
        &self,
        body: &str,
        reply_discussion_id: Option<String>,
        ctx: &AppCtx,
        ui: &mut UiState,
    );
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
            KeyAction::OpenBrowser => {
                if let Some(url) = &self.web_url {
                    let _ = open::that_detached(url);
                }
            }
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
            KeyAction::EditLabels => {
                let label_names: Vec<String> = data.labels.iter().map(|l| l.name.clone()).collect();
                let issue_labels: Vec<Vec<String>> =
                    data.issues.iter().map(|i| i.labels.clone()).collect();
                ui.overlay = Overlay::LabelEditor {
                    state: label_editor::LabelEditorState::new(
                        label_names,
                        &self.labels,
                        &data.label_usage,
                        &issue_labels,
                        20,
                    ),
                };
            }
            KeyAction::EditAssignee => {
                let members = ctx.config.all_members();
                let is_detail = matches!(ui.view, View::MrDetail);
                if is_detail {
                    ui.overlay = Overlay::Picker {
                        state: crate::ui::components::picker::PickerState::new(
                            "Assignee", members, false,
                        ),
                        on_complete: Box::new(|values, app| {
                            if let Some(username) = values.first() {
                                app.dispatch_update_assignee(username);
                            }
                        }),
                    };
                } else {
                    ui.overlay = Overlay::Chord {
                        state: chord_popup::ChordState::new_for_names("Set Assignee", members),
                        on_complete: Box::new(|value, app| {
                            app.dispatch_update_assignee(&value);
                        }),
                    };
                }
            }
            KeyAction::Comment => {
                ui.overlay = Overlay::CommentInput {
                    input: CommentInput::default(),
                    autocomplete: Box::default(),
                    reply_discussion_id: None,
                };
            }
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    // ── Mutations (called from overlay completion handlers) ──────────

    /// Replace labels via `mergeRequestSetLabels`.
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
        // The mutation takes label GIDs, so resolve each title against the
        // project's label list; a title with no match is dropped.
        let label_ids: Vec<u64> = labels
            .iter()
            .filter_map(|name| all_labels.iter().find(|l| l.name == *name).map(|l| l.id))
            .collect();
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.set_mr_labels(&project, &iid, &label_ids).await;
            let _ = tx.send(super::AsyncMsg::MrUpdated(result, project));
        });
        ui.dirty.mrs = true;
    }

    /// Replace assignees via `mergeRequestSetAssignees`, which takes usernames
    /// directly — no `search_users` round-trip needed.
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
            let _ = tx.send(super::AsyncMsg::MrUpdated(result, project));
        });
        ui.dirty.mrs = true;
    }

    /// Resolve or reopen the thread the detail view's cursor is on, then
    /// re-list the threads so the view shows what the server settled on.
    fn resolve_thread(&self, ctx: &AppCtx, ui: &mut UiState) {
        let Some(thread) = ui.views.mr_detail.conversation.thread_at_cursor() else {
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
                .list_discussions(Issuable::MergeRequest, &project, &iid)
                .await;
            let _ = tx.send(super::AsyncMsg::DiscussionsLoaded(discussions));
        });
    }
    fn submit_comment(
        &self,
        body: &str,
        reply_discussion_id: Option<String>,
        ctx: &AppCtx,
        ui: &mut UiState,
    ) {
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let body = body.to_string();
        let project = self.project_path().to_string();
        let iid = self.iid.clone();

        ui.loading = true;
        tokio::spawn(async move {
            let create_result = match &reply_discussion_id {
                Some(disc_id) => client
                    .reply_to_discussion(Issuable::MergeRequest, &project, &iid, disc_id, &body)
                    .await
                    .map(|_| ()),
                None => client
                    .create_thread(Issuable::MergeRequest, &project, &iid, &body)
                    .await
                    .map(|_| ()),
            };
            if let Err(e) = create_result {
                let _ = tx.send(super::AsyncMsg::ActionDone(Err(e)));
                return;
            }
            let discussions = client
                .list_discussions(Issuable::MergeRequest, &project, &iid)
                .await;
            let _ = tx.send(super::AsyncMsg::DiscussionsLoaded(discussions));
        });
    }
}
