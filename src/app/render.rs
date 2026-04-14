//! Render the TUI: tab bar, main view, status bar, and overlays.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;

use crate::keybindings;
use crate::ui::components::{chord_popup, confirm_dialog, error_popup, help, label_editor, picker};
use crate::ui::views::{
    dashboard, filter_editor, issue_detail, issue_list, mr_detail, mr_list, planning,
};

use super::{App, ConfirmAction, Overlay, View};

impl App {
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(1), // Tab bar
            Constraint::Min(1),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(area);

        // Tab bar
        crate::ui::components::tab_bar::render(frame, chunks[0], self.view);

        let ctx = crate::ui::RenderCtx {
            label_colors: &self.label_color_map,
        };

        // Render main view
        match self.view {
            View::Dashboard => {
                let current_iter = self.views.planning.current_iteration.as_ref();
                dashboard::render(
                    frame,
                    chunks[1],
                    &self.config,
                    self.active_team,
                    &self.issues,
                    &self.mrs,
                    self.loading,
                    &mut self.views.board,
                    current_iter,
                    self.views.health.as_mut(),
                    &self.shadow_work_cache,
                    &self.unplanned_work_cache,
                );
            }
            View::IssueList => {
                issue_list::render(
                    frame,
                    chunks[1],
                    &mut self.views.issue_list,
                    &self.issues,
                    &ctx,
                );
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue().cloned() {
                    issue_detail::render(frame, chunks[1], &item, &self.views.issue_detail, &ctx);
                }
            }
            View::MrList => {
                mr_list::render(frame, chunks[1], &mut self.views.mr_list, &self.mrs, &ctx);
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr().cloned() {
                    mr_detail::render(frame, chunks[1], &item, &self.views.mr_detail, &ctx);
                }
            }
            View::Planning => {
                planning::render(
                    frame,
                    chunks[1],
                    &mut self.views.planning,
                    &self.issues,
                    &self.config,
                    self.active_team,
                    &ctx,
                );
            }
        }

        // Status bar
        let team_name = self
            .active_team
            .and_then(|idx| self.config.teams.get(idx))
            .map_or("all", |t| t.name.as_str());
        let view_name = match self.view {
            View::Dashboard => "Dashboard",
            View::IssueList => "Issues",
            View::IssueDetail => "Issue Detail",
            View::MrList => "Merge Requests",
            View::MrDetail => "MR Detail",
            View::Planning => "Planning",
        };
        let item_count = match self.view {
            View::IssueList => self.views.issue_list.list.len(),
            View::MrList => self.views.mr_list.list.len(),
            View::Planning => self
                .views.planning
                .columns
                .iter()
                .map(|c| c.list.len())
                .sum(),
            _ => self.issues.len() + self.mrs.len(),
        };
        // Skip Global and Navigation groups — tabs handle those
        let binding_hints: Vec<(&str, &str)> = keybindings::binding_groups_for_view(self.view)
            .iter()
            .filter(|g| g.title != "Global" && g.title != "Navigation")
            .flat_map(|g| g.bindings.iter())
            .filter(|b| b.visible_in_help())
            .take(8)
            .map(|b| (b.label, b.description))
            .collect();
        let hints = binding_hints.as_slice();
        crate::ui::components::status_bar::render(
            frame,
            chunks[2],
            &crate::ui::components::status_bar::StatusBarProps {
                view_name,
                team_name,
                item_count,
                loading: self.loading,
                loading_msg: self.loading_msg,
                error: self.error.as_deref(),
                last_fetched_at: self.last_fetched_at,
                last_fetch_ms: self.last_fetch_ms,
                hints,
            },
        );

        // Render overlay on top
        match &self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                help::render(frame, area, self.view);
            }
            Overlay::FilterEditor => {
                filter_editor::render(frame, area, &mut self.filter_editor_state, &ctx);
            }
            Overlay::Confirm(action) => {
                let (title, msg) = match action {
                    ConfirmAction::CloseIssue(_, iid) => {
                        ("Close Issue", format!("Close issue #{iid}?"))
                    }
                    ConfirmAction::ReopenIssue(_, iid) => {
                        ("Reopen Issue", format!("Reopen issue #{iid}?"))
                    }
                    ConfirmAction::CloseMr(_, iid) => ("Close MR", format!("Close MR !{iid}?")),
                    ConfirmAction::ApproveMr(_, iid) => {
                        ("Approve MR", format!("Approve MR !{iid}?"))
                    }
                    ConfirmAction::MergeMr(_, iid) => ("Merge MR", format!("Merge MR !{iid}?")),
                    ConfirmAction::QuitApp => ("Quit", "Quit glab-dash?".to_string()),
                };
                confirm_dialog::render(frame, area, title, &msg);
            }
            Overlay::Picker(_) => {
                if let Some(ref mut ps) = self.picker_state {
                    picker::render(frame, area, ps, &ctx);
                }
            }
            Overlay::CommentInput => {
                let popup = centered_rect(60, 40, area);
                ratatui::widgets::Clear.render(popup, frame.buffer_mut());
                let title = if self.reply_discussion_id.is_some() {
                    "Reply (Enter submit, C-j newline)"
                } else {
                    "Comment (Enter submit, C-j newline)"
                };
                crate::ui::components::input::render(frame, popup, &mut self.comment_input, title);
                crate::ui::components::autocomplete::render(frame, popup, &self.autocomplete);
            }
            Overlay::Chord(_) => {
                if let Some(ref cs) = self.chord_state {
                    chord_popup::render(frame, area, cs);
                }
            }
            Overlay::LabelEditor => {
                if let Some(ref les) = self.label_editor_state {
                    label_editor::render(frame, area, les, &self.label_color_map);
                }
            }
            Overlay::Error(msg) => {
                error_popup::render(frame, area, msg);
            }
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
