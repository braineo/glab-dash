use chrono::{DateTime, NaiveDate, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};

use crossterm::event::{KeyCode, KeyEvent};

use crate::cmd::{Cmd, Dirty, EventResult};
use crate::config::{Config, KanbanColumnConfig};
use crate::gitlab::types::{Iteration, TrackedIssue, TrackedMergeRequest, WorkItemStatus};
use crate::keybindings::{self, KeyAction};
use crate::sort;
use crate::ui::styles;
use crate::ui::views::list_model::{FilterBarAction, ItemList, UserFilter};

use std::collections::HashMap;

// ── Iteration Health ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthTab {
    #[default]
    UnplannedWork,
    ShadowWork,
    AtRisk,
}

impl HealthTab {
    pub fn next(self) -> Self {
        match self {
            Self::UnplannedWork => Self::ShadowWork,
            Self::ShadowWork => Self::AtRisk,
            Self::AtRisk => Self::UnplannedWork,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::UnplannedWork => Self::AtRisk,
            Self::ShadowWork => Self::UnplannedWork,
            Self::AtRisk => Self::ShadowWork,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BurnRate {
    OnTrack,
    Behind,
    Ahead,
    #[default]
    Unknown,
}

#[derive(Default)]
pub struct IterationHealth {
    // Progress
    pub total_issues: usize,
    pub done_issues: usize,
    pub days_elapsed: i64,
    pub days_remaining: i64,
    pub days_total: i64,
    pub burn_rate: BurnRate,
    // Focusable lists (indices into App::issues for unplanned_work/at_risk,
    // into App::shadow_work_cache for shadow_work)
    pub unplanned_work: ItemList<TrackedIssue>,
    pub shadow_work: ItemList<TrackedIssue>,
    pub at_risk: ItemList<TrackedIssue>,
    // Loading states (derived from fetch state, not stored separately)
    pub unplanned_work_loading: bool,
    // Tab navigation
    pub active_tab: HealthTab,
}

impl IterationHealth {
    /// Health panel handles list nav in the active tab.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        dirty: &mut Dirty,
        needs_redraw: &mut bool,
    ) -> EventResult {
        // Active tab's list handles nav
        if let Some(moved) = self.active_list_mut().handle_nav_key(key) {
            if moved {
                dirty.selection = true;
            } else {
                *needs_redraw = false;
            }
            return EventResult::Consumed;
        }
        EventResult::Bubble
    }

    pub fn active_list(&self) -> &ItemList<TrackedIssue> {
        match self.active_tab {
            HealthTab::UnplannedWork => &self.unplanned_work,
            HealthTab::ShadowWork => &self.shadow_work,
            HealthTab::AtRisk => &self.at_risk,
        }
    }

    pub fn active_list_mut(&mut self) -> &mut ItemList<TrackedIssue> {
        match self.active_tab {
            HealthTab::UnplannedWork => &mut self.unplanned_work,
            HealthTab::ShadowWork => &mut self.shadow_work,
            HealthTab::AtRisk => &mut self.at_risk,
        }
    }

    pub fn active_tab_loading(&self) -> bool {
        match self.active_tab {
            HealthTab::UnplannedWork => self.unplanned_work_loading,
            HealthTab::ShadowWork | HealthTab::AtRisk => false,
        }
    }

    /// Resolve the selected issue from the active health tab.
    /// `issues` is the main issue list (for unplanned_work/at_risk),
    /// `shadow_work_cache` is the separate shadow work source.
    pub fn selected_issue<'a>(
        &self,
        issues: &'a [TrackedIssue],
        shadow_work_cache: &'a [TrackedIssue],
    ) -> Option<&'a TrackedIssue> {
        match self.active_tab {
            HealthTab::UnplannedWork => self.unplanned_work.selected_item(issues),
            HealthTab::ShadowWork => self.shadow_work.selected_item(shadow_work_cache),
            HealthTab::AtRisk => self.at_risk.selected_item(issues),
        }
    }
}

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
    /// When true, `[`/`]` and `j`/`k` navigate the health panel instead of the board.
    pub health_focused: bool,
}

impl IterationBoardState {
    // ── Key handling ────────────────────────────────────────────────

