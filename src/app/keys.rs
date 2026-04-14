//! Key dispatch: handle_key entry point and binding group dispatch.

use crossterm::event::KeyEvent;

use crate::cmd::EventResult;
use crate::keybindings::{self, KeyAction};

use super::{App, FocusedItem, Overlay, View};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.ui.needs_redraw = true;

        // 1. Overlay (innermost)
        let overlay_result = self.dispatch_overlay(&key);
        if overlay_result.handled() {
            return matches!(overlay_result, EventResult::Quit);
        }

        // 2. View (nav, inline modes, view-specific bindings)
        let view_result = self.dispatch_view(&key);
        if view_result.handled() {
            return matches!(view_result, EventResult::Quit);
        }

        // 3. Focused item (issue/MR actions: s/l/a/c/x/i/o/A/M)
        let item_result = self.dispatch_focused_item(&key);
        if item_result.handled() {
            return matches!(item_result, EventResult::Quit);
        }

        // 4. Global (navigation, help, quit, refresh, team switch)
        self.dispatch_global(&key)
    }

    /// Focused item handles item-specific actions (s/l/a/c/x/i/o for issues,
    /// plus A/M for MRs).  No focused item → everything bubbles.
    fn dispatch_focused_item(&mut self, key: &KeyEvent) -> EventResult {
        let bindings: &[keybindings::Binding] = match &self.ui.focused {
            Some(FocusedItem::Issue { .. }) => keybindings::ISSUE_ACTION_BINDINGS,
            Some(FocusedItem::Mr { .. }) => keybindings::MR_ACTION_BINDINGS,
            None => return EventResult::Bubble,
        };
        if let Some(action) = keybindings::match_group(bindings, key) {
            self.execute_item_action(action);
            return EventResult::Consumed;
        }
        // `o` (OpenBrowser) is in LIST_NAV_BINDINGS but is item-scoped
        if keybindings::match_group(keybindings::LIST_NAV_BINDINGS, key)
            == Some(KeyAction::OpenBrowser)
        {
            self.action_open_browser();
            return EventResult::Consumed;
        }
        EventResult::Bubble
    }

    /// Global bindings: navigation (1-4), quit (q/Esc), help (?), team (t),
    /// refresh (r/R), error (E).
    fn dispatch_global(&mut self, key: &KeyEvent) -> bool {
        // Global bindings (q, ?, Esc, E, t)
        if let Some(action) = keybindings::match_group(keybindings::GLOBAL_BINDINGS, key) {
            self.execute_global_action(action);
            return matches!(action, KeyAction::Back) && self.ui.view_stack.is_empty()
                && matches!(self.ui.overlay, Overlay::Confirm(_));
        }
        // Global navigation (1-4)
        if let Some(action) = keybindings::match_group(keybindings::GLOBAL_NAV_BINDINGS, key) {
            self.execute_global_action(action);
            return false;
        }
        // Refresh (r/R), Open detail (Enter) from LIST_NAV_BINDINGS
        if let Some(
            action @ (KeyAction::Refresh | KeyAction::FullRefresh | KeyAction::OpenDetail),
        ) = keybindings::match_group(keybindings::LIST_NAV_BINDINGS, key)
        {
            self.execute_global_action(action);
            return false;
        }
        // Filter/sort (f/F/S/Tab)
        if let Some(action) = keybindings::match_group(keybindings::FILTER_BINDINGS, key) {
            self.execute_global_action(action);
            return false;
        }
        // ReplyThread (r in detail views)
        if let Some(action @ KeyAction::ReplyThread) =
            keybindings::match_group(keybindings::DETAIL_NAV_BINDINGS, key)
        {
            self.execute_global_action(action);
            return false;
        }
        false
    }

    /// Dispatch to the active view's key handler.  Views handle their own
    /// navigation, fuzzy search, and filter bar.  Unhandled keys bubble.
    fn dispatch_view(&mut self, key: &KeyEvent) -> EventResult {
        match self.ui.view {
            View::IssueList => self.ui.views.issue_list.handle_key(
                key,
                &mut self.ui.dirty,
                &mut self.ui.pending_cmds,
                &mut self.ui.needs_redraw,
            ),
            View::MrList => self.ui.views.mr_list.handle_key(
                key,
                &mut self.ui.dirty,
                &mut self.ui.pending_cmds,
                &mut self.ui.needs_redraw,
            ),
            View::IssueDetail => self.ui.views.issue_detail.handle_key(key),
            View::MrDetail => self.ui.views.mr_detail.handle_key(key),
            View::Dashboard => self.ui.views.board.handle_key(
                key,
                self.ui.views.health.as_mut(),
                &mut self.ui.dirty,
                &mut self.ui.pending_cmds,
                &mut self.ui.needs_redraw,
            ),
            View::Planning => self.ui.views.planning.handle_key(
                key,
                &mut self.ui.dirty,
                &mut self.ui.pending_cmds,
                &mut self.ui.needs_redraw,
            ),
        }
    }

    // ── Action execution ─────────────────────────────────────────────

    /// Execute an item action (called from `dispatch_focused_item`).
    fn execute_item_action(&mut self, action: KeyAction) {
        match action {
            KeyAction::SetStatus => self.do_set_status(),
            KeyAction::ToggleState => self.do_toggle_state(),
            KeyAction::EditLabels => self.action_edit_labels(),
            KeyAction::EditAssignee => self.action_edit_assignee(),
            KeyAction::Comment => self.action_open_comment(),
            KeyAction::MoveIteration => self.show_iteration_chord(),
            KeyAction::Approve => {
                if let Some(FocusedItem::Mr {
                    ref project, iid, ..
                }) = self.ui.focused
                {
                    self.ui.overlay = Overlay::Confirm(super::ConfirmAction::ApproveMr(project.clone(), iid));
                }
            }
            KeyAction::Merge => {
                if let Some(FocusedItem::Mr {
                    ref project, iid, ..
                }) = self.ui.focused
                {
                    self.ui.overlay = Overlay::Confirm(super::ConfirmAction::MergeMr(project.clone(), iid));
                }
            }
            _ => {}
        }
    }

    /// Execute a global action (called from `dispatch_global`).
    fn execute_global_action(&mut self, action: KeyAction) {
        match action {
            KeyAction::Back => {
                if let Some(prev) = self.ui.view_stack.pop() {
                    self.ui.view = prev;
                    self.ui.dirty.selection = true;
                } else {
                    self.ui.overlay = Overlay::Confirm(super::ConfirmAction::QuitApp);
                }
            }
            KeyAction::ToggleHelp => {
                self.ui.overlay = Overlay::Help;
            }
            KeyAction::ShowLastError => {
                if let Some(err) = &self.ui.error {
                    self.ui.overlay = Overlay::Error(err.clone());
                }
            }
            KeyAction::SwitchTeam if !self.ctx.config.teams.is_empty() => {
                let mut names: Vec<String> = vec!["All".to_string()];
                names.extend(self.ctx.config.teams.iter().map(|t| t.name.clone()));
                self.ui.picker_state = Some(crate::ui::components::picker::PickerState::new("Switch Team", names, false));
                self.ui.overlay = Overlay::Picker(super::PickerContext::Team);
            }
            KeyAction::NavigateTo(target) if self.ui.view != target => {
                self.navigate_to_view(target);
            }
            KeyAction::OpenDetail => self.action_open_detail(),
            KeyAction::Refresh => {
                self.ui.loading = true;
                self.ui.pending_cmds.push(crate::cmd::Cmd::FetchAll);
            }
            KeyAction::FullRefresh => {
                self.ui.loading = true;
                self.ui.pending_cmds.push(crate::cmd::Cmd::FetchAllFull);
            }
            KeyAction::FilterMenu => self.action_show_filter_menu(),
            KeyAction::SortByField => self.action_sort_by_field(),
            KeyAction::ReplyThread => self.action_reply_thread(),
            _ => {}
        }
    }

    // ── Action helpers ───────────────────────────────────────────────

    fn navigate_to_view(&mut self, target: View) {
        self.ui.view_stack.clear();
        if target != View::Dashboard {
            self.ui.view_stack.push(View::Dashboard);
        }
        self.ui.view = target;
        // Ensure the target view has up-to-date indices
        self.ui.dirty.view_state = true;
        self.ui.dirty.selection = true;
    }
}
