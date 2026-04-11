use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use crate::config::Config;
use crate::gitlab::types::{Iteration, TrackedIssue, TrackedMergeRequest, WorkItemStatus};
use crate::sort;
use crate::ui::styles;
use crate::ui::views::list_model::{ItemList, NavResult, UserFilter};

use std::collections::HashMap;

// ── Iteration Board ──

pub struct StatusColumn {
    pub list: ItemList<TrackedIssue>,
    pub status_name: String,
    #[allow(dead_code)] // will be used for column styling/grouping
    pub status_category: Option<String>,
}

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
    /// Build columns from available work item statuses.
    /// Adds a "No Status" column at the front, then one column per status ordered by position.
    pub fn build_columns(&mut self, statuses: &[WorkItemStatus]) {
        let mut cols = vec![StatusColumn {
            list: ItemList::default(),
            status_name: "No Status".to_string(),
            status_category: None,
        }];

        let mut sorted_statuses: Vec<&WorkItemStatus> = statuses.iter().collect();
        sorted_statuses.sort_by_key(|s| s.position.unwrap_or(i32::MAX));

        for status in sorted_statuses {
            cols.push(StatusColumn {
                list: ItemList::default(),
                status_name: status.name.clone(),
                status_category: status.category.clone(),
            });
        }

        self.columns = cols;
        self.focused_column = 0;
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

            // Match to status column
            let status_name = item.issue.custom_status.as_deref().unwrap_or("");
            let col_idx = self
                .columns
                .iter()
                .position(|c| c.status_name.eq_ignore_ascii_case(status_name))
                .unwrap_or(0); // fallback to "No Status"

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

    if inner.height < 2 || board.columns.is_empty() {
        return;
    }

    // Equal-width columns
    let constraints: Vec<Constraint> = board
        .columns
        .iter()
        .map(|_| Constraint::Ratio(1, u32::try_from(board.columns.len()).unwrap_or(1)))
        .collect();
    let col_rects = Layout::horizontal(constraints).split(inner);

    for (i, col_rect) in col_rects.iter().enumerate() {
        let is_focused = i == board.focused_column;
        render_board_column(frame, *col_rect, board, i, issues, is_focused);
    }
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
    let header = format!("{} ({})", col.status_name, count);

    let header_style = if is_focused {
        Style::default()
            .fg(styles::TEXT_BRIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::TEXT_DIM)
    };

    // Column header (1 line)
    let header_line = Line::from(Span::styled(header, header_style));
    let header_widget = Paragraph::new(header_line);

    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    frame.render_widget(header_widget, parts[0]);

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

    frame.render_stateful_widget(
        table,
        parts[1],
        &mut board.columns[col_idx].list.table_state,
    );
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
