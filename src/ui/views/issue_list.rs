use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Row, Table, TableState};

use crate::filter::{FilterCondition, matches_issue};
use crate::gitlab::types::{ItemSource, TrackedIssue};
use crate::ui::{components, keys, styles};

#[derive(Default)]
pub struct IssueListState {
    pub table_state: TableState,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub searching: bool,
}

impl IssueListState {
    pub fn apply_filters(
        &mut self,
        issues: &[TrackedIssue],
        conditions: &[FilterCondition],
        me: &str,
        team_members: &[String],
    ) {
        self.filtered_indices = issues
            .iter()
            .enumerate()
            .filter(|(_, item)| matches_issue(item, conditions, me, team_members))
            .filter(|(_, item)| {
                if self.search_query.is_empty() {
                    true
                } else {
                    let q = self.search_query.to_lowercase();
                    item.issue.title.to_lowercase().contains(&q)
                        || item
                            .issue
                            .assignees
                            .iter()
                            .any(|a| a.username.to_lowercase().contains(&q))
                        || item
                            .issue
                            .labels
                            .iter()
                            .any(|l| l.to_lowercase().contains(&q))
                }
            })
            .map(|(i, _)| i)
            .collect();

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
                KeyCode::Char('x') => return IssueListAction::ToggleState,
                KeyCode::Char('l') => return IssueListAction::EditLabels,
                KeyCode::Char('a') => return IssueListAction::EditAssignee,
                KeyCode::Char('c') => return IssueListAction::Comment,
                KeyCode::Char('o') => return IssueListAction::OpenBrowser,
                KeyCode::Char('f') => return IssueListAction::AddFilter,
                KeyCode::Char('F') => return IssueListAction::ClearFilters,
                KeyCode::Char('p') => return IssueListAction::PickPreset,
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
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut IssueListState,
    issues: &[TrackedIssue],
    conditions: &[FilterCondition],
    filter_focused: bool,
    filter_selected: usize,
) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // Filter bar
        Constraint::Min(1),    // Table
    ])
    .split(area);

    // Filter bar
    components::filter_bar::render(
        frame,
        chunks[0],
        conditions,
        filter_focused,
        filter_selected,
    );

    // Build table rows
    let rows: Vec<Row> = state
        .filtered_indices
        .iter()
        .map(|&idx| {
            let item = &issues[idx];
            let source_span = match &item.source {
                ItemSource::Tracking => Span::styled("TRK", styles::source_tracking_style()),
                ItemSource::External(p) => {
                    let short = p.rsplit('/').next().unwrap_or(p);
                    Span::styled(short.to_string(), styles::source_external_style())
                }
            };
            let assignees = item
                .issue
                .assignees
                .iter()
                .map(|a| a.username.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let labels = item.issue.labels.join(",");
            let age = format_age(&item.issue.updated_at);

            Row::new(vec![
                format!("#{}", item.issue.iid),
                source_span.to_string(),
                item.issue.title.clone(),
                item.issue.state.clone(),
                assignees,
                labels,
                age,
            ])
            .style(if item.issue.state == "closed" {
                styles::draft_style()
            } else {
                ratatui::style::Style::default()
            })
        })
        .collect();

    let widths = [
        Constraint::Length(7),  // IID
        Constraint::Length(10), // Source
        Constraint::Min(30),    // Title
        Constraint::Length(8),  // State
        Constraint::Length(15), // Assignees
        Constraint::Length(20), // Labels
        Constraint::Length(8),  // Age
    ];

    let header = Row::new(vec![
        "ID", "Source", "Title", "State", "Assignee", "Labels", "Updated",
    ])
    .style(styles::header_style())
    .bottom_margin(1);

    let title = if state.searching {
        format!(" Issues (/{}) ", state.search_query)
    } else {
        format!(" Issues ({}) ", state.filtered_indices.len())
    };

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(styles::selected_style())
        .highlight_symbol("▶ ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(styles::title_style()),
        );

    frame.render_stateful_widget(table, chunks[1], &mut state.table_state);
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
