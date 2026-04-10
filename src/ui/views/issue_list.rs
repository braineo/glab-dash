use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::style::Style;
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};

use std::collections::HashMap;

use crate::filter::{FilterCondition, matches_issue};
use crate::gitlab::types::TrackedIssue;
use crate::sort::{self, SortSpec};
use crate::ui::{components, keys, styles};

#[derive(Default)]
pub struct IssueListState {
    pub table_state: TableState,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub searching: bool,
    pub active_sort: Vec<SortSpec>,
}

impl IssueListState {
    pub fn apply_filters(
        &mut self,
        issues: &[TrackedIssue],
        conditions: &[FilterCondition],
        me: &str,
        team_members: &[String],
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        self.filtered_indices = issues
            .iter()
            .enumerate()
            .filter(|(_, item)| matches_issue(item, conditions, me, team_members))
            .filter(|(_, item)| {
                if self.search_query.is_empty() {
                    true
                } else {
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
            })
            .map(|(i, _)| i)
            .collect();

        // Sort
        sort::sort_issues(&mut self.filtered_indices, issues, &self.active_sort, label_orders);

        // Clamp selection
        if self.filtered_indices.is_empty() {
            self.table_state.select(None);
        } else if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        } else if let Some(sel) = self.table_state.selected()
            && sel >= self.filtered_indices.len()
        {
            self.table_state
                .select(Some(self.filtered_indices.len() - 1));
        }
    }

    pub fn selected_issue<'a>(&self, issues: &'a [TrackedIssue]) -> Option<&'a TrackedIssue> {
        self.table_state
            .selected()
            .and_then(|sel| self.filtered_indices.get(sel))
            .and_then(|&idx| issues.get(idx))
    }

    pub fn handle_key(&mut self, key: &KeyEvent, _total: usize) -> IssueListAction {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search_query.clear();
                    return IssueListAction::Refilter;
                }
                KeyCode::Enter => {
                    self.searching = false;
                    return IssueListAction::None;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    return IssueListAction::Refilter;
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    return IssueListAction::Refilter;
                }
                _ => return IssueListAction::None,
            }
        }

        let len = self.filtered_indices.len();
        if len == 0 {
            return match key.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                    IssueListAction::None
                }
                KeyCode::Char('r') => IssueListAction::Refresh,
                _ => IssueListAction::None,
            };
        }

        let current = self.table_state.selected().unwrap_or(0);

        if keys::is_down(key) {
            self.table_state.select(Some((current + 1).min(len - 1)));
        } else if keys::is_up(key) {
            self.table_state.select(Some(current.saturating_sub(1)));
        } else if keys::is_top(key) {
            self.table_state.select(Some(0));
        } else if keys::is_bottom(key) {
            self.table_state.select(Some(len - 1));
        } else if keys::is_page_down(key) {
            self.table_state.select(Some((current + 20).min(len - 1)));
        } else if keys::is_page_up(key) {
            self.table_state.select(Some(current.saturating_sub(20)));
        } else if keys::is_enter(key) {
            return IssueListAction::OpenDetail;
        } else {
            match key.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                }
                KeyCode::Char('r') => return IssueListAction::Refresh,
                KeyCode::Char('s') | KeyCode::Char('x') => return IssueListAction::ToggleState,
                KeyCode::Char('l') => return IssueListAction::EditLabels,
                KeyCode::Char('a') => return IssueListAction::EditAssignee,
                KeyCode::Char('c') => return IssueListAction::Comment,
                KeyCode::Char('o') => return IssueListAction::OpenBrowser,
                KeyCode::Char('f') => return IssueListAction::AddFilter,
                KeyCode::Char('F') => return IssueListAction::ClearFilters,
                KeyCode::Char('p') => return IssueListAction::PickPreset,
                KeyCode::Char('S') => return IssueListAction::PickSortPreset,
                _ => {}
            }
        }
        IssueListAction::None
    }
}

#[derive(Debug, PartialEq)]
pub enum IssueListAction {
    None,
    Refilter,
    OpenDetail,
    Refresh,
    ToggleState,
    EditLabels,
    EditAssignee,
    Comment,
    OpenBrowser,
    AddFilter,
    ClearFilters,
    PickPreset,
    PickSortPreset,
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut IssueListState,
    issues: &[TrackedIssue],
    conditions: &[FilterCondition],
    filter_focused: bool,
    filter_selected: usize,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let has_selection = state.table_state.selected().is_some();
    let chunks = Layout::vertical([
        Constraint::Length(1), // Filter bar
        Constraint::Min(1),    // Table
        Constraint::Length(if has_selection { 2 } else { 0 }), // Preview
    ])
    .split(area);

    // Filter + sort bar
    components::filter_bar::render(
        frame,
        chunks[0],
        conditions,
        &state.active_sort,
        filter_focused,
        filter_selected,
    );

