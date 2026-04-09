use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Borders, Row, Table, TableState};

use crate::filter::{FilterCondition, matches_mr};
use crate::gitlab::types::{ItemSource, TrackedMergeRequest};
use crate::ui::{components, keys, styles};

#[derive(Default)]
pub struct MrListState {
    pub table_state: TableState,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub searching: bool,
}

impl MrListState {
    pub fn apply_filters(
        &mut self,
        mrs: &[TrackedMergeRequest],
        conditions: &[FilterCondition],
        me: &str,
        team_members: &[String],
    ) {
        self.filtered_indices = mrs
            .iter()
            .enumerate()
            .filter(|(_, item)| matches_mr(item, conditions, me, team_members))
            .filter(|(_, item)| {
                if self.search_query.is_empty() {
                    true
                } else {
                    let q = self.search_query.to_lowercase();
                    item.mr.title.to_lowercase().contains(&q)
                        || item
                            .mr
                            .author
                            .as_ref()
                            .is_some_and(|a| a.username.to_lowercase().contains(&q))
                        || item
                            .mr
                            .assignees
                            .iter()
                            .any(|a| a.username.to_lowercase().contains(&q))
                }
            })
            .map(|(i, _)| i)
            .collect();

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

    pub fn selected_mr<'a>(
        &self,
        mrs: &'a [TrackedMergeRequest],
    ) -> Option<&'a TrackedMergeRequest> {
        self.table_state
            .selected()
            .and_then(|sel| self.filtered_indices.get(sel))
            .and_then(|&idx| mrs.get(idx))
    }

    pub fn handle_key(&mut self, key: &KeyEvent, _total: usize) -> MrListAction {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search_query.clear();
                    return MrListAction::Refilter;
                }
                KeyCode::Enter => {
                    self.searching = false;
                    return MrListAction::None;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    return MrListAction::Refilter;
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    return MrListAction::Refilter;
                }
                _ => return MrListAction::None,
            }
        }

        let len = self.filtered_indices.len();
        if len == 0 {
            return match key.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                    MrListAction::None
                }
                KeyCode::Char('r') => MrListAction::Refresh,
                _ => MrListAction::None,
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
            return MrListAction::OpenDetail;
        } else {
            match key.code {
                KeyCode::Char('/') => {
                    self.searching = true;
                }
                KeyCode::Char('r') => return MrListAction::Refresh,
                KeyCode::Char('A') => return MrListAction::Approve,
                KeyCode::Char('M') => return MrListAction::Merge,
                KeyCode::Char('x') => return MrListAction::ToggleState,
                KeyCode::Char('l') => return MrListAction::EditLabels,
                KeyCode::Char('a') => return MrListAction::EditAssignee,
                KeyCode::Char('c') => return MrListAction::Comment,
                KeyCode::Char('o') => return MrListAction::OpenBrowser,
                KeyCode::Char('f') => return MrListAction::AddFilter,
                KeyCode::Char('F') => return MrListAction::ClearFilters,
                KeyCode::Char('p') => return MrListAction::PickPreset,
                _ => {}
            }
        }
        MrListAction::None
    }
}

#[derive(Debug, PartialEq)]
pub enum MrListAction {
    None,
    Refilter,
    OpenDetail,
    Refresh,
    Approve,
    Merge,
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
    state: &mut MrListState,
    mrs: &[TrackedMergeRequest],
    conditions: &[FilterCondition],
    filter_focused: bool,
    filter_selected: usize,
) {
    let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);

    components::filter_bar::render(
        frame,
        chunks[0],
        conditions,
        filter_focused,
        filter_selected,
    );

    let rows: Vec<Row> = state
        .filtered_indices
        .iter()
        .map(|&idx| {
            let item = &mrs[idx];
            let source_str = match &item.source {
                ItemSource::Tracking => "TRK".to_string(),
                ItemSource::External(p) => p.rsplit('/').next().unwrap_or(p).to_string(),
            };
            let author = item
                .mr
                .author
                .as_ref()
                .map(|a| a.username.as_str())
                .unwrap_or("-");
            let pipeline = item
                .mr
                .head_pipeline
                .as_ref()
                .map(|p| p.status.as_str())
                .unwrap_or("-");
            let title = if item.mr.draft {
                format!("WIP: {}", item.mr.title)
            } else {
                item.mr.title.clone()
            };
            let approvals = item
                .mr
                .approved_by
                .iter()
                .map(|a| a.user.username.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let age = format_age(&item.mr.updated_at);

            Row::new(vec![
                format!("!{}", item.mr.iid),
                source_str,
                title,
                author.to_string(),
                pipeline.to_string(),
                approvals,
                age,
            ])
            .style(if item.mr.draft {
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
        Constraint::Length(12), // Author
        Constraint::Length(10), // Pipeline
        Constraint::Length(15), // Approvals
        Constraint::Length(8),  // Age
    ];

    let header = Row::new(vec![
        "ID", "Source", "Title", "Author", "Pipeline", "Approved", "Updated",
    ])
    .style(styles::header_style())
    .bottom_margin(1);

    let title = if state.searching {
        format!(" Merge Requests (/{}) ", state.search_query)
    } else {
        format!(" Merge Requests ({}) ", state.filtered_indices.len())
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
