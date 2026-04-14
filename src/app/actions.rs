//! Action methods: browser, labels, assignee, comment, status, confirm, detail navigation.

use crate::cmd::Cmd;
use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest, User};
use crate::ui::components::{chord_popup, label_editor, picker};
use crate::ui::views::planning;

use super::{
    App, ChordContext, ConfirmAction, FocusedItem, Overlay, PickerContext, View,
    build_thread_picker_display,
};

impl App {
    pub(super) fn action_open_browser(&self) {
        let url = self
            .find_focused_issue()
            .map(|i| i.issue.web_url.as_str())
            .or_else(|| self.find_focused_mr().map(|m| m.mr.web_url.as_str()));
        if let Some(url) = url {
            let _ = open::that_detached(url);
        }
    }

    pub(super) fn action_edit_labels(&mut self) {
        let current = self
            .find_focused_issue()
            .map(|i| i.issue.labels.clone())
            .or_else(|| self.find_focused_mr().map(|m| m.mr.labels.clone()));
        let Some(current) = current else { return };
        let label_names: Vec<String> = self.labels.iter().map(|l| l.name.clone()).collect();
        let issue_labels: Vec<Vec<String>> =
            self.issues.iter().map(|i| i.issue.labels.clone()).collect();
        self.label_editor_state = Some(label_editor::LabelEditorState::new(
            label_names,
            &current,
            &self.label_usage,
            &issue_labels,
            20,
        ));
        self.overlay = Overlay::LabelEditor;
    }

    pub(super) fn action_edit_assignee(&mut self) {
        let members = self.picker_members();
        let is_detail = matches!(self.view, View::IssueDetail | View::MrDetail);
        if is_detail {
            self.picker_state = Some(picker::PickerState::new("Assignee", members, false));
            self.overlay = Overlay::Picker(PickerContext::Assignee);
        } else {
            self.chord_state = Some(chord_popup::ChordState::new_for_names(
                "Set Assignee",
                members,
            ));
            self.overlay = Overlay::Chord(ChordContext::Assignee);
        }
    }

    pub(super) fn action_open_comment(&mut self) {
        if self.focused.is_some() {
            self.comment_input = crate::ui::components::input::CommentInput::default();
            self.reply_discussion_id = None;
            self.overlay = Overlay::CommentInput;
        }
    }

    pub(super) fn action_reply_thread(&mut self) {
        let infos = match self.view {
            View::IssueDetail => self.views.issue_detail.thread_picker_items(),
            View::MrDetail => self.views.mr_detail.thread_picker_items(),
            _ => return,
        };
        if !infos.is_empty() {
            let (labels, subtitles) = build_thread_picker_display(&infos);
            self.picker_state = Some(
                picker::PickerState::new("Reply to thread", labels, false)
                    .with_subtitles(subtitles),
            );
            self.overlay = Overlay::Picker(PickerContext::ReplyThread(infos));
        }
    }

    /// `s` key — open full status picker/chord for the focused issue.
    pub(super) fn do_set_status(&mut self) {
        if let Some(FocusedItem::Issue {
            project, id, iid, ..
        }) = self.focused.clone()
        {
            self.fetch_statuses_and_show_chord(&project, id, iid, false);
        }
    }

    /// `x` key — close/reopen the focused item.
    /// Issues: chord picker filtered to close-category statuses (e.g. Done, Won't Do).
    /// Falls back to simple confirm if no custom statuses exist.
    /// MRs: simple close confirm.
    pub(super) fn do_toggle_state(&mut self) {
        match self.focused.clone() {
            Some(FocusedItem::Issue {
                project, id, iid, ..
            }) => {
                self.fetch_statuses_and_show_chord(&project, id, iid, true);
            }
            Some(FocusedItem::Mr { project, iid, .. }) => {
                self.overlay = Overlay::Confirm(ConfirmAction::CloseMr(project, iid));
            }
            None => {}
        }
    }

