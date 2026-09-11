//! Key dispatch: handle_key entry point and binding group dispatch.

use crossterm::event::KeyEvent;

use crate::binding_group;
use crate::cmd::{Cmd, Effects, EventResult};
use crate::keybindings::BindingGroup;
use crate::keybindings::{self, KeyAction};
use crate::ui::components::picker;
use crate::ui::views::Views;

use super::issue_actions::{self, IssueActions};
use super::mr_actions::{self, MrActions};
use super::{App, FocusedItem, Overlay, View};

binding_group! {
    /// Keys the app answers to everywhere.  Outermost in the chain, so a view
    /// or the focused item shadows any it re-binds — a detail view's `r`
    /// (reply) beats `r` (refresh) here, and help drops the shadowed row.
    GLOBAL_GROUP: "Global" {
        ('q') => Back | "q" "Back / Quit",
        ('?') => ToggleHelp | "?" "Toggle help",
        (key Esc) => Back | "Esc" "Go back / close",
        ('E') => ShowLastError | "E" "Show last error",
        ('t') => SwitchTeam | "t" "Switch team",
        ('T') => SwitchTheme | "T" "Switch theme",
        ('r') => Refresh | "r" "Refresh data",
        ('R') => FullRefresh | "R" "Full refresh (re-fetch all)",
    }
}

binding_group! {
    /// Jumping straight to a view.  The tab bar shows these already.
    GLOBAL_NAV_GROUP: "Navigation" {
        ('1') => NavigateTo(View::Dashboard) | "1" "Dashboard (home)",
        ('2') => NavigateTo(View::IssueList) | "2" "Go to issues",
        ('3') => NavigateTo(View::MrList) | "3" "Go to merge requests",
        ('4') => NavigateTo(View::Planning) | "4" "Go to planning",
    }
}

