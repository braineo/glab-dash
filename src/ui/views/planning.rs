use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use crate::config::Config;
use crate::gitlab::types::{Iteration, TrackedIssue};
use crate::sort::{self, SortDirection, SortField, SortSpec};
use crate::ui::{RenderCtx, keys, styles};

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum PlanningLayout {
    ThreeColumn,
    TwoColumn,
}

pub struct PlanningViewState {
    pub focused_column: usize,
    pub column_states: [TableState; 3],
    pub column_indices: [Vec<usize>; 3],
    pub column_visible: [bool; 3],
    pub prev_iteration: Option<Iteration>,
    pub current_iteration: Option<Iteration>,
    pub next_iteration: Option<Iteration>,
    pub layout_mode: PlanningLayout,
    pub search_query: String,
    pub searching: bool,
}

impl Default for PlanningViewState {
    fn default() -> Self {
        Self {
            focused_column: 1, // start on current
            column_states: [
                TableState::default(),
                TableState::default(),
                TableState::default(),
            ],
            column_indices: [Vec::new(), Vec::new(), Vec::new()],
            column_visible: [true, true, true],
            prev_iteration: None,
            current_iteration: None,
            next_iteration: None,
            layout_mode: PlanningLayout::ThreeColumn,
            search_query: String::new(),
            searching: false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum PlanningAction {
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
    /// Show chord popup to pick target iteration (or remove)
    MoveIteration,
}

impl PlanningViewState {
    pub fn handle_key(&mut self, key: &KeyEvent) -> PlanningAction {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search_query.clear();
                    return PlanningAction::Refilter;
                }
                KeyCode::Enter => {
                    self.searching = false;
                    return PlanningAction::None;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    return PlanningAction::Refilter;
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    return PlanningAction::Refilter;
                }
                _ => return PlanningAction::None,
            }
        }

        // Column navigation: [ / ]
        match key.code {
            KeyCode::Char('[') => {
                self.move_focus_left();
                return PlanningAction::None;
            }
            KeyCode::Char(']') => {
                self.move_focus_right();
                return PlanningAction::None;
            }
            _ => {}
        }

        let col = self.focused_column;
        let len = self.column_indices[col].len();

        if len == 0 {
            return match key.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                    PlanningAction::None
                }
                KeyCode::Char('r') => PlanningAction::Refresh,
                KeyCode::Char('<') => {
                    self.column_visible[0] = !self.column_visible[0];
                    self.clamp_focus();
                    PlanningAction::None
                }
                KeyCode::Char('>') => {
                    self.column_visible[2] = !self.column_visible[2];
                    self.clamp_focus();
                    PlanningAction::None
                }
                KeyCode::Char('v') => {
                    self.toggle_layout();
                    PlanningAction::Refilter
                }
                _ => PlanningAction::None,
            };
        }

        let current = self.column_states[col].selected().unwrap_or(0);

        if keys::is_down(key) {
            self.column_states[col].select(Some((current + 1).min(len - 1)));
        } else if keys::is_up(key) {
            self.column_states[col].select(Some(current.saturating_sub(1)));
        } else if keys::is_top(key) {
            self.column_states[col].select(Some(0));
        } else if keys::is_bottom(key) {
            self.column_states[col].select(Some(len - 1));
        } else if keys::is_page_down(key) {
            self.column_states[col].select(Some((current + 20).min(len - 1)));
        } else if keys::is_page_up(key) {
            self.column_states[col].select(Some(current.saturating_sub(20)));
        } else if keys::is_enter(key) {
            return PlanningAction::OpenDetail;
        } else {
            match key.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                }
                KeyCode::Char('r') => return PlanningAction::Refresh,
                KeyCode::Char('s') => return PlanningAction::SetStatus,
                KeyCode::Char('x') => return PlanningAction::ToggleState,
                KeyCode::Char('l') => return PlanningAction::EditLabels,
                KeyCode::Char('a') => return PlanningAction::EditAssignee,
                KeyCode::Char('c') => return PlanningAction::Comment,
                KeyCode::Char('o') => return PlanningAction::OpenBrowser,
                KeyCode::Char('I') => return PlanningAction::MoveIteration,
                KeyCode::Char('v') => {
                    self.toggle_layout();
                    return PlanningAction::Refilter;
                }
                _ => {}
            }
        }
        PlanningAction::None
    }

    pub fn selected_issue<'a>(&self, issues: &'a [TrackedIssue]) -> Option<&'a TrackedIssue> {
        let col = self.focused_column;
        self.column_states[col]
            .selected()
            .and_then(|sel| self.column_indices[col].get(sel))
            .and_then(|&idx| issues.get(idx))
    }

    fn visible_columns(&self) -> Vec<usize> {
        (0..3).filter(|&i| self.column_visible[i]).collect()
    }

    fn move_focus_left(&mut self) {
        let visible = self.visible_columns();
        if let Some(pos) = visible.iter().position(|&c| c == self.focused_column)
            && pos > 0
        {
            self.focused_column = visible[pos - 1];
        }
    }

    fn move_focus_right(&mut self) {
        let visible = self.visible_columns();
        if let Some(pos) = visible.iter().position(|&c| c == self.focused_column)
            && pos + 1 < visible.len()
        {
            self.focused_column = visible[pos + 1];
        }
    }

    fn clamp_focus(&mut self) {
        let visible = self.visible_columns();
        if !visible.is_empty() && !visible.contains(&self.focused_column) {
            self.focused_column = visible[visible.len() / 2];
        }
    }

    fn toggle_layout(&mut self) {
        self.layout_mode = match self.layout_mode {
            PlanningLayout::ThreeColumn => PlanningLayout::TwoColumn,
            PlanningLayout::TwoColumn => PlanningLayout::ThreeColumn,
        };
        // In 2-column mode: col 0 = other, col 1 = current; col 2 is unused
        if self.layout_mode == PlanningLayout::TwoColumn {
            self.column_visible = [true, true, false];
            if self.focused_column == 2 {
                self.focused_column = 1;
            }
        } else {
            self.column_visible = [true, true, true];
        }
    }

    /// Partition issues into columns based on iteration and layout mode.
    pub fn partition_issues(
        &mut self,
        issues: &[TrackedIssue],
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        for col in &mut self.column_indices {
            col.clear();
        }

        let current_id = self.current_iteration.as_ref().map(|i| i.id.as_str());

        match self.layout_mode {
            PlanningLayout::ThreeColumn => {
                let prev_id = self.prev_iteration.as_ref().map(|i| i.id.as_str());
                let next_id = self.next_iteration.as_ref().map(|i| i.id.as_str());

                for (i, item) in issues.iter().enumerate() {
                    if !self.matches_search(item) {
                        continue;
                    }
                    let iter_id = item.issue.iteration.as_ref().map(|it| it.id.as_str());
                    if iter_id == prev_id && prev_id.is_some() {
                        self.column_indices[0].push(i);
                    } else if iter_id == current_id && current_id.is_some() {
                        self.column_indices[1].push(i);
                    } else if iter_id == next_id && next_id.is_some() {
                        self.column_indices[2].push(i);
                    }
                }
            }
            PlanningLayout::TwoColumn => {
                for (i, item) in issues.iter().enumerate() {
                    if !self.matches_search(item) {
                        continue;
                    }
                    let iter_id = item.issue.iteration.as_ref().map(|it| it.id.as_str());
                    if iter_id == current_id && current_id.is_some() {
                        self.column_indices[1].push(i);
                    } else {
                        self.column_indices[0].push(i);
                    }
                }
            }
        }

        // Default sort: workflow:: then p:: label scopes
        let default_sort = vec![
            SortSpec {
                field: SortField::Label,
                direction: SortDirection::Asc,
                label_scope: Some("workflow".to_string()),
            },
            SortSpec {
                field: SortField::Label,
                direction: SortDirection::Asc,
                label_scope: Some("p".to_string()),
            },
        ];
        for col in &mut self.column_indices {
            sort::sort_issues(col, issues, &default_sort, label_orders);
        }

        // Clamp selections
        for i in 0..3 {
            if self.column_indices[i].is_empty() {
                self.column_states[i].select(None);
            } else if self.column_states[i].selected().is_none() {
                self.column_states[i].select(Some(0));
            } else if let Some(sel) = self.column_states[i].selected()
                && sel >= self.column_indices[i].len()
            {
                self.column_states[i].select(Some(self.column_indices[i].len() - 1));
            }
        }
    }

    fn matches_search(&self, item: &TrackedIssue) -> bool {
        if self.search_query.is_empty() {
            return true;
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
        self.search_query
            .to_lowercase()
            .split_whitespace()
            .all(|word| haystack.contains(word))
    }
}

