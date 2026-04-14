//! Key handling for focused issues.
//!
//! `TrackedIssue::handle_action_key` is called from `dispatch_focused_item`
//! with disjoint borrows: `&self` + `&AppData` (both immutable, from the
//! same struct), `&AppCtx` (immutable infra), `&mut UiState` (mutable UI).

use crossterm::event::KeyEvent;

use crate::cmd::EventResult;
use crate::gitlab::types::TrackedIssue;
use crate::keybindings::{self, KeyAction};
use crate::ui::components::{chord_popup, input::CommentInput, label_editor};

use super::{AppCtx, AppData, ChordContext, ConfirmAction, Overlay, UiState, View};

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
                    ui.overlay = Overlay::Picker(super::PickerContext::Assignee);
                } else {
                    ui.chord_state =
                        Some(chord_popup::ChordState::new_for_names("Set Assignee", members));
                    ui.overlay = Overlay::Chord(ChordContext::Assignee);
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
    fn build_status_chord(
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
                let action = if item_state == "opened" {
                    ConfirmAction::CloseIssue(issue_id, iid)
                } else {
                    ConfirmAction::ReopenIssue(issue_id, iid)
                };
                ui.overlay = Overlay::Confirm(action);
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
        ui.overlay = Overlay::Chord(ChordContext::Status(project.to_string(), issue_id, iid));
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
        ui.overlay = Overlay::Chord(ChordContext::Iteration(pos));
    }
}
