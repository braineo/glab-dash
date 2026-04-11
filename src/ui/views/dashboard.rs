use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use crate::config::{Config, KanbanColumnConfig};
use crate::gitlab::types::{Iteration, TrackedIssue, TrackedMergeRequest, WorkItemStatus};
use crate::sort;
use crate::ui::styles;
use crate::ui::views::list_model::{ItemList, NavResult, UserFilter};

use std::collections::HashMap;

// ── Iteration Board ──

pub struct StatusColumn {
    pub list: ItemList<TrackedIssue>,
    pub status_name: String,
    /// Status names that map to this column (lowercase for matching).
    pub status_matches: Vec<String>,
}

/// Max visible columns in the sliding window.
const DEFAULT_VISIBLE_COLUMNS: usize = 3;

#[derive(Default)]
pub struct IterationBoardState {
    pub columns: Vec<StatusColumn>,
    pub focused_column: usize,
    pub filter: UserFilter,
}

#[derive(Debug, PartialEq)]
pub enum DashboardAction {
    None,
    Refilter,
    OpenDetail,
    Refresh,
    SetStatus,
    ToggleState,
    EditLabels,
    EditAssignee,
    Comment,
    OpenBrowser,
}

impl IterationBoardState {
    /// Build columns from config `kanban_columns` if present, otherwise one column per status.
    pub fn build_columns(
        &mut self,
        statuses: &[WorkItemStatus],
        kanban_config: &[KanbanColumnConfig],
    ) {
        let cols = if kanban_config.is_empty() {
            Self::build_columns_auto(statuses)
        } else {
            Self::build_columns_from_config(kanban_config)
        };
        self.columns = cols;
        self.focused_column = 0;
    }

    fn build_columns_auto(statuses: &[WorkItemStatus]) -> Vec<StatusColumn> {
        let mut cols = vec![StatusColumn {
            list: ItemList::default(),
            status_name: "No Status".to_string(),
            status_matches: vec![String::new()],
        }];

        let mut sorted_statuses: Vec<&WorkItemStatus> = statuses.iter().collect();
        sorted_statuses.sort_by_key(|s| s.position.unwrap_or(i32::MAX));

        for status in sorted_statuses {
            cols.push(StatusColumn {
                list: ItemList::default(),
                status_name: status.name.clone(),
                status_matches: vec![status.name.to_lowercase()],
            });
        }
        cols
    }

    fn build_columns_from_config(kanban_config: &[KanbanColumnConfig]) -> Vec<StatusColumn> {
        kanban_config
            .iter()
            .map(|kc| StatusColumn {
                list: ItemList::default(),
                status_name: kc.name.clone(),
                status_matches: kc.statuses.iter().map(|s| s.to_lowercase()).collect(),
            })
            .collect()
    }

    /// Compute the window start so `focused_column` is visible in the window.
    fn window_start(&self, visible: usize) -> usize {
        let total = self.columns.len();
        if total <= visible {
            return 0;
        }
        // Keep focused column within the visible window
        // Try to center the focused column when possible
        let half = visible / 2;
        let start = self.focused_column.saturating_sub(half);
        start.min(total - visible)
    }

    /// Partition current-iteration issues into status columns, apply shared fuzzy/sort.
    pub fn partition_issues(
        &mut self,
        issues: &[TrackedIssue],
        current_iteration: Option<&Iteration>,
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        for col in &mut self.columns {
            col.list.indices.clear();
        }

        let current_id = current_iteration.map(|i| i.id.as_str());

        for (i, item) in issues.iter().enumerate() {
            // Prefilter: only current iteration
            let iter_id = item.issue.iteration.as_ref().map(|it| it.id.as_str());
            if iter_id != current_id || current_id.is_none() {
                continue;
            }

            // Match to status column by checking status_matches
            let status_lower = item
                .issue
                .custom_status
                .as_deref()
                .unwrap_or("")
                .to_lowercase();
            let col_idx = self
                .columns
                .iter()
                .position(|c| c.status_matches.iter().any(|m| m == &status_lower))
                .unwrap_or(0); // fallback to first column

            self.columns[col_idx].list.indices.push(i);
        }

        // Apply shared fuzzy filter and sort to each column
        for col in &mut self.columns {
            col.list.indices.retain(|&i| {
                let item = &issues[i];
                let mut haystack = item.issue.title.to_lowercase();
                for a in &item.issue.assignees {
                    haystack.push(' ');
                    haystack.push_str(&a.username.to_lowercase());
                }
                for l in &item.issue.labels {
                    haystack.push(' ');
                    haystack.push_str(&l.to_lowercase());
                }
                self.filter.fuzzy_matches(&haystack)
            });
            sort::sort_issues(
                &mut col.list.indices,
                issues,
                &self.filter.sort_specs,
                label_orders,
            );
            col.list.clamp_selection();
        }
    }

