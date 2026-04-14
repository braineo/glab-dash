//! Key handling for focused merge requests.

use crossterm::event::KeyEvent;

use crate::cmd::EventResult;
use crate::gitlab::types::TrackedMergeRequest;
use crate::keybindings::{self, KeyAction};
use crate::ui::components::{chord_popup, input::CommentInput, label_editor};

use super::{AppCtx, AppData, ChordContext, ConfirmAction, Overlay, PickerContext, UiState, View};

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
                ui.overlay = Overlay::Confirm(ConfirmAction::CloseMr(project, iid));
            }
            KeyAction::Approve => {
                let project = self.project_path.clone();
                let iid = self.mr.iid;
                ui.overlay = Overlay::Confirm(ConfirmAction::ApproveMr(project, iid));
            }
            KeyAction::Merge => {
                let project = self.project_path.clone();
                let iid = self.mr.iid;
                ui.overlay = Overlay::Confirm(ConfirmAction::MergeMr(project, iid));
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
                    ui.overlay = Overlay::Picker(PickerContext::Assignee);
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
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }
}
