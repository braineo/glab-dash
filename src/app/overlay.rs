//! Overlay key dispatch — each overlay type handles its own keys.

use crossterm::event::{KeyCode, KeyEvent};

use crate::cmd::EventResult;
use crate::ui::components::{chord_popup, input, label_editor, picker};
use crate::ui::keys;
use crate::ui::views::filter_editor;

use super::{App, ConfirmAction, Overlay};

impl App {
    /// Overlay focus: if an overlay is active, it handles the key and
    /// returns Consumed.  Returns Bubble only when no overlay is active.
    pub(super) fn dispatch_overlay(&mut self, key: &KeyEvent) -> EventResult {
        match &self.ui.overlay {
            Overlay::None => EventResult::Bubble,

            Overlay::Help => {
                if key.code == KeyCode::Char('?') || keys::is_back(key) {
                    self.ui.overlay = Overlay::None;
                }
                EventResult::Consumed
            }

            Overlay::Error(_) => {
                self.ui.overlay = Overlay::None;
                EventResult::Consumed
            }

            Overlay::Confirm(action) => {
                let action = action.clone();
                match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        if matches!(action, ConfirmAction::QuitApp) {
                            return EventResult::Quit;
                        }
                        self.execute_confirm(action);
                        self.ui.overlay = Overlay::None;
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.ui.overlay = Overlay::None;
                    }
                    _ => {}
                }
                EventResult::Consumed
            }

            Overlay::Chord(_) => {
                let Some(ref mut cs) = self.ui.chord_state else {
                    return EventResult::Bubble;
                };
                match cs.handle_key(key) {
                    chord_popup::ChordAction::Continue => {}
                    chord_popup::ChordAction::Cancel => {
                        self.ui.chord_state = None;
                        self.ui.overlay = Overlay::None;
                    }
                    chord_popup::ChordAction::Selected(value) => {
                        self.ui.chord_state = None;
                        self.handle_chord_result(&value);
                    }
                }
                EventResult::Consumed
            }

            Overlay::Picker(_) => {
                let Some(ref mut ps) = self.ui.picker_state else {
                    return EventResult::Bubble;
                };
                match ps.handle_key(key) {
                    picker::PickerAction::Continue => {}
                    picker::PickerAction::Cancel => {
                        self.ui.picker_state = None;
                        self.ui.overlay = Overlay::None;
                    }
                    picker::PickerAction::Picked(values) => {
                        self.handle_picker_result(&values);
                        if !matches!(self.ui.overlay, Overlay::CommentInput) {
                            self.ui.overlay = Overlay::None;
                        }
                        self.ui.picker_state = None;
                    }
                }
                EventResult::Consumed
            }

            Overlay::LabelEditor => {
                let Some(ref mut les) = self.ui.label_editor_state else {
                    return EventResult::Bubble;
                };
                match les.handle_key(key) {
                    label_editor::LabelEditorAction::Continue => {}
                    label_editor::LabelEditorAction::Cancel => {
                        self.ui.label_editor_state = None;
                        self.ui.overlay = Overlay::None;
                    }
                    label_editor::LabelEditorAction::Confirmed(labels) => {
                        self.handle_label_editor_result(&labels);
                        self.ui.label_editor_state = None;
                        self.ui.overlay = Overlay::None;
                    }
                }
                EventResult::Consumed
            }

            Overlay::CommentInput => {
                if self.ui.autocomplete.active {
                    if key.code == KeyCode::Tab {
                        self.accept_completion();
                        return EventResult::Consumed;
                    }
                    if key.code == KeyCode::Esc {
                        self.ui.autocomplete.dismiss();
                        return EventResult::Consumed;
                    }
                    if keys::is_nav_up(key) {
                        self.ui.autocomplete.move_up();
                        return EventResult::Consumed;
                    }
                    if keys::is_nav_down(key) {
                        self.ui.autocomplete.move_down();
                        return EventResult::Consumed;
                    }
                }
                match self.ui.comment_input.handle_key(key) {
                    input::InputAction::Cancel => {
                        self.ui.autocomplete.dismiss();
                        self.ui.overlay = Overlay::None;
                    }
                    input::InputAction::Submit => {
                        let body = self.ui.comment_input.text();
                        let body = body.trim().to_string();
                        if !body.is_empty() {
                            self.submit_comment(&body);
                        }
                        self.ui.autocomplete.dismiss();
                        self.ui.overlay = Overlay::None;
                    }
                    input::InputAction::Continue => {
                        let text = self.ui.comment_input.text();
                        let cursor = self.ui.comment_input.cursor_byte_pos();
                        let members = self.ctx.config.all_members();
                        self.ui.autocomplete
                            .update(&text, cursor, &members, &self.data.issues, &self.data.mrs);
                    }
                }
                EventResult::Consumed
            }

            Overlay::FilterEditor => {
                let action = self.ui.filter_editor_state.handle_key(key);
                if self.ui.filter_editor_state.step == filter_editor::EditorStep::EnterValue
                    && self.ui.filter_editor_state.suggestions.is_empty()
                {
                    self.ui.filter_editor_state.suggestions = self.get_filter_suggestions();
                }
                match action {
                    filter_editor::FilterEditorAction::Continue => {}
                    filter_editor::FilterEditorAction::Cancel => {
                        self.ui.overlay = Overlay::None;
                        self.action_show_filter_menu();
                    }
                    filter_editor::FilterEditorAction::AddCondition(cond) => {
                        self.active_filter_mut().conditions.push(cond);
                        self.ui.dirty.view_state = true;
                        self.ui.pending_cmds.push(crate::cmd::Cmd::PersistViewState);
                        self.ui.overlay = Overlay::None;
                        self.action_show_filter_menu();
                    }
                }
                EventResult::Consumed
            }
        }
    }
}
