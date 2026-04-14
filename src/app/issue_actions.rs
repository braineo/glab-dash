//! Key handling for focused issues.
//!
//! `TrackedIssue::handle_action_key` is called from `dispatch_focused_item`
//! with disjoint borrows: `&self` + `&AppData` (both immutable, from the
//! same struct), `&AppCtx` (immutable infra), `&mut UiState` (mutable UI).

use crossterm::event::KeyEvent;

use crate::cmd::{Cmd, EventResult};
use crate::gitlab::types::{ProjectLabel, TrackedIssue, User};
use crate::keybindings::{self, KeyAction};
use crate::ui::components::{chord_popup, input::CommentInput, label_editor};

use super::{AppCtx, AppData, Overlay, UiState, View};

impl TrackedIssue {
    pub fn handle_action_key(
        &self,
        key: &KeyEvent,
        ctx: &AppCtx,
        data: &AppData,
        ui: &mut UiState,
    ) -> EventResult {
        let Some(action) = keybindings::match_group(keybindings::ISSUE_ACTION_BINDINGS, key)
        else {
            if keybindings::match_group(keybindings::LIST_NAV_BINDINGS, key)
                == Some(KeyAction::OpenBrowser)
            {
                let _ = open::that_detached(&self.issue.web_url);
                return EventResult::Consumed;
            }
            return EventResult::Bubble;
        };

        match action {
            KeyAction::SetStatus => {
                Self::fetch_or_show_status_chord(
                    &self.project_path,
                    self.issue.id,
                    self.issue.iid,
                    false,
                    ctx,
                    data,
                    ui,
                );
            }
            KeyAction::ToggleState => {
                Self::fetch_or_show_status_chord(
                    &self.project_path,
                    self.issue.id,
                    self.issue.iid,
                    true,
                    ctx,
                    data,
                    ui,
                );
            }
            KeyAction::EditLabels => {
                let label_names: Vec<String> =
                    data.labels.iter().map(|l| l.name.clone()).collect();
                let issue_labels: Vec<Vec<String>> =
                    data.issues.iter().map(|i| i.issue.labels.clone()).collect();
                ui.label_editor_state = Some(label_editor::LabelEditorState::new(
                    label_names,
                    &self.issue.labels,
                    &data.label_usage,
                    &issue_labels,
                    20,
                ));
                ui.overlay = Overlay::LabelEditor;
            }
            KeyAction::EditAssignee => {
                let members = ctx.config.all_members();
                let is_detail = matches!(ui.view, View::IssueDetail);
                if is_detail {
                    ui.picker_state =
                        Some(crate::ui::components::picker::PickerState::new("Assignee", members, false));
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
            KeyAction::MoveIteration => {
                Self::show_iteration_chord(self.issue.id, data, ui);
            }
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    /// Open status chord from cached statuses, or trigger async fetch.
    fn fetch_or_show_status_chord(
        project: &str,
        issue_id: u64,
        iid: u64,
        close_only: bool,
        ctx: &AppCtx,
        data: &AppData,
        ui: &mut UiState,
    ) {
        if let Some(statuses) = data.work_item_statuses.get(project)
            && !statuses.is_empty()
        {
            Self::build_status_chord(project, issue_id, iid, close_only, statuses, data, ui);
            return;
        }
        // No cached statuses — fetch them asynchronously
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let project = project.to_string();
        ui.loading = true;
        tokio::spawn(async move {
            let result = client.fetch_work_item_statuses(&project).await;
            let _ = tx.send(super::AsyncMsg::StatusesLoaded(
                result, project, issue_id, iid, close_only,
            ));
        });
    }

    /// Build the status chord from already-cached statuses.
    pub fn build_status_chord(
        project: &str,
        issue_id: u64,
        iid: u64,
        close_only: bool,
        statuses: &[crate::gitlab::types::WorkItemStatus],
        data: &AppData,
        ui: &mut UiState,
    ) {
        let is_duplicate = |s: &crate::gitlab::types::WorkItemStatus| {
            s.name.to_lowercase().contains("duplicate")
        };

        let mut sorted_indices: Vec<usize> = (0..statuses.len())
            .filter(|&i| !is_duplicate(&statuses[i]))
            .collect();
        sorted_indices.sort_by_key(|&i| match statuses[i].category.as_deref() {
            Some("done") => 0,
            Some("active" | "opened") => 1,
            Some("canceled") => 2,
            _ => 3,
        });
        let sorted_names: Vec<String> =
            sorted_indices.iter().map(|&i| statuses[i].name.clone()).collect();
        let sorted_codes = chord_popup::generate_priority_codes(&sorted_names);

        let mut all_codes = vec![String::new(); statuses.len()];
        for (sorted_pos, &orig_idx) in sorted_indices.iter().enumerate() {
            all_codes[orig_idx].clone_from(&sorted_codes[sorted_pos]);
        }
        let all_names: Vec<String> = statuses.iter().map(|s| s.name.clone()).collect();

        let project_owned = project.to_string();

        if close_only {
            let is_close_category = |s: &crate::gitlab::types::WorkItemStatus| {
                s.category
                    .as_deref()
                    .is_some_and(|c| matches!(c, "done" | "canceled" | "closed"))
            };

            let mut close_items: Vec<(usize, &str)> = statuses
                .iter()
                .enumerate()
                .filter(|(_, s)| is_close_category(s))
                .map(|(i, s)| (i, s.category.as_deref().unwrap_or("")))
                .collect();

            if close_items.is_empty() {
                let item_state = data
                    .issues
                    .iter()
                    .find(|i| i.issue.id == issue_id)
                    .map_or("opened", |i| i.issue.state.as_str());
                Self::show_close_reopen_confirm(issue_id, iid, item_state, ui);
                return;
            }

            close_items.sort_by_key(|(_, cat)| match *cat {
                "done" => 0,
                "canceled" => 1,
                _ => 2,
            });

            let options: Vec<(String, String)> = close_items
                .iter()
                .map(|&(i, _)| (all_codes[i].clone(), all_names[i].clone()))
                .collect();
            let max_code_len = options.iter().map(|(c, _)| c.len()).max().unwrap_or(1);

            ui.chord_state = Some(
                chord_popup::ChordState::from_options("Close As", options, max_code_len)
                    .with_kind(chord_popup::ChordKind::Status),
            );
        } else {
            let options: Vec<(String, String)> = all_codes
                .into_iter()
                .zip(all_names)
                .filter(|(code, _)| !code.is_empty())
                .collect();
            let max_code_len = options.iter().map(|(c, _)| c.len()).max().unwrap_or(1);

            ui.chord_state = Some(
                chord_popup::ChordState::from_options("Set Status", options, max_code_len)
                    .with_kind(chord_popup::ChordKind::Status),
            );
        }
        ui.chord_on_complete = Some(Box::new(move |value, app| {
            app.set_issue_status(&project_owned, issue_id, iid, &value);
        }));
        ui.overlay = Overlay::Chord;
    }

    /// Show a close/reopen confirm dialog for issues without custom statuses.
    pub fn show_close_reopen_confirm(
        issue_id: u64,
        iid: u64,
        item_state: &str,
        ui: &mut UiState,
    ) {
        if item_state == "opened" {
            ui.confirm_title = "Close Issue".to_string();
            ui.confirm_message = format!("Close issue #{iid}?");
            ui.confirm_on_accept = Some(Box::new(move |app| {
                // Optimistic update
                if let Some(pos) = app.data.issues.iter().position(|i| i.issue.id == issue_id) {
                    app.data.issues[pos].issue.state = "closed".to_string();
                    app.data.issues[pos].issue.updated_at = chrono::Utc::now();
                    app.ui.dirty.issues = true;
                    app.ui.pending_cmds.push(Cmd::PersistIssues);
                }
                app.ui.pending_cmds.push(Cmd::SpawnCloseIssue { issue_id });
            }));
        } else {
            ui.confirm_title = "Reopen Issue".to_string();
            ui.confirm_message = format!("Reopen issue #{iid}?");
            ui.confirm_on_accept = Some(Box::new(move |app| {
                // Optimistic update
                if let Some(pos) = app.data.issues.iter().position(|i| i.issue.id == issue_id) {
                    app.data.issues[pos].issue.state = "opened".to_string();
                    app.data.issues[pos].issue.updated_at = chrono::Utc::now();
                    app.ui.dirty.issues = true;
                    app.ui.pending_cmds.push(Cmd::PersistIssues);
                }
                app.ui.pending_cmds.push(Cmd::SpawnReopenIssue { issue_id });
            }));
        }
        ui.overlay = Overlay::Confirm;
    }

    /// Open the iteration move chord.
    fn show_iteration_chord(issue_id: u64, data: &AppData, ui: &mut UiState) {
        let Some(pos) = data.issues.iter().position(|i| i.issue.id == issue_id) else {
            return;
        };
        let current_iter_id = data.issues[pos]
            .issue
            .iteration
            .as_ref()
            .map(|i| i.id.clone());

        let mut options: Vec<(String, String)> = Vec::new();
        options.push(("n".to_string(), "None (remove)".to_string()));

        let mut code = b'a';
        for iter in &data.iterations {
            if Some(&iter.id) != current_iter_id.as_ref() {
                let label = if iter.title.is_empty() { "Unnamed" } else { &iter.title };
                options.push((String::from(code as char), label.to_string()));
                code += 1;
                if code > b'z' {
                    break;
                }
            }
        }

        let max_code_len = options.iter().map(|(c, _)| c.len()).max().unwrap_or(1);
        ui.chord_state = Some(chord_popup::ChordState::from_options(
            "Move to Iteration",
            options,
            max_code_len,
        ));
        ui.chord_on_complete = Some(Box::new(move |value, app| {
            app.apply_iteration_move(pos, &value);
        }));
        ui.overlay = Overlay::Chord;
    }

    // ── Mutations (called from overlay completion handlers) ──────────

    /// Update labels via GraphQL diff (add/remove GIDs).
    pub fn update_labels(
        &mut self,
        labels: &[String],
        all_labels: &[ProjectLabel],
        ctx: &AppCtx,
        ui: &mut UiState,
    ) {
        let old_labels = &self.issue.labels;
        let new_set: std::collections::HashSet<&str> =
            labels.iter().map(String::as_str).collect();
        let old_set: std::collections::HashSet<&str> =
            old_labels.iter().map(String::as_str).collect();

        let label_id = |name: &str| -> Option<u64> {
            all_labels.iter().find(|l| l.name == name).map(|l| l.id)
        };

        let add_gids: Vec<String> = new_set
            .difference(&old_set)
            .filter_map(|name| label_id(name))
            .map(|id| format!("gid://gitlab/Label/{id}"))
            .collect();
        let remove_gids: Vec<String> = old_set
            .difference(&new_set)
            .filter_map(|name| label_id(name))
            .map(|id| format!("gid://gitlab/Label/{id}"))
            .collect();

        self.issue.labels = labels.to_vec();
        let issue_id = self.issue.id;
        let input = serde_json::json!({
            "labelsWidget": {
                "addLabelIds": add_gids,
                "removeLabelIds": remove_gids,
            }
        });
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        tokio::spawn(async move {
            let result = client.update_issue(issue_id, input).await;
            let _ = tx.send(super::AsyncMsg::IssueUpdated(result));
        });
        ui.dirty.issues = true;
    }

    /// Update assignee via GraphQL.
    pub fn update_assignee(&mut self, username: &str, ctx: &AppCtx, ui: &mut UiState) {
        let placeholder = User {
            id: 0,
            username: username.to_string(),
            name: username.to_string(),
            avatar_url: None,
            web_url: String::new(),
        };
        self.issue.assignees = vec![placeholder];

        let issue_id = self.issue.id;
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let username = username.to_string();
        tokio::spawn(async move {
            let users = client.search_users(&username).await;
            match users {
                Ok(users) => {
                    if let Some(user) = users.first() {
                        let input = serde_json::json!({
                            "assigneesWidget": {
                                "assigneeIds": [format!("gid://gitlab/User/{}", user.id)]
                            }
                        });
                        let result = client.update_issue(issue_id, input).await;
                        let _ = tx.send(super::AsyncMsg::IssueUpdated(result));
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
        ui.dirty.issues = true;
    }

    /// Submit a comment or reply.
    pub fn submit_comment(&self, body: &str, reply_discussion_id: Option<String>, ctx: &AppCtx, ui: &mut UiState) {
        let client = ctx.client.clone();
        let tx = ctx.async_tx.clone();
        let body = body.to_string();
        let project = self.project_path.clone();
        let iid = self.issue.iid;

        ui.loading = true;
        tokio::spawn(async move {
            let create_result = match &reply_discussion_id {
                Some(disc_id) => {
                    client.reply_to_issue_discussion(&project, iid, disc_id, &body).await
                }
                None => client.create_issue_note(&project, iid, &body).await,
            };
            if let Err(e) = create_result {
                let _ = tx.send(super::AsyncMsg::ActionDone(Err(e)));
                return;
            }
            let discussions = client.list_issue_discussions(&project, iid).await;
            let _ = tx.send(super::AsyncMsg::DiscussionsLoaded(discussions));
        });
    }
}