    /// Dashboard key handler. Delegates to focused child (health or column),
    /// then handles board-level keys (column nav, tab toggle, filter, search).
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        mut health: Option<&mut IterationHealth>,
        dirty: &mut Dirty,
        cmds: &mut Vec<Cmd>,
        needs_redraw: &mut bool,
    ) -> EventResult {
        // Filter bar
        if self.filter.bar_focused {
            match self.filter.handle_bar_key(key) {
                FilterBarAction::Deleted => {
                    dirty.view_state = true;
                    cmds.push(Cmd::PersistViewState);
                }
                FilterBarAction::Unfocused | FilterBarAction::Consumed => {}
            }
            return EventResult::Consumed;
        }

        // Fuzzy search
        if self.filter.is_searching() {
            if self.filter.handle_fuzzy_input(key) == Some(true) {
                dirty.view_state = true;
            }
            dirty.selection = true;
            return EventResult::Consumed;
        }

        // 1. Focused child: health panel or board column
        let child_result = if self.health_focused {
            health.as_mut().map_or(EventResult::Bubble, |h| {
                h.handle_key(key, dirty, needs_redraw)
            })
        } else {
            self.columns
                .get_mut(self.focused_column)
                .and_then(|col| {
                    let moved = col.list.handle_nav_key(key)?;
                    if moved {
                        dirty.selection = true;
                    } else {
                        *needs_redraw = false;
                    }
                    Some(EventResult::Consumed)
                })
                .unwrap_or(EventResult::Bubble)
        };
        if child_result.handled() {
            return child_result;
        }

        // 2. Board-level: column/tab nav
        if let Some(action) = keybindings::match_group(keybindings::BOARD_NAV_BINDINGS, key) {
            match action {
                KeyAction::ToggleDashboardFocus => {
                    self.health_focused = !self.health_focused;
                    dirty.selection = true;
                }
                KeyAction::ColumnLeft => {
                    if self.health_focused {
                        if let Some(h) = health.as_mut() {
                            h.active_tab = h.active_tab.prev();
                            h.active_list_mut().table_state.select(Some(0));
                        }
                    } else if !self.columns.is_empty() {
                        self.focused_column = self.focused_column.saturating_sub(1);
                    }
                    dirty.selection = true;
                }
                KeyAction::ColumnRight => {
                    if self.health_focused {
                        if let Some(h) = health.as_mut() {
                            h.active_tab = h.active_tab.next();
                            h.active_list_mut().table_state.select(Some(0));
                        }
                    } else if !self.columns.is_empty()
                        && self.focused_column + 1 < self.columns.len()
                    {
                        self.focused_column += 1;
                    }
                    dirty.selection = true;
                }
                _ => return EventResult::Bubble,
            }
            return EventResult::Consumed;
        }

        // 3. Start search
        if key.code == KeyCode::Char('/') {
            self.filter.start_search();
            dirty.selection = true;
            return EventResult::Consumed;
        }

        EventResult::Bubble
    }

    // ── Column management ───────────────────────────────────────────

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
        if self.focused_column >= self.columns.len() {
            self.focused_column = self.columns.len().saturating_sub(1);
        }
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
        me: &str,
        team_members: &[String],
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
            if self.columns.is_empty() {
                continue;
            }
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

        // Apply shared filter conditions, fuzzy filter, and sort to each column
        for col in &mut self.columns {
            col.list.indices.retain(|&i| {
                let item = &issues[i];
                if !crate::filter::condition::matches_issue(
                    item,
                    &self.filter.conditions,
                    me,
                    team_members,
                ) {
                    return false;
                }
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
    board_issues: &[TrackedIssue],
    current_iteration: Option<&Iteration>,
    health: Option<&mut IterationHealth>,
    shadow_work_cache: &[TrackedIssue],
    unplanned_work_cache: &HashMap<u64, DateTime<Utc>>,
) {
    let chunks = Layout::vertical([
        Constraint::Length(3),      // Header
        Constraint::Percentage(35), // Summary + Health
        Constraint::Min(5),         // Iteration board
    ])
    .split(area);

    // Header
    let team_name = active_team
        .and_then(|idx| config.teams.get(idx))
        .map_or("all", |t| t.name.as_str());
    let tracking_display = config.tracking_projects.join(", ");
    let header_text = Line::from(vec![
        Span::styled(
            format!(" {} glab-dash", styles::ICON_DASHBOARD),
            styles::title_style(),
        ),
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
        if loading {
            Span::styled(
                format!(" {}", styles::ICON_LOADING),
                Style::default().fg(styles::YELLOW),
            )
        } else {
            Span::raw("")
        },
    ]);
    let header = Paragraph::new(header_text).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::BORDER)),
    );
    frame.render_widget(header, chunks[0]);

    // Summary (left) + Health panel (right)
    let content_chunks =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(chunks[1]);
    render_quick_stats(frame, content_chunks[0], config, active_team, issues, mrs);
    render_iteration_health(
        frame,
        content_chunks[1],
        health,
        current_iteration,
        board.health_focused,
        board_issues,
        shadow_work_cache,
        unplanned_work_cache,
    );

    // Iteration board (bottom half)
    render_iteration_board(
        frame,
        chunks[2],
        board,
        board_issues,
        current_iteration,
        !board.health_focused,
    );
}

