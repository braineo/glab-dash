//! Action methods: browser, labels, assignee, comment, status, confirm, detail navigation.

use crate::cmd::Cmd;
use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};
use crate::ui::components::{chord_popup, picker};

use super::{
    App, ChordContext, ConfirmAction, FocusedItem, Overlay, PickerContext, View,
    build_thread_picker_display,
};

impl App {
    pub(super) fn action_reply_thread(&mut self) {
        let infos = match self.ui.view {
            View::IssueDetail => self.ui.views.issue_detail.thread_picker_items(),
            View::MrDetail => self.ui.views.mr_detail.thread_picker_items(),
            _ => return,
        };
        if !infos.is_empty() {
            let (labels, subtitles) = build_thread_picker_display(&infos);
            self.ui.picker_state = Some(
                picker::PickerState::new("Reply to thread", labels, false)
                    .with_subtitles(subtitles),
            );
            self.ui.overlay = Overlay::Picker(PickerContext::ReplyThread(infos));
        }
    }

    /// Build and display the status chord popup from cached statuses.
    pub(super) fn show_status_chord(&mut self, project: &str, issue_id: u64, iid: u64, close_only: bool) {
        let Some(statuses) = self.data.work_item_statuses.get(project) else {
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
                    .ui.views.issue_list
                    .selected_issue(&self.data.issues)
                    .or_else(|| self.current_detail_issue())
                    .map_or("opened", |i| i.issue.state.as_str());
                let action = if item_state == "opened" {
                    ConfirmAction::CloseIssue(issue_id, iid)
                } else {
                    ConfirmAction::ReopenIssue(issue_id, iid)
                };
                self.ui.overlay = Overlay::Confirm(action);
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

            self.ui.chord_state = Some(
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

            self.ui.chord_state = Some(
                chord_popup::ChordState::from_options("Set Status", options, max_code_len)
                    .with_kind(chord_popup::ChordKind::Status),
            );
        }
        self.ui.overlay = Overlay::Chord(ChordContext::Status(project.to_string(), issue_id, iid));
    }

    pub(super) fn set_issue_status(&mut self, project: &str, issue_id: u64, iid: u64, status_name: &str) {
        // Find the status ID from cached statuses
        let status_id = self
            .data.work_item_statuses
            .get(project)
            .and_then(|statuses| statuses.iter().find(|s| s.name == status_name))
            .map(|s| s.id.clone());

        let Some(status_id) = status_id else {
            self.show_error(format!("Status '{status_name}' not found"));
            return;
        };

        // Optimistic update
        if let Some(pos) = self
            .data.issues
            .iter()
            .position(|e| e.issue.iid == iid && e.project_path == project)
        {
            self.data.issues[pos].issue.custom_status = Some(status_name.to_string());
            self.ui.dirty.issues = true;
        }
        self.ui.pending_cmds.push(Cmd::PersistIssues);
        self.ui.pending_cmds.push(Cmd::SpawnSetStatus {
            project: project.to_string(),
            issue_id,
            iid,
            status_id,
            status_display: status_name.to_string(),
        });
    }

    pub(super) fn accept_completion(&mut self) {
        let Some(item) = self.ui.autocomplete.selected_item().cloned() else {
            return;
        };
        let trigger_pos = self.ui.autocomplete.trigger_pos;
        let trigger_len =
            crate::ui::components::autocomplete::AutocompleteState::trigger_char_len();
        let text = self.ui.comment_input.text();
        let cursor = self.ui.comment_input.cursor_byte_pos();

        let mut new_value = String::with_capacity(text.len() + item.insert.len());
        new_value.push_str(&text[..trigger_pos + trigger_len]);
        new_value.push_str(&item.insert);
        new_value.push(' ');
        new_value.push_str(&text[cursor..]);

        let new_cursor = trigger_pos + trigger_len + item.insert.len() + 1;
        self.ui.comment_input
            .set_text_and_cursor(&new_value, new_cursor);
        self.ui.autocomplete.dismiss();
    }

    pub(super) fn show_error(&mut self, msg: String) {
        self.ui.error = Some(msg.clone());
        self.ui.overlay = Overlay::Error(msg);
    }


    pub(super) fn handle_label_editor_result(&mut self, labels: &[String]) {
        for label in labels {
            *self.data.label_usage.entry(label.clone()).or_insert(0) += 1;
        }
        self.dispatch_update_labels(labels);
        self.ui.pending_cmds.push(Cmd::PersistLabelUsage);
    }

    /// Dispatch label update to the focused issue or MR.
    pub(super) fn dispatch_update_labels(&mut self, labels: &[String]) {
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { id, .. }) => {
                if let Some(issue) = self.data.issues.iter_mut().find(|i| i.issue.id == id) {
                    issue.update_labels(labels, &self.data.labels, &self.ctx, &mut self.ui);
                }
            }
            Some(FocusedItem::Mr { project, iid }) => {
                if let Some(mr) = self.data.mrs.iter_mut().find(|m| m.mr.iid == iid && m.project_path == project) {
                    mr.update_labels(labels, &self.ctx, &mut self.ui);
                }
            }
            None => {}
        }
    }

    /// Dispatch assignee update to the focused issue or MR.
    pub(super) fn dispatch_update_assignee(&mut self, username: &str) {
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { id, .. }) => {
                if let Some(issue) = self.data.issues.iter_mut().find(|i| i.issue.id == id) {
                    issue.update_assignee(username, &self.ctx, &mut self.ui);
                }
            }
            Some(FocusedItem::Mr { project, iid }) => {
                if let Some(mr) = self.data.mrs.iter_mut().find(|m| m.mr.iid == iid && m.project_path == project) {
                    mr.update_assignee(username, &self.ctx, &mut self.ui);
                }
            }
            None => {}
        }
    }

    /// Dispatch comment submit to the focused issue or MR.
    pub(super) fn dispatch_submit_comment(&mut self, body: &str) {
        let reply_id = self.ui.reply_discussion_id.take();
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { id, .. }) => {
                if let Some(issue) = self.data.issues.iter().find(|i| i.issue.id == id) {
                    issue.submit_comment(body, reply_id, &self.ctx, &mut self.ui);
                }
            }
            Some(FocusedItem::Mr { project, iid }) => {
                if let Some(mr) = self.data.mrs.iter().find(|m| m.mr.iid == iid && m.project_path == project) {
                    mr.submit_comment(body, reply_id, &self.ctx, &mut self.ui);
                }
            }
            None => {}
        }
    }

    pub(super) fn current_detail_issue(&self) -> Option<&TrackedIssue> {
        // The detail view shows the issue that was selected when we opened it
        self.ui.views.issue_list.selected_issue(&self.data.issues)
    }

    pub(super) fn current_detail_mr(&self) -> Option<&TrackedMergeRequest> {
        self.ui.views.mr_list.selected_mr(&self.data.mrs)
    }

    pub(super) fn action_open_detail(&mut self) {
        match self.ui.view {
            View::IssueList => {
                if let Some(item) = self.ui.views.issue_list.selected_issue(&self.data.issues) {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    self.ui.views.issue_detail.reset();
                    self.ui.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.ui.view_stack.push(View::IssueList);
                    self.ui.view = View::IssueDetail;
                }
            }
            View::MrList => {
                if let Some(item) = self.ui.views.mr_list.selected_mr(&self.data.mrs) {
                    let project = item.project_path.clone();
                    let iid = item.mr.iid;
                    self.ui.views.mr_detail.reset();
                    self.ui.views.mr_detail.loading_notes = true;
                    self.fetch_notes_for_mr(&project, iid);
                    self.ui.view_stack.push(View::MrList);
                    self.ui.view = View::MrDetail;
                }
            }
            View::Dashboard if self.ui.views.board.health_focused => {
                if let Some(FocusedItem::Issue { project, iid, .. }) = self.ui.focused.clone() {
                    self.sync_issue_list_for_detail(&project, iid);
                    self.ui.views.issue_detail.reset();
                    self.ui.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.ui.view_stack.push(View::Dashboard);
                    self.ui.view = View::IssueDetail;
                }
            }
            View::Dashboard => {
                if let Some(item) = self
                    .ui.views.board
                    .selected_issue(&self.data.issues)
                    .cloned()
                {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    // Sync issue_list_state for detail view
                    let col = self.ui.views.board.focused_column;
                    if let Some(idx) = self
                        .ui.views.board
                        .columns
                        .get(col)
                        .and_then(|c| c.list.selected_index())
                    {
                        if let Some(pos) = self
                            .ui.views.issue_list
                            .list
                            .indices
                            .iter()
                            .position(|&i| i == idx)
                        {
                            self.ui.views.issue_list.list.table_state.select(Some(pos));
                        } else {
                            self.ui.views.issue_list.list.indices.push(idx);
                            self.ui.views.issue_list
                                .list
                                .table_state
                                .select(Some(self.ui.views.issue_list.list.indices.len() - 1));
                        }
                    }
                    self.ui.views.issue_detail.reset();
                    self.ui.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.ui.view_stack.push(View::Dashboard);
                    self.ui.view = View::IssueDetail;
                }
            }
            View::Planning => {
                if let Some(item) = self.ui.views.planning.selected_issue(&self.data.issues).cloned() {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    let col = self.ui.views.planning.focused_column;
                    if let Some(sel) = self.ui.views.planning.columns[col].list.table_state.selected()
                        && let Some(&idx) = self.ui.views.planning.columns[col].list.indices.get(sel)
                    {
                        if let Some(pos) = self
                            .ui.views.issue_list
                            .list
                            .indices
                            .iter()
                            .position(|&i| i == idx)
                        {
                            self.ui.views.issue_list.list.table_state.select(Some(pos));
                        } else {
                            self.ui.views.issue_list.list.indices.push(idx);
                            self.ui.views.issue_list
                                .list
                                .table_state
                                .select(Some(self.ui.views.issue_list.list.indices.len() - 1));
                        }
                    }
                    self.ui.views.issue_detail.reset();
                    self.ui.views.issue_detail.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.ui.view_stack.push(View::Planning);
                    self.ui.view = View::IssueDetail;
                }
            }
            _ => {}
        }
        self.ui.dirty.selection = true;
    }

    /// Ensure `issue_list_state` points at the issue identified by (project, iid)
    /// so the detail view can display it via `current_detail_issue()`.
    /// If the issue isn't in `self.data.issues` (e.g. shadow work from a separate cache),
    /// it is appended so the detail view can render it.
    fn sync_issue_list_for_detail(&mut self, project: &str, iid: u64) {
        let pos = self
            .data.issues
            .iter()
            .position(|i| i.issue.iid == iid && i.project_path == project)
            .or_else(|| {
                // Shadow work issues live in a separate cache — copy into issues
                let sw = self
                    .data.shadow_work_cache
                    .iter()
                    .find(|i| i.issue.iid == iid && i.project_path == project)?
                    .clone();
                self.data.issues.push(sw);
                Some(self.data.issues.len() - 1)
            });

        if let Some(pos) = pos {
            if let Some(list_pos) = self
                .ui.views.issue_list
                .list
                .indices
                .iter()
                .position(|&i| i == pos)
            {
                self.ui.views.issue_list
                    .list
                    .table_state
                    .select(Some(list_pos));
            } else {
                self.ui.views.issue_list.list.indices.push(pos);
                self.ui.views.issue_list
                    .list
                    .table_state
                    .select(Some(self.ui.views.issue_list.list.indices.len() - 1));
            }
        }
    }

    pub(super) fn execute_confirm(&mut self, action: ConfirmAction) {
        // Optimistic updates — set dirty flags, reconcile will refilter
        match &action {
            ConfirmAction::CloseIssue(issue_id, _) => {
                if let Some(pos) = self.data.issues.iter().position(|i| i.issue.id == *issue_id) {
                    self.data.issues[pos].issue.state = "closed".to_string();
                    self.data.issues[pos].issue.updated_at = chrono::Utc::now();
                    self.ui.dirty.issues = true;
                    self.ui.pending_cmds.push(Cmd::PersistIssues);
                }
            }
            ConfirmAction::ReopenIssue(issue_id, _) => {
                if let Some(pos) = self.data.issues.iter().position(|i| i.issue.id == *issue_id) {
                    self.data.issues[pos].issue.state = "opened".to_string();
                    self.data.issues[pos].issue.updated_at = chrono::Utc::now();
                    self.ui.dirty.issues = true;
                    self.ui.pending_cmds.push(Cmd::PersistIssues);
                }
            }
            ConfirmAction::CloseMr(project, iid) => {
                if let Some(pos) = self
                    .data.mrs
                    .iter()
                    .position(|m| m.project_path == *project && m.mr.iid == *iid)
                {
                    self.data.mrs[pos].mr.state = "closed".to_string();
                    self.data.mrs[pos].mr.updated_at = chrono::Utc::now();
                    self.ui.dirty.mrs = true;
                    self.ui.pending_cmds.push(Cmd::PersistMrs);
                }
            }
            ConfirmAction::MergeMr(project, iid) => {
                if let Some(pos) = self
                    .data.mrs
                    .iter()
                    .position(|m| m.project_path == *project && m.mr.iid == *iid)
                {
                    self.data.mrs[pos].mr.state = "merged".to_string();
                    self.data.mrs[pos].mr.updated_at = chrono::Utc::now();
                    self.ui.dirty.mrs = true;
                    self.ui.pending_cmds.push(Cmd::PersistMrs);
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
        self.ui.pending_cmds.push(spawn_cmd);
    }

    pub(super) fn apply_iteration_move(&mut self, issue_idx: usize, choice: &str) {
        let target = if choice.starts_with('◁') {
            self.ui.views.planning.prev_iteration.clone()
        } else if choice.starts_with('●') {
            self.ui.views.planning.current_iteration.clone()
        } else if choice.starts_with('▷') {
            self.ui.views.planning.next_iteration.clone()
        } else {
            // Remove iteration
            None
        };

        let issue_id = self.data.issues[issue_idx].issue.id;
        let old_iteration = self.data.issues[issue_idx].issue.iteration.clone();

        // Optimistic update
        self.data.issues[issue_idx].issue.iteration.clone_from(&target);
        self.data.issues[issue_idx].issue.updated_at = chrono::Utc::now();
        self.ui.dirty.issues = true;

        let target_gid = target.as_ref().map(|i| i.id.clone());
        self.ui.pending_cmds.push(Cmd::SpawnMoveIteration {
            issue_id,
            target_gid,
            old_iteration,
        });
        self.ui.pending_cmds.push(Cmd::FetchHealthData);
    }
}
