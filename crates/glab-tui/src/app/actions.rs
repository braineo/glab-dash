//! Action methods: browser, labels, assignee, comment, status, detail navigation.

use glab_core::domain::ItemRef;
use glab_core::domain::{Issue, Item, MergeRequest, StatusValue};

use crate::ui::components::related;

use super::issue_actions::IssueActions;
use super::item_actions;
use super::mr_actions::MrActions;
use super::{App, FocusedItem, Overlay, View};

impl App {
    pub(super) fn set_issue_status(
        &mut self,
        project: &str,
        issue_id: &str,
        iid: &str,
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
            .position(|e| e.iid == iid && e.project_path() == project)
        {
            self.data.issues[pos].status = Some(StatusValue {
                name: status_name.to_string(),
                category: status_category,
            });
            self.ui.dirty.issues = true;
        }
        self.ui.pending_cmds.push(crate::cmd::Cmd::PersistIssues);
        self.ui.pending_cmds.push(crate::cmd::Cmd::SpawnSetStatus {
            project: project.to_string(),
            issue_id: issue_id.to_string(),
            iid: iid.to_string(),
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
                if let Some(issue) = self.data.issues.iter_mut().find(|i| i.id == id) {
                    issue.update_labels(labels, &self.data.labels, &self.ctx, &mut self.ui);
                }
            }
            Some(FocusedItem::Mr { project, iid }) => {
                if let Some(mr) = self
                    .data
                    .mrs
                    .iter_mut()
                    .find(|m| m.iid == iid && m.project_path() == project)
                {
                    mr.update_labels(labels, &self.data.labels, &self.ctx, &mut self.ui);
                }
            }
            None => {}
        }
    }

    /// Dispatch assignee update to the focused issue or MR.
    pub(super) fn dispatch_update_assignee(&mut self, username: &str) {
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { id, .. }) => {
                if let Some(issue) = self.data.issues.iter_mut().find(|i| i.id == id) {
                    issue.update_assignee(username, &self.ctx, &mut self.ui);
                }
            }
            Some(FocusedItem::Mr { project, iid }) => {
                if let Some(mr) = self
                    .data
                    .mrs
                    .iter_mut()
                    .find(|m| m.iid == iid && m.project_path() == project)
                {
                    mr.update_assignee(username, &self.ctx, &mut self.ui);
                }
            }
            None => {}
        }
    }

    /// Both kinds take the same path; this only says which item.
    pub(super) fn dispatch_submit_comment(
        &mut self,
        body: &str,
        target: crate::ui::components::input::CommentTarget,
    ) {
        if let Some(focused) = self.ui.focused.clone() {
            item_actions::submit_comment(
                &focused.item_ref(),
                body,
                target,
                &self.ctx,
                &mut self.ui,
            );
        }
    }

    /// Look up the issue shown in the detail view by its stored gid.
    pub(super) fn current_detail_issue(&self) -> Option<&Issue> {
        super::issue_by_id(&self.data, &self.ui.views.issue_detail.id)
    }

    /// Look up the MR shown in the detail view by its stored (project, iid).
    pub(super) fn current_detail_mr(&self) -> Option<&MergeRequest> {
        let d = &self.ui.views.mr_detail;
        if d.project.is_empty() {
            return None;
        }
        self.data
            .mrs
            .iter()
            .find(|m| m.iid == d.iid && m.project_path() == d.project)
    }

    pub(super) fn action_open_detail(&mut self) {
        match self.ui.focused.clone() {
            Some(FocusedItem::Issue { id, project, iid }) => {
                self.open_issue_detail(&id, &project, &iid);
                self.ui.view_stack.push(self.ui.view);
                self.ui.view = View::IssueDetail;
            }
            Some(FocusedItem::Mr { project, iid }) => {
                self.ui.views.mr_detail.open(&project, &iid);
                self.fetch_notes_for_mr(&project, &iid);
                self.ui.view_stack.push(self.ui.view);
                self.ui.view = View::MrDetail;
            }
            None => {}
        }
        self.ui.dirty.selection = true;
    }

    /// Does not switch the view: the caller decides what to stack.
    pub(super) fn open_issue_detail(&mut self, id: &str, project: &str, iid: &str) {
        self.ui.views.issue_detail.open(id, project, iid);
        self.fetch_notes_for_issue(project, iid);
        self.ui
            .pending_cmds
            .push(crate::cmd::Cmd::FetchRelated(ItemRef::issue(project, iid)));
        self.ui.dirty.selection = true;
    }

    /// A fetched item opens in the detail view; one outside the team's scope
    /// has no local copy to render, so it opens in the browser.
    ///
    /// ponytail: a chain of blockers cannot be walked back item by item; give
    /// the detail view its own stack if that bites.
    pub(super) fn action_open_related(&mut self) {
        let detail = &self.ui.views.issue_detail;
        let Some(target) = self
            .data
            .related_by_item
            .get(&detail.item())
            .and_then(|related| related::at_cursor(related, &detail.body))
            .cloned()
        else {
            return;
        };
        let reference = target.item.reference();
        let Some(issue) = self.data.issues.iter().find(|i| i.reference == reference) else {
            let _ = open::that_detached(&target.web_url);
            return;
        };
        let (id, project, iid) = (
            issue.id.clone(),
            issue.project_path().to_string(),
            issue.iid.clone(),
        );
        self.open_issue_detail(&id, &project, &iid);
    }

    pub(super) fn apply_iteration_move(
        &mut self,
        issue_id: &str,
        target: Option<&glab_core::domain::Iteration>,
    ) {
        let issue_idx = self.data.issues.iter().position(|i| i.id == issue_id);
        let Some(issue_idx) = issue_idx else {
            return;
        };

        let old_iteration = self.data.issues[issue_idx].iteration.clone();

        // Optimistic update
        self.data.issues[issue_idx].iteration = target.cloned();
        self.data.issues[issue_idx].updated_at = chrono::Utc::now();
        self.ui.dirty.issues = true;

        let target_gid = target.as_ref().map(|i| i.id.clone());
        self.ui
            .pending_cmds
            .push(crate::cmd::Cmd::SpawnMoveIteration {
                issue_id: issue_id.to_string(),
                target_gid,
                old_iteration,
            });
        self.ui.pending_cmds.push(crate::cmd::Cmd::FetchHealthData);
    }
}
