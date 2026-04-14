//! Render the TUI: tab bar, main view, status bar, and overlays.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;

use crate::keybindings;
use crate::ui::components::{chord_popup, confirm_dialog, error_popup, help, label_editor, picker};
use crate::ui::views::{
    dashboard, filter_editor, issue_detail, issue_list, mr_detail, mr_list, planning,
};

use super::{App, Overlay, View};

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
        crate::ui::components::tab_bar::render(frame, chunks[0], self.ui.view);

        let ctx = crate::ui::RenderCtx {
            label_colors: &self.data.label_color_map,
        };

        // Render main view
        match self.ui.view {
            View::Dashboard => {
                let current_iter = self.ui.views.planning.current_iteration.as_ref();
                dashboard::render(
                    frame,
                    chunks[1],
                    &self.ctx.config,
                    self.ui.active_team,
                    &self.data.issues,
                    &self.data.mrs,
                    self.ui.loading,
                    &mut self.ui.views.board,
                    current_iter,
                    self.ui.views.health.as_mut(),
                    &self.data.shadow_work_cache,
                    &self.data.unplanned_work_cache,
                );
            }
            View::IssueList => {
                issue_list::render(
                    frame,
                    chunks[1],
                    &mut self.ui.views.issue_list,
                    &self.data.issues,
                    &ctx,
                );
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue().cloned() {
                    issue_detail::render(
                        frame,
                        chunks[1],
                        &item,
                        &self.ui.views.issue_detail,
                        &ctx,
                    );
                }
            }
            View::MrList => {
                mr_list::render(
                    frame,
                    chunks[1],
                    &mut self.ui.views.mr_list,
                    &self.data.mrs,
                    &ctx,
                );
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr().cloned() {
                    mr_detail::render(frame, chunks[1], &item, &self.ui.views.mr_detail, &ctx);
                }
            }
            View::Planning => {
                planning::render(
                    frame,
                    chunks[1],
                    &mut self.ui.views.planning,
                    &self.data.issues,
                    &self.ctx.config,
                    self.ui.active_team,
                    &ctx,
                );
            }
        }

        // Status bar
        let team_name = self
            .ui
            .active_team
            .and_then(|idx| self.ctx.config.teams.get(idx))
            .map_or("all", |t| t.name.as_str());
        let view_name = match self.ui.view {
            View::Dashboard => "Dashboard",
            View::IssueList => "Issues",
            View::IssueDetail => "Issue Detail",
            View::MrList => "Merge Requests",
            View::MrDetail => "MR Detail",
            View::Planning => "Planning",
        };
        let item_count = match self.ui.view {
            View::IssueList => self.ui.views.issue_list.list.len(),
            View::MrList => self.ui.views.mr_list.list.len(),
            View::Planning => self
                .ui
                .views
                .planning
                .columns
                .iter()
                .map(|c| c.list.len())
                .sum(),
            _ => self.data.issues.len() + self.data.mrs.len(),
        };
        // Skip Global and Navigation groups — tabs handle those
        let binding_hints: Vec<(&str, &str)> = keybindings::binding_groups_for_view(self.ui.view)
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
                loading: self.ui.loading,
                loading_msg: self.ui.loading_msg,
                error: self.ui.error.as_deref(),
                last_fetched_at: self.ui.last_fetched_at,
                last_fetch_ms: self.ui.last_fetch_ms,
                hints,
            },
        );

        // Render overlay on top
        match &self.ui.overlay {
            Overlay::None => {}
            Overlay::Help => {
                help::render(frame, area, self.ui.view);
            }
            Overlay::FilterEditor => {
                filter_editor::render(frame, area, &mut self.ui.filter_editor_state, &ctx);
            }
            Overlay::Confirm => {
                confirm_dialog::render(
                    frame,
                    area,
                    &self.ui.confirm_title,
                    &self.ui.confirm_message,
                );
            }
            Overlay::Picker => {
                if let Some(ref mut ps) = self.ui.picker_state {
                    picker::render(frame, area, ps, &ctx);
                }
            }
            Overlay::CommentInput => {
                let popup = centered_rect(60, 40, area);
                ratatui::widgets::Clear.render(popup, frame.buffer_mut());
                let title = if self.ui.reply_discussion_id.is_some() {
                    "Reply (Enter submit, C-j newline)"
                } else {
                    "Comment (Enter submit, C-j newline)"
                };
                crate::ui::components::input::render(
                    frame,
                    popup,
                    &mut self.ui.comment_input,
                    title,
                );
                crate::ui::components::autocomplete::render(frame, popup, &self.ui.autocomplete);
            }
            Overlay::Chord => {
                if let Some(ref cs) = self.ui.chord_state {
                    chord_popup::render(frame, area, cs);
                }
            }
            Overlay::LabelEditor => {
                if let Some(ref les) = self.ui.label_editor_state {
                    label_editor::render(frame, area, les, &self.data.label_color_map);
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