    /// Build and display the status chord popup from cached statuses.
    pub(super) fn show_status_chord(&mut self, project: &str, issue_id: u64, iid: u64, close_only: bool) {
        let Some(statuses) = self.work_item_statuses.get(project) else {
            return;
        };

        // Exclude "Duplicate" — requires linking to another issue,
        // which can't be done from a simple status change.
        let is_duplicate =
            |s: &crate::gitlab::types::WorkItemStatus| s.name.to_lowercase().contains("duplicate");

        // Filter then sort by category priority so "done" statuses get shorter codes.
        let mut sorted_indices: Vec<usize> = (0..statuses.len())
            .filter(|&i| !is_duplicate(&statuses[i]))
            .collect();
        sorted_indices.sort_by_key(|&i| match statuses[i].category.as_deref() {
            Some("done") => 0,
            Some("active" | "opened") => 1,
            Some("canceled") => 2,
            _ => 3,
        });
        let sorted_names: Vec<String> = sorted_indices
            .iter()
            .map(|&i| statuses[i].name.clone())
            .collect();
        let sorted_codes = chord_popup::generate_priority_codes(&sorted_names);

        // Map codes back to original indices
        let mut all_codes = vec![String::new(); statuses.len()];
        for (sorted_pos, &orig_idx) in sorted_indices.iter().enumerate() {
            all_codes[orig_idx].clone_from(&sorted_codes[sorted_pos]);
        }
        let all_names: Vec<String> = statuses.iter().map(|s| s.name.clone()).collect();

        if close_only {
            let is_close_category = |s: &crate::gitlab::types::WorkItemStatus| {
                s.category
                    .as_deref()
                    .is_some_and(|c| matches!(c, "done" | "canceled" | "closed"))
            };

            // Collect close statuses with their pre-computed codes
            let mut close_items: Vec<(usize, &str)> = statuses
                .iter()
                .enumerate()
                .filter(|(_, s)| is_close_category(s))
                .map(|(i, s)| (i, s.category.as_deref().unwrap_or("")))
                .collect();

            if close_items.is_empty() {
                // No close-category statuses — fall back to simple close/reopen
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
                return;
            }

            // Sort: "done" first so it gets priority in display
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

            self.chord_state = Some(
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

            self.chord_state = Some(
                chord_popup::ChordState::from_options("Set Status", options, max_code_len)
                    .with_kind(chord_popup::ChordKind::Status),
            );
        }
        self.overlay = Overlay::Chord(ChordContext::Status(project.to_string(), issue_id, iid));
    }

    pub(super) fn set_issue_status(&mut self, project: &str, issue_id: u64, iid: u64, status_name: &str) {
        // Find the status ID from cached statuses
        let status_id = self
            .work_item_statuses
            .get(project)
            .and_then(|statuses| statuses.iter().find(|s| s.name == status_name))
            .map(|s| s.id.clone());

        let Some(status_id) = status_id else {
            self.show_error(format!("Status '{status_name}' not found"));
            return;
        };

        // Optimistic update
        if let Some(pos) = self
            .issues
            .iter()
            .position(|e| e.issue.iid == iid && e.project_path == project)
        {
            self.issues[pos].issue.custom_status = Some(status_name.to_string());
            self.dirty.issues = true;
        }
        self.pending_cmds.push(Cmd::PersistIssues);
        self.pending_cmds.push(Cmd::SpawnSetStatus {
            project: project.to_string(),
            issue_id,
            iid,
            status_id,
            status_display: status_name.to_string(),
        });
    }

    pub(super) fn update_labels(&mut self, labels: &[String]) {
        let Some((idx, project, iid, is_mr)) = self.selected_item_idx() else {
            return;
        };

        let client = self.client.clone();
        let tx = self.async_tx.clone();

        if is_mr {
            self.mrs[idx].mr.labels = labels.to_vec();
            let payload = serde_json::json!({"labels": labels.join(",")});
            tokio::spawn(async move {
                let result = client.update_mr(&project, iid, payload).await;
                let _ = tx.send(super::AsyncMsg::MrUpdated(result, project));
            });
        } else {
            let old_labels = &self.issues[idx].issue.labels;
            let new_set: std::collections::HashSet<&str> =
                labels.iter().map(String::as_str).collect();
            let old_set: std::collections::HashSet<&str> =
                old_labels.iter().map(String::as_str).collect();

            let add_gids: Vec<String> = new_set
                .difference(&old_set)
                .filter_map(|name| self.label_id_by_name(name))
                .map(|id| format!("gid://gitlab/Label/{id}"))
                .collect();
            let remove_gids: Vec<String> = old_set
                .difference(&new_set)
                .filter_map(|name| self.label_id_by_name(name))
                .map(|id| format!("gid://gitlab/Label/{id}"))
                .collect();

            self.issues[idx].issue.labels = labels.to_vec();
            let issue_id = self.issues[idx].issue.id;
            let input = serde_json::json!({
                "labelsWidget": {
                    "addLabelIds": add_gids,
                    "removeLabelIds": remove_gids,
                }
            });
            tokio::spawn(async move {
                let result = client.update_issue(issue_id, input).await;
                let _ = tx.send(super::AsyncMsg::IssueUpdated(result));
            });
        }
    }

    pub(super) fn update_assignee(&mut self, username: &str) {
        let Some((idx, project, iid, is_mr)) = self.selected_item_idx() else {
            return;
        };

        // Optimistic update with a placeholder User
        let placeholder = User {
            id: 0,
            username: username.to_string(),
            name: username.to_string(),
            avatar_url: None,
            web_url: String::new(),
        };
        if is_mr {
            self.mrs[idx].mr.assignees = vec![placeholder.clone()];
        } else {
            self.issues[idx].issue.assignees = vec![placeholder];
        }

        let issue_id = if is_mr {
            0 // not used for MRs
        } else {
            self.issues[idx].issue.id
        };

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let username = username.to_string();
        tokio::spawn(async move {
            let users = client.search_users(&username).await;
            match users {
                Ok(users) => {
                    if let Some(user) = users.first() {
                        if is_mr {
                            let payload = serde_json::json!({"assignee_ids": [user.id]});
                            let result = client.update_mr(&project, iid, payload).await;
                            let _ = tx.send(super::AsyncMsg::MrUpdated(result, project));
                        } else {
                            let input = serde_json::json!({
                                "assigneesWidget": {
                                    "assigneeIds": [format!("gid://gitlab/User/{}", user.id)]
                                }
                            });
                            let result = client.update_issue(issue_id, input).await;
                            let _ = tx.send(super::AsyncMsg::IssueUpdated(result));
                        }
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
    }

    pub(super) fn submit_comment(&mut self, body: &str) {
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let body = body.to_string();

        let (project, iid, is_mr) = match self.focused.as_ref() {
            Some(FocusedItem::Issue { project, iid, .. }) => {
                (project.clone(), *iid, false)
            }
            Some(FocusedItem::Mr { project, iid }) => (project.clone(), *iid, true),
            None => return,
        };

        let reply_id = self.reply_discussion_id.take();
        self.loading = true;
        tokio::spawn(async move {
            let create_result = match &reply_id {
                Some(disc_id) => {
                    if is_mr {
                        client
                            .reply_to_mr_discussion(&project, iid, disc_id, &body)
                            .await
                    } else {
                        client
                            .reply_to_issue_discussion(&project, iid, disc_id, &body)
                            .await
                    }
                }
                None => {
                    if is_mr {
                        client.create_mr_note(&project, iid, &body).await
                    } else {
                        client.create_issue_note(&project, iid, &body).await
                    }
                }
            };
            if let Err(e) = create_result {
                let _ = tx.send(super::AsyncMsg::ActionDone(Err(e)));
                return;
            }
            // Re-fetch discussions so the UI shows the new comment
            let discussions = if is_mr {
                client.list_mr_discussions(&project, iid).await
            } else {
                client.list_issue_discussions(&project, iid).await
            };
            let _ = tx.send(super::AsyncMsg::DiscussionsLoaded(discussions));
        });
    }

    pub(super) fn accept_completion(&mut self) {
        let Some(item) = self.autocomplete.selected_item().cloned() else {
            return;
        };
        let trigger_pos = self.autocomplete.trigger_pos;
        let trigger_len =
            crate::ui::components::autocomplete::AutocompleteState::trigger_char_len();
        let text = self.comment_input.text();
        let cursor = self.comment_input.cursor_byte_pos();

        let mut new_value = String::with_capacity(text.len() + item.insert.len());
        new_value.push_str(&text[..trigger_pos + trigger_len]);
        new_value.push_str(&item.insert);
        new_value.push(' ');
        new_value.push_str(&text[cursor..]);

        let new_cursor = trigger_pos + trigger_len + item.insert.len() + 1;
        self.comment_input
            .set_text_and_cursor(&new_value, new_cursor);
        self.autocomplete.dismiss();
    }

    pub(super) fn show_error(&mut self, msg: String) {
        self.error = Some(msg.clone());
        self.overlay = Overlay::Error(msg);
    }

    /// Return the index into `self.issues` / `self.mrs` for the focused item,
    /// plus (`project_path`, iid, `is_mr`).
    fn selected_item_idx(&self) -> Option<(usize, String, u64, bool)> {
        match self.focused.as_ref()? {
            FocusedItem::Issue { project, id, iid } => {
                let idx = self.issues.iter().position(|i| i.issue.id == *id)?;
                Some((idx, project.clone(), *iid, false))
            }
            FocusedItem::Mr { project, iid } => {
                let idx = self
                    .mrs
                    .iter()
                    .position(|m| m.mr.iid == *iid && m.project_path == *project)?;
                Some((idx, project.clone(), *iid, true))
            }
        }
    }

    pub(super) fn handle_label_editor_result(&mut self, labels: &[String]) {
        for label in labels {
            *self.label_usage.entry(label.clone()).or_insert(0) += 1;
        }
        self.update_labels(labels);
        self.pending_cmds.push(Cmd::PersistLabelUsage);
    }

    fn label_id_by_name(&self, name: &str) -> Option<u64> {
        self.labels.iter().find(|l| l.name == name).map(|l| l.id)
    }

    /// Look up the focused issue by ID.  Searches both `self.issues` and
    /// `self.shadow_work_cache` (for health-panel items).
    pub(super) fn find_focused_issue(&self) -> Option<&TrackedIssue> {
        let FocusedItem::Issue { id, .. } = self.focused.as_ref()? else {
            return None;
        };
        self.issues
            .iter()
            .find(|i| i.issue.id == *id)
            .or_else(|| self.shadow_work_cache.iter().find(|i| i.issue.id == *id))
    }

    /// Look up the focused MR by (project, iid).
    pub(super) fn find_focused_mr(&self) -> Option<&TrackedMergeRequest> {
        let FocusedItem::Mr { project, iid } = self.focused.as_ref()? else {
            return None;
        };
        self.mrs
            .iter()
            .find(|m| m.mr.iid == *iid && m.project_path == *project)
    }

    pub(super) fn current_detail_issue(&self) -> Option<&TrackedIssue> {
        // The detail view shows the issue that was selected when we opened it
        self.views.issue_list.selected_issue(&self.issues)
    }

    pub(super) fn current_detail_mr(&self) -> Option<&TrackedMergeRequest> {
        self.views.mr_list.selected_mr(&self.mrs)
    }

    pub(super) fn action_open_detail(&mut self) {
        match self.view {
            View::IssueList => {
                if let Some(item) = self.views.issue_list.selected_issue(&self.issues) {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    self.views.issue_detail.reset();
                    self.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::IssueList);
                    self.view = View::IssueDetail;
                }
            }
            View::MrList => {
                if let Some(item) = self.views.mr_list.selected_mr(&self.mrs) {
                    let project = item.project_path.clone();
                    let iid = item.mr.iid;
                    self.views.mr_detail.reset();
                    self.views.mr_detail.loading_notes = true;
                    self.fetch_notes_for_mr(&project, iid);
                    self.view_stack.push(View::MrList);
                    self.view = View::MrDetail;
                }
            }
            View::Dashboard if self.views.board.health_focused => {
                if let Some(FocusedItem::Issue { project, iid, .. }) = self.focused.clone() {
                    self.sync_issue_list_for_detail(&project, iid);
                    self.views.issue_detail.reset();
                    self.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::Dashboard);
                    self.view = View::IssueDetail;
                }
            }
            View::Dashboard => {
                if let Some(item) = self
                    .views.board
                    .selected_issue(&self.issues)
                    .cloned()
                {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    // Sync issue_list_state for detail view
                    let col = self.views.board.focused_column;
                    if let Some(idx) = self
                        .views.board
                        .columns
                        .get(col)
                        .and_then(|c| c.list.selected_index())
                    {
                        if let Some(pos) = self
                            .views.issue_list
                            .list
                            .indices
                            .iter()
                            .position(|&i| i == idx)
                        {
                            self.views.issue_list.list.table_state.select(Some(pos));
                        } else {
                            self.views.issue_list.list.indices.push(idx);
                            self.views.issue_list
                                .list
                                .table_state
                                .select(Some(self.views.issue_list.list.indices.len() - 1));
                        }
                    }
                    self.views.issue_detail.reset();
                    self.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::Dashboard);
                    self.view = View::IssueDetail;
                }
            }
            View::Planning => {
                if let Some(item) = self.views.planning.selected_issue(&self.issues).cloned() {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    let col = self.views.planning.focused_column;
                    if let Some(sel) = self.views.planning.columns[col].list.table_state.selected()
                        && let Some(&idx) = self.views.planning.columns[col].list.indices.get(sel)
                    {
                        if let Some(pos) = self
                            .views.issue_list
                            .list
                            .indices
                            .iter()
                            .position(|&i| i == idx)
                        {
                            self.views.issue_list.list.table_state.select(Some(pos));
                        } else {
                            self.views.issue_list.list.indices.push(idx);
                            self.views.issue_list
                                .list
                                .table_state
                                .select(Some(self.views.issue_list.list.indices.len() - 1));
                        }
                    }
                    self.views.issue_detail.reset();
                    self.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::Planning);
                    self.view = View::IssueDetail;
                }
            }
            _ => {}
        }
        self.dirty.selection = true;
    }

    /// Ensure `issue_list_state` points at the issue identified by (project, iid)
    /// so the detail view can display it via `current_detail_issue()`.
    /// If the issue isn't in `self.issues` (e.g. shadow work from a separate cache),
    /// it is appended so the detail view can render it.
    fn sync_issue_list_for_detail(&mut self, project: &str, iid: u64) {
        let pos = self
            .issues
            .iter()
            .position(|i| i.issue.iid == iid && i.project_path == project)
            .or_else(|| {
                // Shadow work issues live in a separate cache — copy into issues
                let sw = self
                    .shadow_work_cache
                    .iter()
                    .find(|i| i.issue.iid == iid && i.project_path == project)?
                    .clone();
                self.issues.push(sw);
                Some(self.issues.len() - 1)
            });

        if let Some(pos) = pos {
            if let Some(list_pos) = self
                .views.issue_list
                .list
                .indices
                .iter()
                .position(|&i| i == pos)
            {
                self.views.issue_list
                    .list
                    .table_state
                    .select(Some(list_pos));
            } else {
                self.views.issue_list.list.indices.push(pos);
                self.views.issue_list
                    .list
                    .table_state
                    .select(Some(self.views.issue_list.list.indices.len() - 1));
            }
        }
    }

    pub(super) fn execute_confirm(&mut self, action: ConfirmAction) {
        // Optimistic updates — set dirty flags, reconcile will refilter
        match &action {
            ConfirmAction::CloseIssue(issue_id, _) => {
                if let Some(pos) = self.issues.iter().position(|i| i.issue.id == *issue_id) {
                    self.issues[pos].issue.state = "closed".to_string();
                    self.issues[pos].issue.updated_at = chrono::Utc::now();
                    self.dirty.issues = true;
                    self.pending_cmds.push(Cmd::PersistIssues);
                }
            }
            ConfirmAction::ReopenIssue(issue_id, _) => {
                if let Some(pos) = self.issues.iter().position(|i| i.issue.id == *issue_id) {
                    self.issues[pos].issue.state = "opened".to_string();
                    self.issues[pos].issue.updated_at = chrono::Utc::now();
                    self.dirty.issues = true;
                    self.pending_cmds.push(Cmd::PersistIssues);
                }
            }
            ConfirmAction::CloseMr(project, iid) => {
                if let Some(pos) = self
                    .mrs
                    .iter()
                    .position(|m| m.project_path == *project && m.mr.iid == *iid)
                {
                    self.mrs[pos].mr.state = "closed".to_string();
                    self.mrs[pos].mr.updated_at = chrono::Utc::now();
                    self.dirty.mrs = true;
                    self.pending_cmds.push(Cmd::PersistMrs);
                }
            }
            ConfirmAction::MergeMr(project, iid) => {
                if let Some(pos) = self
                    .mrs
                    .iter()
                    .position(|m| m.project_path == *project && m.mr.iid == *iid)
                {
                    self.mrs[pos].mr.state = "merged".to_string();
                    self.mrs[pos].mr.updated_at = chrono::Utc::now();
                    self.dirty.mrs = true;
                    self.pending_cmds.push(Cmd::PersistMrs);
                }
            }
            _ => {}
        }

        // Spawn API call via Cmd
        let spawn_cmd = match action {
            ConfirmAction::CloseIssue(issue_id, _) => Cmd::SpawnCloseIssue { issue_id },
            ConfirmAction::ReopenIssue(issue_id, _) => Cmd::SpawnReopenIssue { issue_id },
            ConfirmAction::CloseMr(project, iid) => Cmd::SpawnCloseMr { project, iid },
            ConfirmAction::ApproveMr(project, iid) => Cmd::SpawnApproveMr { project, iid },
            ConfirmAction::MergeMr(project, iid) => Cmd::SpawnMergeMr { project, iid },
            ConfirmAction::QuitApp => unreachable!(),
        };
        self.pending_cmds.push(spawn_cmd);
    }

    pub(super) fn show_iteration_chord(&mut self) {
        let Some(FocusedItem::Issue { id, .. }) = &self.focused else {
            return;
        };
        let Some(issue_idx) = self.issues.iter().position(|i| i.issue.id == *id) else {
            return;
        };

        // Build choices: prev / current / next / remove
        let mut labels = Vec::new();
        if let Some(iter) = &self.views.planning.prev_iteration {
            labels.push(format!("◁ {}", planning::iteration_label(iter)));
        }
        if let Some(iter) = &self.views.planning.current_iteration {
            labels.push(format!("● {}", planning::iteration_label(iter)));
        }
        if let Some(iter) = &self.views.planning.next_iteration {
            labels.push(format!("▷ {}", planning::iteration_label(iter)));
        }
        labels.push("⊘ Remove iteration".to_string());

        self.chord_state = Some(chord_popup::ChordState::new("Move to iteration", labels));
        self.overlay = Overlay::Chord(ChordContext::Iteration(issue_idx));
    }

    pub(super) fn apply_iteration_move(&mut self, issue_idx: usize, choice: &str) {
        let target = if choice.starts_with('◁') {
            self.views.planning.prev_iteration.clone()
        } else if choice.starts_with('●') {
            self.views.planning.current_iteration.clone()
        } else if choice.starts_with('▷') {
            self.views.planning.next_iteration.clone()
        } else {
            // Remove iteration
            None
        };

        let issue_id = self.issues[issue_idx].issue.id;
        let old_iteration = self.issues[issue_idx].issue.iteration.clone();

        // Optimistic update
        self.issues[issue_idx].issue.iteration.clone_from(&target);
        self.issues[issue_idx].issue.updated_at = chrono::Utc::now();
        self.dirty.issues = true;

        let target_gid = target.as_ref().map(|i| i.id.clone());
        self.pending_cmds.push(Cmd::SpawnMoveIteration {
            issue_id,
            target_gid,
            old_iteration,
        });
        self.pending_cmds.push(Cmd::FetchHealthData);
    }
}
