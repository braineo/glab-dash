//! Action methods: browser, labels, assignee, comment, status, detail navigation.

use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};

use super::{App, FocusedItem, Overlay, View};

impl App {
    pub(super) fn set_issue_status(
        &mut self,
        project: &str,
        issue_id: u64,
        iid: u64,
        status_name: &str,
    ) {
        // Find the status from cached statuses
        let status = self
            .data
            .work_item_statuses
            .get(project)
            .and_then(|statuses| statuses.iter().find(|s| s.name == status_name));

        let Some(status) = status else {
            self.show_error(format!("Status '{status_name}' not found"));
            return;
        };

        let status_id = status.id.clone();
        let status_category = status.category.clone();

        // Optimistic update
        if let Some(pos) = self
            .data
            .issues
            .iter()
            .position(|e| e.issue.iid == iid && e.project_path == project)
        {
            self.data.issues[pos].issue.custom_status = Some(status_name.to_string());
            self.data.issues[pos].issue.custom_status_category = status_category;
            self.ui.dirty.issues = true;
        }
        self.ui.pending_cmds.push(crate::cmd::Cmd::PersistIssues);
        self.ui.pending_cmds.push(crate::cmd::Cmd::SpawnSetStatus {
            project: project.to_string(),
            issue_id,
            iid,
            status_id,
            status_display: status_name.to_string(),
        });
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
        self.ui
            .pending_cmds
            .push(crate::cmd::Cmd::PersistLabelUsage);
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
                if let Some(mr) = self
                    .data
                    .mrs
                    .iter_mut()
                    .find(|m| m.mr.iid == iid && m.project_path == project)
                {
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
                if let Some(mr) = self
                    .data
                    .mrs
                    .iter_mut()
                    .find(|m| m.mr.iid == iid && m.project_path == project)
                {
                    mr.update_assignee(username, &self.ctx, &mut self.ui);
                }
            }
            None => {}
        }
    }

    /// Dispatch comment submit to the focused issue or MR.
    pub(super) fn dispatch_submit_comment(
        &mut self,
        body: &str,
        reply_discussion_id: Option<&str>,
    ) {
        let reply_id = reply_discussion_id.map(String::from);
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { id, .. }) => {
                if let Some(issue) = self.data.issues.iter().find(|i| i.issue.id == id) {
                    issue.submit_comment(body, reply_id, &self.ctx, &mut self.ui);
                }
            }
            Some(FocusedItem::Mr { project, iid }) => {
                if let Some(mr) = self
                    .data
                    .mrs
                    .iter()
                    .find(|m| m.mr.iid == iid && m.project_path == project)
                {
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

    pub(super) fn apply_iteration_move(
        &mut self,
        issue_id: u64,
        target: Option<&crate::gitlab::types::Iteration>,
    ) {
        let issue_idx = self.data.issues.iter().position(|i| i.issue.id == issue_id);
        let Some(issue_idx) = issue_idx else {
            return;
        };

        let old_iteration = self.data.issues[issue_idx].issue.iteration.clone();

        // Optimistic update
        self.data.issues[issue_idx].issue.iteration = target.cloned();
        self.data.issues[issue_idx].issue.updated_at = chrono::Utc::now();
        self.ui.dirty.issues = true;

        let target_gid = target.as_ref().map(|i| i.id.clone());
        self.ui
            .pending_cmds
            .push(crate::cmd::Cmd::SpawnMoveIteration {
                issue_id,
                target_gid,
                old_iteration,
            });
        self.ui.pending_cmds.push(crate::cmd::Cmd::FetchHealthData);
    }
}