// ── Rendering ──

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut PlanningViewState,
    issues: &[TrackedIssue],
    config: &Config,
    active_team: usize,
    ctx: &RenderCtx,
) {
    let visible: Vec<usize> = state.visible_columns();
    if visible.is_empty() {
        let msg = Paragraph::new("No columns visible. Press [ or ] to show columns.");
        frame.render_widget(msg, area);
        return;
    }

    // Search bar takes 1 row if searching
    let search_height = if state.searching { 1 } else { 0 };
    let main_chunks =
        Layout::vertical([Constraint::Length(search_height), Constraint::Min(1)]).split(area);

    if state.searching {
        let search_line = Line::from(vec![
            Span::styled(" / ", styles::help_key_style()),
            Span::raw(&state.search_query),
            Span::styled("▏", Style::default()),
        ]);
        frame.render_widget(Paragraph::new(search_line), main_chunks[0]);
    }

    let col_area = main_chunks[1];

    // Split into equal-width columns
    let constraints: Vec<Constraint> = visible
        .iter()
        .map(|_| Constraint::Ratio(1, visible.len() as u32))
        .collect();
    let columns = Layout::horizontal(constraints).split(col_area);

    let team_members = config.team_members(active_team);

    for (vi, &col_idx) in visible.iter().enumerate() {
        let is_focused = col_idx == state.focused_column;
        render_column(
            frame,
            columns[vi],
            state,
            col_idx,
            issues,
            &team_members,
            is_focused,
            ctx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_column(
    frame: &mut Frame,
    area: Rect,
    state: &mut PlanningViewState,
    col_idx: usize,
    issues: &[TrackedIssue],
    team_members: &[String],
    is_focused: bool,
    _ctx: &RenderCtx,
) {
    let title = column_title(state, col_idx);
    let count = state.column_indices[col_idx].len();
    let total_weight: u32 = state.column_indices[col_idx]
        .iter()
        .filter_map(|&i| issues.get(i))
        .filter_map(|item| item.issue.weight)
        .sum();
    let header = format!("{title}  ({count} issues, {total_weight}w)");

    let border_style = if is_focused {
        Style::default()
            .fg(styles::BLUE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::TEXT_DIM)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(
            header,
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

    if inner.height < 2 {
        return;
    }

    // Reserve space for member stats footer
    let stats = member_stats(&state.column_indices[col_idx], issues, team_members);
    let stats_height = if stats.is_empty() {
        0
    } else {
        (stats.len() as u16).min(inner.height.saturating_sub(2))
    };

    let parts =
        Layout::vertical([Constraint::Min(1), Constraint::Length(stats_height)]).split(inner);

    // Issue table
    let indices = &state.column_indices[col_idx];
    let rows: Vec<Row> = indices
        .iter()
        .filter_map(|&i| issues.get(i))
        .map(|item| {
            let status_icon = status_icon_for(&item.issue.custom_status);
            let iid = format!("#{}", item.issue.iid);
            let assignee = item
                .issue
                .assignees
                .first()
                .map(|u| u.username.as_str())
                .unwrap_or("-");
            let weight = item
                .issue
                .weight
                .map(|w| format!("{w}w"))
                .unwrap_or_default();

            // Truncate title to reasonable length
            let title = &item.issue.title;

            Row::new(vec![
                Cell::from(Span::styled(
                    status_icon,
                    styles::status_style(
                        item.issue
                            .custom_status
                            .as_deref()
                            .unwrap_or(&item.issue.state),
                    ),
                )),
                Cell::from(Span::styled(iid, Style::default().fg(styles::TEXT_DIM))),
                Cell::from(Span::styled(
                    title.as_str(),
                    Style::default().fg(styles::TEXT),
                )),
                Cell::from(Span::styled(assignee, Style::default().fg(styles::CYAN))),
                Cell::from(Span::styled(weight, Style::default().fg(styles::YELLOW))),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(2),  // status icon
        Constraint::Length(6),  // iid
        Constraint::Min(8),     // title
        Constraint::Length(10), // assignee
        Constraint::Length(4),  // weight
    ];

    let table = Table::new(rows, widths).row_highlight_style(styles::selected_style());

    frame.render_stateful_widget(table, parts[0], &mut state.column_states[col_idx]);

    // Member stats footer
    if stats_height > 0 {
        let stat_lines: Vec<Line> = stats
            .iter()
            .map(|(name, count, weight)| {
                Line::from(vec![
                    Span::styled(format!(" {name}"), Style::default().fg(styles::CYAN)),
                    Span::styled(format!(": {count}"), Style::default().fg(styles::TEXT)),
                    Span::styled(
                        format!(" ({weight}w)"),
                        Style::default().fg(styles::TEXT_DIM),
                    ),
                ])
            })
            .collect();
        let stats_widget = Paragraph::new(stat_lines);
        frame.render_widget(stats_widget, parts[1]);
    }
}

pub fn iteration_label(iter: &Iteration) -> String {
    if !iter.title.is_empty() {
        return iter.title.clone();
    }
    // Titles are often null for auto-generated iterations — use date range
    match (&iter.start_date, &iter.due_date) {
        (Some(s), Some(d)) => format!("{s} — {d}"),
        (Some(s), None) => format!("{s} —"),
        (None, Some(d)) => format!("— {d}"),
        (None, None) => iter.id.clone(),
    }
}

fn column_title(state: &PlanningViewState, col_idx: usize) -> String {
    match state.layout_mode {
        PlanningLayout::ThreeColumn => match col_idx {
            0 => state
                .prev_iteration
                .as_ref()
                .map(|i| format!("◁ {}", iteration_label(i)))
                .unwrap_or_else(|| "◁ Previous".to_string()),
            1 => state
                .current_iteration
                .as_ref()
                .map(|i| format!("● {}", iteration_label(i)))
                .unwrap_or_else(|| "● Current".to_string()),
            2 => state
                .next_iteration
                .as_ref()
                .map(|i| format!("▷ {}", iteration_label(i)))
                .unwrap_or_else(|| "▷ Next".to_string()),
            _ => String::new(),
        },
        PlanningLayout::TwoColumn => match col_idx {
            0 => "Other".to_string(),
            1 => state
                .current_iteration
                .as_ref()
                .map(|i| format!("● {}", iteration_label(i)))
                .unwrap_or_else(|| "● Current".to_string()),
            _ => String::new(),
        },
    }
}

fn status_icon_for(custom_status: &Option<String>) -> &'static str {
    match custom_status.as_deref() {
        Some(s) if s.to_lowercase().contains("done") => "✓",
        Some(s) if s.to_lowercase().contains("progress") => "▶",
        Some(s) if s.to_lowercase().contains("review") => "◉",
        Some(s) if s.to_lowercase().contains("block") => "⊘",
        Some(s) if s.to_lowercase().contains("cancel") => "✗",
        Some(_) => "●",
        None => "○",
    }
}

fn member_stats(
    indices: &[usize],
    issues: &[TrackedIssue],
    team_members: &[String],
) -> Vec<(String, usize, u32)> {
    let mut stats: Vec<(String, usize, u32)> = Vec::new();
    for member in team_members {
        let mut count = 0;
        let mut weight = 0u32;
        for &idx in indices {
            if let Some(item) = issues.get(idx)
                && item
                    .issue
                    .assignees
                    .iter()
                    .any(|a| a.username.eq_ignore_ascii_case(member))
            {
                count += 1;
                weight += item.issue.weight.unwrap_or(0);
            }
        }
        if count > 0 {
            stats.push((member.clone(), count, weight));
        }
    }
    stats
}
