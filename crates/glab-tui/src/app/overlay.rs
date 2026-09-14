//! Overlay key dispatch — each overlay type handles its own keys.

use crossterm::event::{KeyCode, KeyEvent};

use crate::cmd::EventResult;
use crate::ui::components::{chord_popup, input, label_editor, picker};
use crate::ui::keys;
use crate::ui::views::filter_editor;

use super::{App, Overlay};

impl App {
    /// Overlay focus: if an overlay is active, it handles the key and
    /// returns Consumed.  Returns Bubble only when no overlay is active.
    pub(super) fn dispatch_overlay(&mut self, key: &KeyEvent) -> EventResult {
        // Take the overlay out so we can destructure it with owned access
        // while still having `&mut self` for callbacks.
        let overlay = std::mem::replace(&mut self.ui.overlay, Overlay::None);

        match overlay {
            Overlay::None => {
                self.ui.overlay = Overlay::None;
                EventResult::Bubble
            }

            Overlay::Help => {
                if key.code == KeyCode::Char('?') || keys::is_back(key) {
                    // overlay already None
                } else {
                    self.ui.overlay = Overlay::Help;
                }
                EventResult::Consumed
            }

            Overlay::Error(_) => {
                // Any key dismisses error; overlay already None
                EventResult::Consumed
            }

            Overlay::Confirm {
                title,
                message,
                on_accept,
            } => {
                match key.code {
                    KeyCode::Char('y' | 'Y') => match on_accept {
                        Some(cb) => cb(self),
                        None => return EventResult::Quit,
                    },
                    KeyCode::Char('n') | KeyCode::Esc => {}
                    _ => {
                        // Unrecognized key — put overlay back
                        self.ui.overlay = Overlay::Confirm {
                            title,
                            message,
                            on_accept,
                        };
                    }
                }
                EventResult::Consumed
            }

            Overlay::Chord {
                mut state,
                on_complete,
            } => {
                match state.handle_key(key) {
                    chord_popup::ChordAction::Continue => {
                        self.ui.overlay = Overlay::Chord { state, on_complete };
                    }
                    chord_popup::ChordAction::Cancel => {} // overlay already None
                    chord_popup::ChordAction::Selected(value) => {
                        on_complete(value, self);
                    }
                }
                EventResult::Consumed
            }

            Overlay::Picker {
                mut state,
                on_complete,
            } => {
                match state.handle_key(key) {
                    picker::PickerAction::Continue => {
                        self.ui.overlay = Overlay::Picker { state, on_complete };
                    }
                    picker::PickerAction::Cancel => {} // overlay already None
                    picker::PickerAction::Picked(values) => {
                        on_complete(values, self);
                    }
                }
                EventResult::Consumed
            }

            Overlay::LabelEditor { mut state } => {
                match state.handle_key(key) {
                    label_editor::LabelEditorAction::Continue => {
                        self.ui.overlay = Overlay::LabelEditor { state };
                    }
                    label_editor::LabelEditorAction::Cancel => {} // overlay already None
                    label_editor::LabelEditorAction::Confirmed(labels) => {
                        self.handle_label_editor_result(&labels);
                    }
                }
                EventResult::Consumed
            }

            Overlay::CommentInput {
                mut input,
                mut autocomplete,
                target,
            } => {
                // Handle autocomplete keys first if active; each of these
                // only steers the popup, leaving the draft open.
                let steered = autocomplete.active
                    && match key.code {
                        KeyCode::Tab => {
                            if let Some(item) = autocomplete.selected_item().cloned() {
                                input.replace_before_cursor(
                                    autocomplete.query.chars().count(),
                                    &item.insert,
                                );
                            }
                            autocomplete.dismiss();
                            true
                        }
                        KeyCode::Esc => {
                            autocomplete.dismiss();
                            true
                        }
                        _ if keys::is_nav_up(key) => {
                            autocomplete.move_up();
                            true
                        }
                        _ if keys::is_nav_down(key) => {
                            autocomplete.move_down();
                            true
                        }
                        _ => false,
                    };

                // Cancel and submit are the only two that close the draft, and
                // the overlay is already `None` for them.
                if !steered {
                    match input.handle_key(key) {
                        input::InputAction::Cancel => return EventResult::Consumed,
                        input::InputAction::Submit => {
                            let body = input.text();
                            let body = body.trim().to_string();
                            if !body.is_empty() {
                                self.dispatch_submit_comment(&body, target);
                            }
                            return EventResult::Consumed;
                        }
                        // No completion popup while isearch is moving the cursor.
                        input::InputAction::Continue if input.is_searching() => {
                            autocomplete.dismiss();
                        }
                        input::InputAction::Continue => {
                            let text = input.text();
                            let cursor = input.cursor_byte_pos();
                            let members = self.ctx.config.all_members();
                            autocomplete.update(
                                &text,
                                cursor,
                                &members,
                                &self.data.team_issues,
                                &self.data.team_mrs,
                            );
                        }
                    }
                }

                self.ui.overlay = Overlay::CommentInput {
                    input,
                    autocomplete,
                    target,
                };
                EventResult::Consumed
            }

            Overlay::FilterEditor(mut state) => {
                let action = state.handle_key(key);
                if state.step == filter_editor::EditorStep::EnterValue
                    && state.suggestions.is_empty()
                {
                    state.suggestions = self.get_filter_suggestions();
                }
                match action {
                    filter_editor::FilterEditorAction::Continue => {
                        self.ui.overlay = Overlay::FilterEditor(state);
                    }
                    filter_editor::FilterEditorAction::Cancel => {
                        self.action_show_filter_menu();
                    }
                    filter_editor::FilterEditorAction::AddCondition(cond) => {
                        self.active_filter_mut().conditions.push(cond);
                        self.ui.dirty.view_state = true;
                        self.ui.pending_cmds.push(crate::cmd::Cmd::PersistViewState);
                        self.action_show_filter_menu();
                    }
                }
                EventResult::Consumed
            }
        }
    }
}