    pub fn selected_issue<'a>(&self, issues: &'a [TrackedIssue]) -> Option<&'a TrackedIssue> {
        self.columns
            .get(self.focused_column)
            .and_then(|col| col.list.selected_item(issues))
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DashboardAction {
        // Fuzzy search input
        if let Some(needs_refilter) = self.filter.handle_fuzzy_input(key) {
            return if needs_refilter {
                DashboardAction::Refilter
            } else {
                DashboardAction::None
            };
        }

        // Column navigation
        match key.code {
            KeyCode::Char('[') | KeyCode::Left if !self.columns.is_empty() => {
                self.focused_column = self.focused_column.saturating_sub(1);
                return DashboardAction::None;
            }
            KeyCode::Char(']') | KeyCode::Right if !self.columns.is_empty() => {
                if self.focused_column + 1 < self.columns.len() {
                    self.focused_column += 1;
                }
                return DashboardAction::None;
            }
            _ => {}
        }

        if self.columns.is_empty() {
            return DashboardAction::None;
        }

        let col = self.focused_column;

        if self.columns[col].list.is_empty() {
            return match key.code {
                KeyCode::Char('/') => {
                    self.filter.start_search();
                    DashboardAction::None
                }
                KeyCode::Char('r') => DashboardAction::Refresh,
                _ => DashboardAction::None,
            };
        }

        // Navigation within focused column
        match self.columns[col].list.handle_nav(key) {
            NavResult::Handled => return DashboardAction::None,
            NavResult::OpenDetail => return DashboardAction::OpenDetail,
            NavResult::None => {}
        }

        match key.code {
            KeyCode::Char('/') => {
                self.filter.start_search();
            }
            KeyCode::Char('r') => return DashboardAction::Refresh,
            KeyCode::Char('s') => return DashboardAction::SetStatus,
            KeyCode::Char('x') => return DashboardAction::ToggleState,
            KeyCode::Char('l') => return DashboardAction::EditLabels,
            KeyCode::Char('a') => return DashboardAction::EditAssignee,
            KeyCode::Char('c') => return DashboardAction::Comment,
            KeyCode::Char('o') => return DashboardAction::OpenBrowser,
            _ => {}
        }
        DashboardAction::None
    }
}

// ── Rendering ──

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    active_team: Option<usize>,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
    loading: bool,
    board: &mut IterationBoardState,
    current_iteration: Option<&Iteration>,
) {
    let chunks = Layout::vertical([
        Constraint::Length(3),      // Header
        Constraint::Percentage(35), // Summary
        Constraint::Min(5),         // Iteration board
    ])
    .split(area);

    // Header
    let team_name = active_team
        .and_then(|idx| config.teams.get(idx))
        .map_or("all", |t| t.name.as_str());
    let tracking_display = config.tracking_projects.join(", ");
    let header_text = Line::from(vec![
        Span::styled(" \u{25c8} glab-dash", styles::title_style()),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
        Span::styled(
            format!("Team: {team_name}"),
            Style::default().fg(styles::TEAL),
        ),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
        Span::styled(
            format!("Tracking: {tracking_display}"),
            styles::help_desc_style(),
        ),
    ]);
    let header = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::BORDER)),
    );
    frame.render_widget(header, chunks[0]);

    // Summary (top half)
    let content_chunks =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);
    render_member_summary(frame, content_chunks[0], config, active_team, issues, mrs);
    render_quick_stats(frame, content_chunks[1], config, issues, mrs, loading);

    // Iteration board (bottom half)
    render_iteration_board(frame, chunks[2], board, issues, current_iteration);
}

