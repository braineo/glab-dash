//! Key handling for focused merge requests.

use crossterm::event::KeyEvent;

use crate::cmd::{Cmd, EventResult};
use crate::gitlab::types::{TrackedMergeRequest, User};
use crate::keybindings::{self, KeyAction};
use crate::ui::components::{chord_popup, input::CommentInput, label_editor};

use super::{AppCtx, AppData, Overlay, UiState, View};

impl TrackedMergeRequest {
    pub fn handle_action_key(
        &self,
        key: &KeyEvent,
        ctx: &AppCtx,
        data: &AppData,
        ui: &mut UiState,
    ) -> EventResult {
        let Some(action) = keybindings::match_group(keybindings::MR_ACTION_BINDINGS, key) else {
            if keybindings::match_group(keybindings::LIST_NAV_BINDINGS, key)
                == Some(KeyAction::OpenBrowser)
            {
                let _ = open::that_detached(&self.mr.web_url);
                return EventResult::Consumed;
            }
            return EventResult::Bubble;
        };

        match action {
            KeyAction::ToggleState => {
                let project = self.project_path.clone();
                let iid = self.mr.iid;
                ui.confirm_title = "Close MR".to_string();
                ui.confirm_message = format!("Close MR !{iid}?");
                ui.confirm_on_accept = Some(Box::new(move |app| {
                    if let Some(pos) = app
                        .data.mrs
                        .iter()
                        .position(|m| m.project_path == project && m.mr.iid == iid)
                    {
                        app.data.mrs[pos].mr.state = "closed".to_string();
                        app.data.mrs[pos].mr.updated_at = chrono::Utc::now();
                        app.ui.dirty.mrs = true;
                        app.ui.pending_cmds.push(Cmd::PersistMrs);
                    }
                    app.ui.pending_cmds.push(Cmd::SpawnCloseMr { project, iid });
                }));
                ui.overlay = Overlay::Confirm;
            }
            KeyAction::Approve => {
                let project = self.project_path.clone();
                let iid = self.mr.iid;
                ui.confirm_title = "Approve MR".to_string();
                ui.confirm_message = format!("Approve MR !{iid}?");
                ui.confirm_on_accept = Some(Box::new(move |app| {
                    app.ui.pending_cmds.push(Cmd::SpawnApproveMr { project, iid });
                }));
                ui.overlay = Overlay::Confirm;
            }
            KeyAction::Merge => {
                let project = self.project_path.clone();
                let iid = self.mr.iid;
                ui.confirm_title = "Merge MR".to_string();
                ui.confirm_message = format!("Merge MR !{iid}?");
                ui.confirm_on_accept = Some(Box::new(move |app| {
                    if let Some(pos) = app
                        .data.mrs
                        .iter()
                        .position(|m| m.project_path == project && m.mr.iid == iid)
                    {
                        app.data.mrs[pos].mr.state = "merged".to_string();
                        app.data.mrs[pos].mr.updated_at = chrono::Utc::now();
                        app.ui.dirty.mrs = true;
                        app.ui.pending_cmds.push(Cmd::PersistMrs);
                    }
                    app.ui.pending_cmds.push(Cmd::SpawnMergeMr { project, iid });
                }));
                ui.overlay = Overlay::Confirm;
            }
            KeyAction::EditLabels => {
                let label_names: Vec<String> =
                    data.labels.iter().map(|l| l.name.clone()).collect();
                let issue_labels: Vec<Vec<String>> =
                    data.issues.iter().map(|i| i.issue.labels.clone()).collect();
                ui.label_editor_state = Some(label_editor::LabelEditorState::new(
                    label_names,
                    &self.mr.labels,
                    &data.label_usage,
                    &issue_labels,
                    20,
                ));
                ui.overlay = Overlay::LabelEditor;
            }
            KeyAction::EditAssignee => {
                let members = ctx.config.all_members();
                let is_detail = matches!(ui.view, View::MrDetail);
                if is_detail {
                    ui.picker_state = Some(
                        crate::ui::components::picker::PickerState::new("Assignee", members, false),
                    );
                    ui.picker_on_complete = Some(Box::new(|values, app| {
                        if let Some(username) = values.first() {
                            app.dispatch_update_assignee(username);
                        }
                    }));
                    ui.overlay = Overlay::Picker;
                } else {
                    ui.chord_state =
                        Some(chord_popup::ChordState::new_for_names("Set Assignee", members));
                    ui.chord_on_complete = Some(Box::new(|value, app| {
                        app.dispatch_update_assignee(&value);
                    }));
                    ui.overlay = Overlay::Chord;
                }
            }
            KeyAction::Comment => {
                ui.comment_input = CommentInput::default();
                ui.reply_discussion_id = None;
                ui.overlay = Overlay::CommentInput;
            }
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    // ── Mutations (called from overlay completion handlers) ──────────

    /// Update labels via REST API.
    pub fn update_labels(&mut self, labels: &[String], ctx: &AppCtx, ui: &mut UiState) {
        self.mr.labels = labels.to_vec();
        let project = self.project_path.clone();
        let iid = self.mr.iid;
        let payload = serde_json::json!({"labels": labels.join(",")});
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.update_mr(&project, iid, payload).await;
            let _ = tx.send(super::AsyncMsg::MrUpdated(result, project));
        });
        ui.dirty.mrs = true;
    }

    /// Update assignee via REST API.
    pub fn update_assignee(&mut self, username: &str, ctx: &AppCtx, ui: &mut UiState) {
        let placeholder = User {
            id: 0,
            username: username.to_string(),
            name: username.to_string(),
            avatar_url: None,
            web_url: String::new(),
        };
        self.mr.assignees = vec![placeholder];

        let project = self.project_path.clone();
        let iid = self.mr.iid;
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let username = username.to_string();
        tokio::spawn(async move {
            let users = client.search_users(&username).await;
            match users {
                Ok(users) => {
                    if let Some(user) = users.first() {
                        let payload = serde_json::json!({"assignee_ids": [user.id]});
                        let result = client.update_mr(&project, iid, payload).await;
                        let _ = tx.send(super::AsyncMsg::MrUpdated(result, project));
                    } else {
                        let _ = tx.send(super::AsyncMsg::ActionDone(Err(anyhow::anyhow!(
                            "User '{username}' not found"
                        ))));
                    }
                }
                Err(e) => {
                    let _ = tx.send(super::AsyncMsg::ActionDone(Err(e)));
                }
            }
        });
        ui.dirty.mrs = true;
    }

    /// Submit a comment or reply.
    pub fn submit_comment(&self, body: &str, reply_discussion_id: Option<String>, ctx: &AppCtx, ui: &mut UiState) {
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let body = body.to_string();
        let project = self.project_path.clone();
        let iid = self.mr.iid;

        ui.loading = true;
        tokio::spawn(async move {
            let create_result = match &reply_discussion_id {
                Some(disc_id) => {
                    client.reply_to_mr_discussion(&project, iid, disc_id, &body).await
                }
                None => client.create_mr_note(&project, iid, &body).await,
            };
            if let Err(e) = create_result {
                let _ = tx.send(super::AsyncMsg::ActionDone(Err(e)));
                return;
            }
            let discussions = client.list_mr_discussions(&project, iid).await;
            let _ = tx.send(super::AsyncMsg::DiscussionsLoaded(discussions));
        });
    }
}
