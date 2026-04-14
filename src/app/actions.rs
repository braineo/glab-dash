//! Action methods: browser, labels, assignee, comment, status, confirm, detail navigation.

use crate::cmd::Cmd;
use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};
use crate::ui::components::picker;

use super::{
    App, ConfirmAction, FocusedItem, Overlay, PickerContext, View,
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

    /// Look up the issue shown in the detail view by its stored (project, iid).
    pub(super) fn current_detail_issue(&self) -> Option<&TrackedIssue> {
        let d = &self.ui.views.issue_detail;
        if d.project.is_empty() {
            return None;
        }
        self.data
            .issues
            .iter()
            .find(|i| i.issue.iid == d.iid && i.project_path == d.project)
            .or_else(|| {
                self.data
                    .shadow_work_cache
                    .iter()
                    .find(|i| i.issue.iid == d.iid && i.project_path == d.project)
            })
    }

    /// Look up the MR shown in the detail view by its stored (project, iid).
    pub(super) fn current_detail_mr(&self) -> Option<&TrackedMergeRequest> {
        let d = &self.ui.views.mr_detail;
        if d.project.is_empty() {
            return None;
        }
        self.data
            .mrs
            .iter()
            .find(|m| m.mr.iid == d.iid && m.project_path == d.project)
    }

    pub(super) fn action_open_detail(&mut self) {
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { project, iid, .. }) => {
                self.ui.views.issue_detail.open(&project, iid);
                self.fetch_notes_for_issue(&project, iid);
                self.ui.view_stack.push(self.ui.view);
                self.ui.view = View::IssueDetail;
            }
            Some(FocusedItem::Mr { project, iid }) => {
                self.ui.views.mr_detail.open(&project, iid);
                self.fetch_notes_for_mr(&project, iid);
                self.ui.view_stack.push(self.ui.view);
                self.ui.view = View::MrDetail;
            }
            None => {}
        }
        self.ui.dirty.selection = true;
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