/// The outermost layer of the chain, tried after the view and the focused item.
static GLOBAL_GROUPS: &[&BindingGroup] = &[&GLOBAL_GROUP, &GLOBAL_NAV_GROUP];

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.ui.needs_redraw = true;

        // An open overlay owns raw presses: while a comment or a filter value
        // is being typed, `f` is text, not the filter menu.
        let overlay = self.dispatch_overlay(&key);
        if overlay.handled() {
            return matches!(overlay, EventResult::Quit);
        }

        // One key, one action, decided once against the live chain.  Views are
        // still handed the raw key for their own text-input modes, which read
        // it and ignore the action.
        let action = keybindings::resolve(&self.active_groups(), &key);

        let view = self.dispatch_view(&key, action);
        if view.handled() {
            return matches!(view, EventResult::Quit);
        }
        let Some(action) = action else {
            self.ui.needs_redraw = false; // an unbound key changed nothing
            return false;
        };
        let item = self.dispatch_focused_item(action);
        if item.handled() {
            return matches!(item, EventResult::Quit);
        }
        // Nothing below claimed it and no arm here matched: the key is bound to
        // an action nothing implements.  Trips in debug so a dead binding shows
        // up the first time it is pressed rather than silently doing nothing.
        let handled = self.execute_global_action(action);
        debug_assert!(
            handled,
            "{action:?} is bound to a key but nothing handles it"
        );
        false
    }

    /// The groups specific to what is on screen: the active view's, then the
    /// item under the cursor.  The status bar hints from these — the globals
    /// are on the tab bar already.
    pub fn contextual_groups(&self) -> Vec<&'static BindingGroup> {
        let mut groups = Views::binding_groups(self.ui.view).to_vec();
        match self.ui.focused {
            Some(FocusedItem::Issue { .. }) => groups.push(&issue_actions::ISSUE_ACTION_GROUP),
            Some(FocusedItem::Mr { .. }) => groups.push(&mr_actions::MR_ACTION_GROUP),
            None => {}
        }
        groups
    }

    /// The full chain, innermost first — the same order `handle_key` walks, so
    /// the group that shadows a key is the one that handles it.  This is the
    /// only place that order is written down; help reads it too.
    pub fn active_groups(&self) -> Vec<&'static BindingGroup> {
        let mut groups = self.contextual_groups();
        groups.extend(GLOBAL_GROUPS);
        groups
    }

    /// Focused item handles item-specific actions.  The domain type
    /// (`Issue` / `MergeRequest`) owns its key handling.
    fn dispatch_focused_item(&mut self, action: KeyAction) -> EventResult {
        let focused = match &self.ui.focused {
            Some(f) => f.clone(),
            None => return EventResult::Bubble,
        };
        // Disjoint borrows: &data (immutable) + &ctx (immutable) + &mut ui (mutable)
        match &focused {
            FocusedItem::Issue { id, .. } => {
                let Some(issue) = super::issue_by_id(&self.data, id) else {
                    return EventResult::Bubble;
                };
                issue.handle_action_key(action, &self.ctx, &self.data, &mut self.ui)
            }
            FocusedItem::Mr { project, iid } => {
                let Some(mr) = self
                    .data
                    .mrs
                    .iter()
                    .find(|m| m.iid == *iid && m.project_path() == *project)
                else {
                    return EventResult::Bubble;
                };
                mr.handle_action_key(action, &self.ctx, &self.data, &mut self.ui)
            }
        }
    }

    /// Dispatch to the active view's key handler.  Views handle their own
    /// navigation, fuzzy search, and filter bar.  Unhandled keys bubble.
    fn dispatch_view(&mut self, key: &KeyEvent, action: Option<KeyAction>) -> EventResult {
        let ui = &mut self.ui;
        let mut fx = Effects {
            dirty: &mut ui.dirty,
            cmds: &mut ui.pending_cmds,
            needs_redraw: &mut ui.needs_redraw,
        };
        match ui.view {
            View::IssueList => ui.views.issue_list.handle_key(key, action, &mut fx),
            View::MrList => ui.views.mr_list.handle_key(key, action, &mut fx),
            View::IssueDetail => ui.views.issue_detail.handle_key(action, &mut ui.overlay),
            View::MrDetail => ui.views.mr_detail.handle_key(action, &mut ui.overlay),
            View::Dashboard => {
                ui.views
                    .board
                    .handle_key(key, action, ui.views.health.as_mut(), &mut fx)
            }
            View::Planning => ui.views.planning.handle_key(key, action, &mut fx),
        }
    }

    /// Run an action no view or focused item claimed.  Exhaustive on purpose:
    /// an action that reaches here with no arm is a binding nothing implements.
    fn execute_global_action(&mut self, action: KeyAction) -> bool {
        match action {
            KeyAction::Back => {
                if let Some(prev) = self.ui.view_stack.pop() {
                    self.ui.view = prev;
                    self.ui.dirty.selection = true;
                } else {
                    self.ui.overlay = Overlay::Confirm {
                        title: "Quit".to_string(),
                        message: "Quit glab-dash?".to_string(),
                        on_accept: None,
                    };
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
            KeyAction::SwitchTeam => {
                if self.ctx.config.teams.is_empty() {
                    return true;
                }
                let mut names: Vec<String> = vec!["All".to_string()];
                names.extend(self.ctx.config.teams.iter().map(|t| t.name.clone()));
                self.ui.overlay = Overlay::Picker {
                    state: picker::PickerState::new("Switch Team", names, false),
                    on_complete: Box::new(|values, app| {
                        if let Some(name) = values.first() {
                            if name == "All" {
                                app.ui.active_team = None;
                            } else {
                                app.ui.active_team =
                                    app.ctx.config.teams.iter().position(|t| t.name == *name);
                            }
                            app.ui.dirty.issues = true;
                            app.ui.dirty.mrs = true;
                            app.ui.dirty.statuses = true;
                            app.ui.dirty.selection = true;

                            app.ui.pending_cmds.push(Cmd::FetchAll);
                            app.ui.pending_cmds.push(Cmd::PersistViewState);
                        }
                    }),
                };
            }
            KeyAction::SwitchTheme => {
                self.ui.overlay = Overlay::Picker {
                    state: picker::PickerState::new(
                        "Switch Theme",
                        crate::ui::styles::theme_names(),
                        false,
                    )
                    .with_preview(
                        crate::ui::styles::theme_name(),
                        crate::ui::styles::set_theme,
                    ),
                    on_complete: Box::new(|values, app| {
                        if let Some(name) = values.first()
                            && crate::ui::styles::set_theme(name)
                        {
                            app.ui.pending_cmds.push(Cmd::PersistViewState);
                        }
                    }),
                };
            }
            KeyAction::NavigateTo(target) => {
                if self.ui.view != target {
                    self.navigate_to_view(target);
                }
            }
            KeyAction::OpenDetail => self.action_open_detail(),
            KeyAction::Refresh => self.ui.pending_cmds.push(crate::cmd::Cmd::FetchAll),
            KeyAction::FullRefresh => self.ui.pending_cmds.push(crate::cmd::Cmd::FetchAllFull),
            KeyAction::FilterMenu => self.action_show_filter_menu(),
            KeyAction::SortByField => self.action_sort_by_field(),
            KeyAction::ClearFilters => self.action_clear_filters(),
            KeyAction::FocusFilterBar => {
                // Nothing to walk when no condition is set, and the bar would
                // swallow every key until Esc.
                let filter = self.active_filter_mut();
                filter.bar_focused = !filter.conditions.is_empty();
                filter.bar_selected = 0;
            }
            _ => return false,
        }
        true
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybindings::{KeyMatcher, active_bindings};
    use crossterm::event::{KeyCode, KeyModifiers};

    impl KeyMatcher {
        /// The key event this matcher accepts, so a test can feed a declared
        /// binding back through `resolve`.
        fn to_event(self) -> KeyEvent {
            match self {
                Self::Char(c) => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                Self::Ctrl(c) => KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL),
                Self::Key(code) => KeyEvent::new(code, KeyModifiers::NONE),
            }
        }
    }

    /// The chain for `view` with `item` under the cursor, built the same way
    /// `active_groups` builds it.
    fn chain(view: View, item: Option<&FocusedItem>) -> Vec<&'static BindingGroup> {
        let mut groups = Views::binding_groups(view).to_vec();
        match item {
            Some(FocusedItem::Issue { .. }) => groups.push(&issue_actions::ISSUE_ACTION_GROUP),
            Some(FocusedItem::Mr { .. }) => groups.push(&mr_actions::MR_ACTION_GROUP),
            None => {}
        }
        groups.extend(GLOBAL_GROUPS);
        groups
    }

    fn an_issue() -> FocusedItem {
        FocusedItem::Issue {
            project: "g/p".into(),
            id: "1".into(),
            iid: "1".into(),
        }
    }

    fn an_mr() -> FocusedItem {
        FocusedItem::Mr {
            project: "g/p".into(),
            iid: "1".into(),
        }
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    /// Every chain the app can be in.
    fn all_chains() -> Vec<(View, Vec<&'static BindingGroup>)> {
        vec![
            (View::Dashboard, chain(View::Dashboard, Some(&an_issue()))),
            (View::IssueList, chain(View::IssueList, Some(&an_issue()))),
            (
                View::IssueDetail,
                chain(View::IssueDetail, Some(&an_issue())),
            ),
            (View::MrList, chain(View::MrList, Some(&an_mr()))),
            (View::MrDetail, chain(View::MrDetail, Some(&an_mr()))),
            (View::Planning, chain(View::Planning, Some(&an_issue()))),
        ]
    }

    /// The guard on the whole scheme: help offers exactly what fires.  Fails if
    /// a group is ever reordered so that one shadows a key another advertises.
    #[test]
    fn help_offers_exactly_what_resolve_fires() {
        for (view, groups) in all_chains() {
            for (group, bindings) in active_bindings(&groups) {
                for b in bindings {
                    assert_eq!(
                        keybindings::resolve(&groups, &b.matcher.to_event()),
                        Some(b.action),
                        "{view:?} / {}: {:?} is offered but masked",
                        group.title,
                        b.matcher,
                    );
                }
            }
        }
    }

    /// A detail view is innermost, so its `c` shadows the focused item's "add
    /// comment" with "reply to this thread" and no special case.  `x` and `r`
    /// are deliberately left alone, so closing the item and refreshing still
    /// work from a detail; and the view composes neither a list nor filtering,
    /// so those keys resolve to nothing at all.
    #[test]
    fn a_detail_chain_shadows_comment_but_leaves_close_and_refresh_alone() {
        let list = chain(View::IssueList, Some(&an_issue()));
        assert_eq!(
            keybindings::resolve(&list, &key('c')),
            Some(KeyAction::Comment)
        );

        for view in [View::IssueDetail, View::MrDetail] {
            let groups = chain(view, Some(&an_issue()));
            assert_eq!(
                keybindings::resolve(&groups, &key('c')),
                Some(KeyAction::ReplyThread),
                "{view:?}"
            );
            assert_eq!(
                keybindings::resolve(&groups, &key('C')),
                Some(KeyAction::NewThread),
                "{view:?}"
            );
            assert_eq!(
                keybindings::resolve(&groups, &code(KeyCode::Tab)),
                Some(KeyAction::ToggleThread),
                "{view:?}"
            );
            assert_eq!(
                keybindings::resolve(&groups, &key(' ')),
                Some(KeyAction::ResolveThread),
                "{view:?}"
            );
            assert_eq!(
                keybindings::resolve(&groups, &key('x')),
                Some(KeyAction::ToggleState),
                "{view:?}"
            );
            assert_eq!(
                keybindings::resolve(&groups, &key('r')),
                Some(KeyAction::Refresh),
                "{view:?}"
            );
            assert_eq!(
                keybindings::resolve(&groups, &key('R')),
                Some(KeyAction::FullRefresh)
            );
            for c in ['f', 'F', 'S'] {
                assert_eq!(
                    keybindings::resolve(&groups, &key(c)),
                    None,
                    "{view:?} / {c}"
                );
            }
            assert_eq!(
                keybindings::resolve(&groups, &code(KeyCode::Enter)),
                None,
                "{view:?} / Enter"
            );
        }
    }
}