fn render_iteration_board(
    frame: &mut Frame,
    area: Rect,
    board: &mut IterationBoardState,
    issues: &[TrackedIssue],
    current_iteration: Option<&Iteration>,
    board_focused: bool,
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
                format!(" {} {iter_label} /", styles::ICON_OPEN),
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
                format!(" {} {iter_label} /", styles::ICON_OPEN),
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
            format!(" {} {iter_label}", styles::ICON_OPEN),
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

    // Filter + sort bar
    let has_filter_bar = !board.filter.conditions.is_empty() || !board.filter.sort_specs.is_empty();
    let filter_bar_height = u16::from(has_filter_bar);

    // Reserve 1 line at bottom for column indicator, optional filter bar at top
    let board_parts = Layout::vertical([
        Constraint::Length(filter_bar_height),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    if has_filter_bar {
        crate::ui::components::filter_bar::render(
            frame,
            board_parts[0],
            &board.filter.conditions,
            &board.filter.sort_specs,
            board.filter.bar_focused,
            board.filter.bar_selected,
        );
    }
    let board_area = board_parts[1];
    let indicator_area = board_parts[2];

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
        let is_focused = board_focused && col_idx == board.focused_column;
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

// ── Iteration Health Rendering ──

#[allow(clippy::too_many_arguments)]
fn render_iteration_health(
    frame: &mut Frame,
    area: Rect,
    health: Option<&mut IterationHealth>,
    current_iteration: Option<&Iteration>,
    is_focused: bool,
    issues: &[TrackedIssue],
    shadow_work_cache: &[TrackedIssue],
    unplanned_work_cache: &HashMap<u64, DateTime<Utc>>,
) {
    let border_color = if is_focused {
        styles::CYAN
    } else {
        styles::BORDER
    };

    let Some(health) = health else {
        let msg = if current_iteration.is_some() {
            "Loading iteration health\u{2026}"
        } else {
            "No active iteration"
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(
            format!("  {msg}"),
            styles::help_desc_style(),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" Iteration Health ", styles::title_style())),
        );
        frame.render_widget(paragraph, area);
        return;
    };

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    let inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    if inner.height < 3 {
        return;
    }

    let parts = Layout::vertical([
        Constraint::Length(1), // Progress line
        Constraint::Length(1), // Tab bar
        Constraint::Min(1),    // List area
    ])
    .split(inner);

    render_progress_line(frame, parts[0], &*health, current_iteration);
    render_health_tabs(frame, parts[1], &*health);
    render_health_list(
        frame,
        parts[2],
        health,
        is_focused,
        issues,
        shadow_work_cache,
        unplanned_work_cache,
    );
}

fn render_progress_line(
    frame: &mut Frame,
    area: Rect,
    health: &IterationHealth,
    current_iteration: Option<&Iteration>,
) {
    let iter_label = current_iteration.map_or_else(
        || "Iteration".to_string(),
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

    // Progress bar: manual █░ rendering
    let pct = (health.done_issues * 100)
        .checked_div(health.total_issues)
        .unwrap_or(0);
    let bar_width = 12;
    let filled = (pct * bar_width) / 100;
    let empty = bar_width - filled;
    let bar_filled: String = "\u{2588}".repeat(filled);
    let bar_empty: String = "\u{2591}".repeat(empty);

    // Burn rate indicator
    let (burn_label, burn_color) = match health.burn_rate {
        BurnRate::Ahead => ("\u{25b2} Ahead", styles::GREEN),
        BurnRate::OnTrack => ("\u{25cf} On Track", styles::GREEN),
        BurnRate::Behind => ("\u{25bc} Behind", styles::RED),
        BurnRate::Unknown => ("\u{25cb} \u{2014}", styles::TEXT_DIM),
    };

    let mut spans = vec![
        Span::styled(
            format!(" {iter_label}"),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" \u{2502} ", Style::default().fg(styles::BORDER)),
    ];

    // Day X/Y (N days left)
    if health.days_total > 0 {
        spans.push(Span::styled(
            format!(
                "Day {}/{} ({}d left)",
                health.days_elapsed, health.days_total, health.days_remaining
            ),
            Style::default().fg(styles::TEXT),
        ));
        spans.push(Span::styled(
            " \u{2502} ",
            Style::default().fg(styles::BORDER),
        ));
    }

    // Progress bar
    spans.push(Span::styled(bar_filled, Style::default().fg(styles::GREEN)));
    spans.push(Span::styled(
        bar_empty,
        Style::default().fg(styles::TEXT_DIM),
    ));
    spans.push(Span::styled(
        format!(" {}/{} done", health.done_issues, health.total_issues),
        Style::default().fg(styles::TEXT),
    ));
    spans.push(Span::styled(
        " \u{2502} ",
        Style::default().fg(styles::BORDER),
    ));

    // Burn rate
    spans.push(Span::styled(burn_label, Style::default().fg(burn_color)));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_health_tabs(frame: &mut Frame, area: Rect, health: &IterationHealth) {
    let tabs = [
        (
            HealthTab::UnplannedWork,
            "Unplanned",
            health.unplanned_work.indices.len(),
            health.unplanned_work_loading,
        ),
        (
            HealthTab::ShadowWork,
            "Shadow Work",
            health.shadow_work.indices.len(),
            false,
        ),
        (
            HealthTab::AtRisk,
            "At Risk",
            health.at_risk.indices.len(),
            false,
        ),
    ];

    let mut spans = vec![Span::raw(" ")];
    for (i, (tab, label, count, loading)) in tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default()));
        }
        let is_active = *tab == health.active_tab;
        let count_str = if *loading {
            format!("{label} {}", styles::ICON_LOADING)
        } else {
            format!("{label} {count}")
        };
        if is_active {
            spans.push(Span::styled(
                format!("[{count_str}]"),
                Style::default()
                    .fg(styles::CYAN)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            let color = if *count > 0 {
                styles::YELLOW
            } else {
                styles::TEXT_DIM
            };
            spans.push(Span::styled(
                format!(" {count_str} "),
                Style::default().fg(color),
            ));
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[allow(clippy::too_many_arguments)]
fn render_health_list(
    frame: &mut Frame,
    area: Rect,
    health: &mut IterationHealth,
    is_focused: bool,
    issues: &[TrackedIssue],
    shadow_work_cache: &[TrackedIssue],
    unplanned_work_cache: &HashMap<u64, DateTime<Utc>>,
) {
    let list = health.active_list();
    let tab = health.active_tab;

    if list.indices.is_empty() {
        let msg = if health.active_tab_loading() {
            "  Loading\u{2026}"
        } else {
            match tab {
                HealthTab::UnplannedWork => "  No unplanned work detected",
                HealthTab::ShadowWork => "  No shadow work detected",
                HealthTab::AtRisk => "  No at-risk issues",
            }
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(msg, styles::help_desc_style())));
        frame.render_widget(paragraph, area);
        return;
    }

    // Pick the right source slice for this tab
    let source: &[TrackedIssue] = match tab {
        HealthTab::UnplannedWork | HealthTab::AtRisk => issues,
        HealthTab::ShadowWork => shadow_work_cache,
    };

    let rows: Vec<Row> = list
        .indices
        .iter()
        .filter_map(|&i| source.get(i))
        .enumerate()
        .map(|(i, item)| {
            let iid_str = format!(" #{}", item.issue.iid);
            let assignee = item
                .issue
                .assignees
                .first()
                .map_or(String::new(), |a| format!("@{}", a.username));
            let detail = match tab {
                HealthTab::UnplannedWork => unplanned_work_cache
                    .get(&item.issue.id)
                    .map_or_else(String::new, |dt| format!("added {}", dt.format("%b %d"))),
                HealthTab::ShadowWork => {
                    let closed = item.issue.closed_at.unwrap_or(item.issue.updated_at);
                    format!("closed {}", closed.format("%b %d"))
                }
                HealthTab::AtRisk => {
                    let days = (Utc::now() - item.issue.updated_at).num_days();
                    format!("{days}d no update")
                }
            };
            let row = Row::new(vec![
                Cell::from(Span::styled(iid_str, Style::default().fg(styles::TEXT_DIM))),
                Cell::from(Span::styled(
                    item.issue.title.as_str(),
                    Style::default().fg(styles::TEXT),
                )),
                Cell::from(Span::styled(assignee, Style::default().fg(styles::CYAN))),
                Cell::from(Span::styled(detail, Style::default().fg(styles::YELLOW))),
            ]);
            if i % 2 == 1 {
                row.style(styles::row_alt_style())
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(7),  // #iid
        Constraint::Min(15),    // title
        Constraint::Length(12), // @assignee
        Constraint::Length(16), // detail
    ];

    let table = if is_focused {
        Table::new(rows, widths).row_highlight_style(styles::selected_style())
    } else {
        Table::new(rows, widths)
    };

    frame.render_stateful_widget(table, area, &mut health.active_list_mut().table_state);
}

// ── Summary Panel (left side) ──

fn render_quick_stats(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    active_team: Option<usize>,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
) {
    let outer = styles::block("Overview");
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.height < 4 {
        return;
    }

    // Split: stats summary (3 lines) + member table (rest)
    let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(inner);

    render_stats_summary(frame, parts[0], config, issues, mrs);
    render_member_table(frame, parts[1], config, active_team, issues, mrs);
}

fn render_stats_summary(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
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

    let mut issue_spans = vec![
        Span::styled(
            format!(" {} ", styles::ICON_ISSUES),
            styles::section_header_style(),
        ),
        Span::styled(
            format!("{tracking_issues}"),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" tracking  ", Style::default().fg(styles::TEXT_DIM)),
        Span::styled(
            format!("{external_issues}"),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" external", Style::default().fg(styles::TEXT_DIM)),
    ];
    if unassigned_issues > 0 {
        issue_spans.push(Span::styled("  ", Style::default()));
        issue_spans.push(Span::styled(
            format!("{unassigned_issues} unassigned"),
            styles::error_style(),
        ));
    }

    let mut mr_spans = vec![
        Span::styled(
            format!(" {} ", styles::ICON_MRS),
            styles::section_header_style(),
        ),
        Span::styled(
            format!("{open_mrs}"),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" open  ", Style::default().fg(styles::TEXT_DIM)),
        Span::styled(
            format!("{draft_mrs}"),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" draft", Style::default().fg(styles::TEXT_DIM)),
    ];
    if my_review_mrs > 0 {
        mr_spans.push(Span::styled("  ", Style::default()));
        mr_spans.push(Span::styled(
            format!("{my_review_mrs} review"),
            Style::default().fg(styles::YELLOW),
        ));
    }

    let lines = vec![
        Line::from(issue_spans),
        Line::from(mr_spans),
        Line::from(""),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_member_table(
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
                Cell::from(Span::styled(
                    member.clone(),
                    Style::default().fg(styles::TEXT),
                )),
                Cell::from(Span::styled(
                    issue_count.to_string(),
                    Style::default()
                        .fg(styles::TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    mr_count.to_string(),
                    Style::default()
                        .fg(styles::TEXT_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                )),
            ]);
            if i % 2 == 1 {
                row.style(styles::row_alt_style())
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Min(12),   // member name
        Constraint::Length(6), // issues
        Constraint::Length(6), // MRs
    ];
    let table = Table::new(rows, widths).header(Row::new(vec![
        Cell::from(Span::styled(" Member", styles::header_style())),
        Cell::from(Span::styled("Iss", styles::header_style())),
        Cell::from(Span::styled("MRs", styles::header_style())),
    ]));
    frame.render_widget(table, area);
}

/// Compute iteration health metrics from available data.
///
/// This is a pure function that derives all health metrics from the provided data.
/// Called from `App::compute_iteration_health()`.
pub fn compute_health(
    issues: &[TrackedIssue],
    current_iteration: &Iteration,
    unplanned_work_cache: &HashMap<u64, DateTime<Utc>>,
    unplanned_work_loading: bool,
    shadow_work_cache: &[TrackedIssue],
    prev_health: Option<&IterationHealth>,
) -> IterationHealth {
    let current_id = &current_iteration.id;

    // Parse iteration dates
    let start_date = current_iteration
        .start_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let due_date = current_iteration
        .due_date
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
    let today = Utc::now().date_naive();

    let (days_elapsed, days_remaining, days_total) = match (start_date, due_date) {
        (Some(start), Some(end)) => {
            let total = (end - start).num_days();
            let elapsed = (today - start).num_days().max(0).min(total);
            let remaining = (end - today).num_days().max(0);
            (elapsed, remaining, total)
        }
        _ => (0, 0, 0),
    };

    // Collect iteration issues
    let iter_issues: Vec<&TrackedIssue> = issues
        .iter()
        .filter(|i| {
            i.issue
                .iteration
                .as_ref()
                .is_some_and(|it| it.id == *current_id)
        })
        .collect();

    let total_issues = iter_issues.len();

    // Determine "done" via status category or state
    let is_done = |ti: &TrackedIssue| -> bool {
        ti.issue
            .custom_status_category
            .as_deref()
            .map_or(ti.issue.state == "closed", |cat| cat == "done")
    };

    let done_issues = iter_issues.iter().filter(|i| is_done(i)).count();

    // Burn rate — precision loss is fine for small counts
    #[allow(clippy::cast_precision_loss)]
    let burn_rate = if days_total > 0 && total_issues > 0 {
        let expected_pct = days_elapsed as f64 / days_total as f64;
        let actual_pct = done_issues as f64 / total_issues as f64;
        if expected_pct < 0.05 {
            // Too early to judge
            BurnRate::Unknown
        } else {
            let ratio = actual_pct / expected_pct;
            if ratio >= 1.1 {
                BurnRate::Ahead
            } else if ratio >= 0.8 {
                BurnRate::OnTrack
            } else {
                BurnRate::Behind
            }
        }
    } else {
        BurnRate::Unknown
    };

    // Unplanned work: issues added 3+ days after iteration start (indices into `issues`)
    let unplanned_threshold = start_date
        .map(|s| s.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc() + chrono::Duration::days(3));
    let mut unplanned_work = ItemList::<TrackedIssue>::default();
    if let Some(threshold) = unplanned_threshold {
        for (i, item) in issues.iter().enumerate() {
            let in_iter = item
                .issue
                .iteration
                .as_ref()
                .is_some_and(|it| it.id == *current_id);
            if in_iter
                && let Some(added_at) = unplanned_work_cache.get(&item.issue.id)
                && *added_at > threshold
            {
                unplanned_work.indices.push(i);
            }
        }
    }
    unplanned_work.indices.sort_by(|&a, &b| {
        let added_a = unplanned_work_cache
            .get(&issues[a].issue.id)
            .unwrap_or(&issues[a].issue.created_at);
        let added_b = unplanned_work_cache
            .get(&issues[b].issue.id)
            .unwrap_or(&issues[b].issue.created_at);
        added_b.cmp(added_a)
    });
    unplanned_work.clamp_selection();

    // Shadow work: closed issues updated during iteration but not in it (indices into `shadow_work_cache`).
    // Exclude "canceled" category (duplicates, won't do, etc.) — only real completed work.
    let is_canceled = |ti: &TrackedIssue| -> bool {
        ti.issue
            .custom_status_category
            .as_deref()
            .is_some_and(|cat| cat == "canceled")
    };

    // Shadow work: DB already filters by closed_at range and excludes current iteration.
    // Here we just exclude canceled issues (status category check needs Rust).
    let mut shadow_work = ItemList::<TrackedIssue>::default();
    for (i, ti) in shadow_work_cache.iter().enumerate() {
        if !is_canceled(ti) {
            shadow_work.indices.push(i);
        }
    }
    shadow_work.indices.sort_by(|&a, &b| {
        let issue_a = &shadow_work_cache[a].issue;
        let issue_b = &shadow_work_cache[b].issue;
        let closed_a = issue_a.closed_at.unwrap_or(issue_a.updated_at);
        let closed_b = issue_b.closed_at.unwrap_or(issue_b.updated_at);
        closed_b.cmp(&closed_a)
    });
    shadow_work.clamp_selection();

    // At risk: iteration issues with "active" category status, not updated in 5+ days (indices into `issues`)
    let stale_threshold = Utc::now() - chrono::Duration::days(5);
    let is_active_status = |ti: &TrackedIssue| -> bool {
        ti.issue
            .custom_status_category
            .as_deref()
            .is_some_and(|cat| cat == "active")
    };

    let mut at_risk = ItemList::<TrackedIssue>::default();
    for (i, item) in issues.iter().enumerate() {
        let in_iter = item
            .issue
            .iteration
            .as_ref()
            .is_some_and(|it| it.id == *current_id);
        if in_iter
            && is_active_status(item)
            && item.issue.updated_at < stale_threshold
            && !is_done(item)
        {
            at_risk.indices.push(i);
        }
    }
    at_risk.indices.sort_by(|&a, &b| {
        let issue_a = &issues[a].issue;
        let issue_b = &issues[b].issue;
        issue_a.updated_at.cmp(&issue_b.updated_at)
    });
    at_risk.clamp_selection();

    // Preserve tab + selection state from previous health
    let active_tab = prev_health.map_or(HealthTab::default(), |h| {
        unplanned_work.table_state = h.unplanned_work.table_state.clone();
        shadow_work.table_state = h.shadow_work.table_state.clone();
        at_risk.table_state = h.at_risk.table_state.clone();
        unplanned_work.clamp_selection();
        shadow_work.clamp_selection();
        at_risk.clamp_selection();
        h.active_tab
    });

    IterationHealth {
        total_issues,
        done_issues,
        days_elapsed,
        days_remaining,
        days_total,
        burn_rate,
        unplanned_work,
        shadow_work,
        at_risk,
        unplanned_work_loading,
        active_tab,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitlab::types::Issue;

    fn make_issue(id: u64, iteration_id: Option<&str>) -> TrackedIssue {
        TrackedIssue {
            project_path: "test/project".to_string(),
            issue: Issue {
                id,
                iid: id,
                title: format!("Issue {id}"),
                state: "opened".to_string(),
                author: None,
                assignees: vec![],
                labels: vec![],
                milestone: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                closed_at: None,
                web_url: String::new(),
                description: None,
                user_notes_count: 0,
                references: None,
                custom_status: None,
                custom_status_category: None,
                iteration: iteration_id.map(|id| crate::gitlab::types::Iteration {
                    id: id.to_string(),
                    title: "Sprint 1".to_string(),
                    start_date: None,
                    due_date: None,
                    state: "current".to_string(),
                }),
                weight: None,
            },
        }
    }

    #[test]
    fn partition_issues_with_empty_columns_does_not_panic() {
        let mut board = IterationBoardState::default();
        assert!(board.columns.is_empty());

        let iter = crate::gitlab::types::Iteration {
            id: "gid://gitlab/Iteration/1".to_string(),
            title: "Sprint 1".to_string(),
            start_date: Some("2026-04-01".to_string()),
            due_date: Some("2026-04-14".to_string()),
            state: "current".to_string(),
        };
        let issues = vec![
            make_issue(1, Some("gid://gitlab/Iteration/1")),
            make_issue(2, Some("gid://gitlab/Iteration/1")),
        ];

        // Must not panic even though columns is empty
        board.partition_issues(
            &issues,
            Some(&iter),
            &std::collections::HashMap::new(),
            "",
            &[],
        );
        assert!(board.columns.is_empty());
    }

    #[test]
    fn partition_issues_with_columns_assigns_correctly() {
        let mut board = IterationBoardState {
            columns: vec![
                StatusColumn {
                    list: ItemList::default(),
                    status_name: "No Status".to_string(),
                    status_matches: vec![String::new()],
                },
                StatusColumn {
                    list: ItemList::default(),
                    status_name: "In Progress".to_string(),
                    status_matches: vec!["in progress".to_string()],
                },
            ],
            ..Default::default()
        };

        let iter = crate::gitlab::types::Iteration {
            id: "gid://gitlab/Iteration/1".to_string(),
            title: "Sprint 1".to_string(),
            start_date: None,
            due_date: None,
            state: "current".to_string(),
        };

        let mut in_progress = make_issue(1, Some("gid://gitlab/Iteration/1"));
        in_progress.issue.custom_status = Some("In Progress".to_string());
        let no_status = make_issue(2, Some("gid://gitlab/Iteration/1"));

        let issues = vec![in_progress, no_status];
        board.partition_issues(
            &issues,
            Some(&iter),
            &std::collections::HashMap::new(),
            "",
            &[],
        );

        assert_eq!(board.columns[0].list.indices.len(), 1); // "No Status"
        assert_eq!(board.columns[1].list.indices.len(), 1); // "In Progress"
    }
}