    // Build table rows
    let selected_idx = state.table_state.selected();
    let rows: Vec<Row> = state
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(row_idx, &idx)| {
            let item = &issues[idx];
            let source_span = {
                let p = &item.project_path;
                let short = p.rsplit('/').next().unwrap_or(p);
                Span::styled(short.to_string(), styles::source_external_style())
            };
            let assignees = item
                .issue
                .assignees
                .iter()
                .map(|a| a.username.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let labels = styles::labels_compact(&item.issue.labels, 30, label_colors);
            let age = format_age(&item.issue.updated_at);

            // Show custom status if available, otherwise fall back to state
            let (state_icon, state_text) =
                if let Some(ref status) = item.issue.custom_status {
                    (styles::status_icon(status), status.clone())
                } else {
                    let icon = match item.issue.state.as_str() {
                        "opened" => styles::ICON_OPEN,
                        "closed" => styles::ICON_CLOSED,
                        _ => " ",
                    };
                    (icon, item.issue.state.clone())
                };

            let state_style = if item.issue.custom_status.is_some() {
                styles::status_style(&state_text)
            } else {
                styles::state_style(&item.issue.state)
            };

            let row = Row::new([
                Cell::from(format!("#{}", item.issue.iid)),
                Cell::from(source_span.to_string()),
                Cell::from(item.issue.title.clone()),
                Cell::from(Line::from(Span::styled(
                    format!("{state_icon} {state_text}"),
                    state_style,
                ))),
                Cell::from(assignees),
                Cell::from(labels),
                Cell::from(age),
            ]);
            let is_selected = selected_idx == Some(row_idx);
            let is_closed = item.issue.state == "closed";
            if is_selected {
                row.style(styles::selected_style())
            } else if is_closed {
                row.style(styles::draft_style())
            } else if row_idx % 2 == 1 {
                row.style(styles::row_alt_style())
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(7),  // IID
        Constraint::Length(10), // Source
        Constraint::Min(30),    // Title
        Constraint::Length(18), // State / Status
        Constraint::Length(15), // Assignees
        Constraint::Length(32), // Labels
        Constraint::Length(8),  // Age
    ];

    let header = Row::new(vec![
        "ID", "Source", "Title", "State", "Assignee", "Labels", "Updated",
    ])
    .style(styles::header_style())
    .bottom_margin(1);

    let table_block = if state.searching {
        let title_line = Line::from(vec![
            Span::styled(" Issues /", Style::default().fg(styles::CYAN).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(&state.search_query, Style::default().fg(styles::TEXT_BRIGHT).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled("▎", Style::default().fg(styles::CYAN)),
            Span::styled(" Enter", Style::default().fg(styles::YELLOW).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(":accept ", Style::default().fg(styles::TEXT_DIM)),
            Span::styled("Esc", Style::default().fg(styles::YELLOW).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(":cancel ", Style::default().fg(styles::TEXT_DIM)),
        ]);
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(styles::CYAN))
            .title(title_line)
    } else if !state.search_query.is_empty() {
        let title_line = Line::from(vec![
            Span::styled(" Issues /", Style::default().fg(styles::CYAN).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(&state.search_query, Style::default().fg(styles::TEXT_BRIGHT).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(" ", Style::default()),
        ]);
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(styles::BORDER))
            .title(title_line)
    } else {
        styles::block("Issues")
    };

    let table = Table::new(rows, widths)
        .header(header)
        .highlight_symbol(styles::ICON_SELECTOR)
        .block(table_block);

    frame.render_stateful_widget(table, chunks[1], &mut state.table_state);

    // Preview pane: show full labels of selected item
    if let Some(item) = state.selected_issue(issues) {
        let mut spans: Vec<Span> = vec![
            Span::styled(" Labels: ", styles::help_desc_style()),
        ];
        if item.issue.labels.is_empty() {
            spans.push(Span::styled("none", styles::help_desc_style()));
        } else {
            for (i, label) in item.issue.labels.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let color = label_colors.get(label.as_str()).map(|s| s.as_str());
                spans.extend(styles::label_spans(label, color));
            }
        }
        let preview = Paragraph::new(vec![
            Line::from(spans),
            Line::from(vec![
                Span::styled(" Assignees: ", styles::help_desc_style()),
                Span::styled(
                    item.issue
                        .assignees
                        .iter()
                        .map(|a| a.username.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    ratatui::style::Style::default().fg(styles::TEXT_BRIGHT),
                ),
                Span::styled("  Source: ", styles::help_desc_style()),
                Span::styled(
                    item.project_path.clone(),
                    ratatui::style::Style::default().fg(styles::TEXT),
                ),
            ]),
        ])
        .style(ratatui::style::Style::default().bg(styles::SURFACE));
        frame.render_widget(preview, chunks[2]);
    }
}

fn format_age(dt: &chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(*dt);
    if diff.num_days() > 0 {
        format!("{}d", diff.num_days())
    } else if diff.num_hours() > 0 {
        format!("{}h", diff.num_hours())
    } else {
        format!("{}m", diff.num_minutes())
    }
}