fn render_iteration_board(
    frame: &mut Frame,
    area: Rect,
    board: &mut IterationBoardState,
    issues: &[TrackedIssue],
    current_iteration: Option<&Iteration>,
) {
    if board.columns.is_empty() {
        let msg = Paragraph::new(Line::from(vec![
            Span::styled(" Iteration Board", styles::section_header_style()),
            Span::styled(" — no statuses loaded yet", styles::help_desc_style()),
        ]));
        frame.render_widget(msg, area);
        return;
    }

    let iter_label = current_iteration.map_or_else(
        || "No current iteration".to_string(),
        |i| {
            if i.title.is_empty() {
                match (&i.start_date, &i.due_date) {
                    (Some(s), Some(d)) => format!("{s} \u{2014} {d}"),
                    _ => "Current".to_string(),
                }
            } else {
                i.title.clone()
            }
        },
    );

    // Search indicator in title
    let title_line = if board.filter.is_searching() {
        Line::from(vec![
            Span::styled(
                format!(" \u{25cf} {iter_label} /"),
                Style::default()
                    .fg(styles::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                board.filter.fuzzy_query.as_str(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("\u{258e}", Style::default().fg(styles::CYAN)),
        ])
    } else if board.filter.has_query() {
        Line::from(vec![
            Span::styled(
                format!(" \u{25cf} {iter_label} /"),
                Style::default()
                    .fg(styles::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                board.filter.fuzzy_query.as_str(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(Span::styled(
            format!(" \u{25cf} {iter_label}"),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))
    };

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if board.filter.is_searching() {
            Style::default().fg(styles::CYAN)
        } else {
            Style::default().fg(styles::BORDER)
        })
        .title(title_line);

    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.height < 3 || board.columns.is_empty() {
        return;
    }

    // Reserve 1 line at bottom for column indicator
    let board_parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let board_area = board_parts[0];
    let indicator_area = board_parts[1];

    // Sliding window: show up to DEFAULT_VISIBLE_COLUMNS columns
    let visible = DEFAULT_VISIBLE_COLUMNS.min(board.columns.len());
    let win_start = board.window_start(visible);
    let win_end = (win_start + visible).min(board.columns.len());

    let constraints: Vec<Constraint> = (win_start..win_end)
        .map(|_| Constraint::Ratio(1, u32::try_from(win_end - win_start).unwrap_or(1)))
        .collect();
    let col_rects = Layout::horizontal(constraints).split(board_area);

    for (slot, col_rect) in col_rects.iter().enumerate() {
        let col_idx = win_start + slot;
        let is_focused = col_idx == board.focused_column;
        render_board_column(frame, *col_rect, board, col_idx, issues, is_focused);
    }

    // Bottom indicator: show all columns with counts
    render_column_indicator(frame, indicator_area, board, win_start, win_end);
}

fn render_board_column(
    frame: &mut Frame,
    area: Rect,
    board: &mut IterationBoardState,
    col_idx: usize,
    issues: &[TrackedIssue],
    is_focused: bool,
) {
    let col = &board.columns[col_idx];
    let count = col.list.len();
    let header_text = format!("{} ({})", col.status_name, count);

    let border_style = if is_focused {
        Style::default()
            .fg(styles::BLUE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::TEXT_DIM)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if is_focused {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(border_style)
        .title(Span::styled(
            header_text,
            if is_focused {
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::TEXT)
            },
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 {
        return;
    }

    // Issue rows
    let rows: Vec<Row> = col
        .list
        .indices
        .iter()
        .filter_map(|&i| issues.get(i))
        .map(|item| {
            let iid = format!("#{}", item.issue.iid);
            let assignee = item
                .issue
                .assignees
                .first()
                .map_or("-", |u| u.username.as_str());

            Row::new(vec![
                Cell::from(Span::styled(iid, Style::default().fg(styles::TEXT_DIM))),
                Cell::from(Span::styled(
                    item.issue.title.as_str(),
                    Style::default().fg(styles::TEXT),
                )),
                Cell::from(Span::styled(assignee, Style::default().fg(styles::CYAN))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(6),  // iid
        Constraint::Min(8),     // title
        Constraint::Length(10), // assignee
    ];

    let table = Table::new(rows, widths).row_highlight_style(styles::selected_style());

    frame.render_stateful_widget(table, inner, &mut board.columns[col_idx].list.table_state);
}

fn render_column_indicator(
    frame: &mut Frame,
    area: Rect,
    board: &IterationBoardState,
    win_start: usize,
    win_end: usize,
) {
    let mut spans: Vec<Span> = Vec::new();

    // Left arrow if scrolled
    if win_start > 0 {
        spans.push(Span::styled(
            "\u{25c0} ",
            Style::default().fg(styles::TEXT_DIM),
        ));
    } else {
        spans.push(Span::raw("  "));
    }

    for (i, col) in board.columns.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " \u{2502} ",
                Style::default().fg(styles::BORDER),
            ));
        }

        let label = format!("{} {}", col.status_name, col.list.len());
        let in_window = i >= win_start && i < win_end;
        let is_focused = i == board.focused_column;

        let style = if is_focused {
            Style::default()
                .fg(styles::CYAN)
                .add_modifier(Modifier::BOLD)
        } else if in_window {
            Style::default().fg(styles::TEXT_BRIGHT)
        } else {
            Style::default().fg(styles::TEXT_DIM)
        };

        spans.push(Span::styled(label, style));
    }

    // Right arrow if more columns
    if win_end < board.columns.len() {
        spans.push(Span::styled(
            " \u{25b6}",
            Style::default().fg(styles::TEXT_DIM),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ── Existing summary renderers ──

fn render_member_summary(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    active_team: Option<usize>,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
) {
    let members = match active_team {
        Some(idx) => config.team_members(idx),
        None => config.all_members(),
    };
    let rows: Vec<Row> = members
        .iter()
        .enumerate()
        .map(|(i, member)| {
            let issue_count = issues
                .iter()
                .filter(|issue| {
                    issue
                        .issue
                        .assignees
                        .iter()
                        .any(|a| a.username.eq_ignore_ascii_case(member))
                })
                .count();
            let mr_count = mrs
                .iter()
                .filter(|m| {
                    m.mr.assignees
                        .iter()
                        .any(|a| a.username.eq_ignore_ascii_case(member))
                        || m.mr
                            .reviewers
                            .iter()
                            .any(|r| r.username.eq_ignore_ascii_case(member))
                })
                .count();
            let row = Row::new(vec![
                member.clone(),
                issue_count.to_string(),
                mr_count.to_string(),
            ]);
            if i % 2 == 1 {
                row.style(styles::row_alt_style())
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec!["Member", "Issues", "MRs"])
                .style(styles::header_style())
                .bottom_margin(1),
        )
        .block(styles::block("Team Members"));

    frame.render_widget(table, area);
}

fn render_quick_stats(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
    loading: bool,
) {
    let tracking_issues = issues
        .iter()
        .filter(|i| config.is_tracking_project(&i.project_path))
        .count();
    let external_issues = issues
        .iter()
        .filter(|i| !config.is_tracking_project(&i.project_path))
        .count();
    let unassigned_issues = issues
        .iter()
        .filter(|i| i.issue.assignees.is_empty())
        .count();
    let open_mrs = mrs.iter().filter(|m| m.mr.state == "opened").count();
    let draft_mrs = mrs
        .iter()
        .filter(|m| m.mr.draft || m.mr.work_in_progress)
        .count();
    let my_review_mrs = mrs
        .iter()
        .filter(|m| {
            m.mr.reviewers
                .iter()
                .any(|r| r.username.eq_ignore_ascii_case(&config.me))
        })
        .count();

    let loading_indicator = if loading { " \u{27f3}" } else { "" };

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  \u{25cf} Issues{loading_indicator}"),
            styles::section_header_style(),
        )]),
        Line::from(vec![
            Span::styled("    Tracking repo:   ", styles::help_desc_style()),
            Span::styled(
                tracking_issues.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("    External:        ", styles::help_desc_style()),
            Span::styled(
                external_issues.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(if unassigned_issues > 0 {
            vec![
                Span::styled("    Unassigned:      ", styles::help_desc_style()),
                Span::styled(unassigned_issues.to_string(), styles::error_style()),
            ]
        } else {
            vec![
                Span::styled("    Unassigned:      ", styles::help_desc_style()),
                Span::styled("0", Style::default().fg(styles::TEXT_BRIGHT)),
            ]
        }),
        Line::from(""),
        Line::from(Span::styled(
            "  \u{2482} Merge Requests",
            styles::section_header_style(),
        )),
        Line::from(vec![
            Span::styled("    Open:            ", styles::help_desc_style()),
            Span::styled(
                open_mrs.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("    Draft:           ", styles::help_desc_style()),
            Span::styled(
                draft_mrs.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("    Needs my review: ", styles::help_desc_style()),
            Span::styled(
                my_review_mrs.to_string(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(styles::block("Overview"));
    frame.render_widget(paragraph, area);
}
