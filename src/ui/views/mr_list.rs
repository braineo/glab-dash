use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Row, Table, TableState};

use std::collections::HashMap;

use crate::filter::{FilterCondition, matches_mr};
use crate::gitlab::types::TrackedMergeRequest;
use crate::sort::{self, SortSpec};
use crate::ui::{components, keys, styles};

#[derive(Default)]
pub struct MrListState {
    pub table_state: TableState,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub searching: bool,
    pub active_sort: Vec<SortSpec>,
}

impl MrListState {
    pub fn apply_filters(
        &mut self,
        mrs: &[TrackedMergeRequest],
        conditions: &[FilterCondition],
        me: &str,
        team_members: &[String],
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        self.filtered_indices = mrs
            .iter()
            .enumerate()
            .filter(|(_, item)| matches_mr(item, conditions, me, team_members))
            .filter(|(_, item)| {
                if self.search_query.is_empty() {
                    true
                } else {
                    let mut haystack = item.mr.title.to_lowercase();
                    if let Some(a) = &item.mr.author {
                        haystack.push(' ');
                        haystack.push_str(&a.username.to_lowercase());
                    }
                    for a in &item.mr.assignees {
                        haystack.push(' ');
                        haystack.push_str(&a.username.to_lowercase());
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
        sort::sort_mrs(&mut self.filtered_indices, mrs, &self.active_sort, label_orders);

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
                KeyCode::Char('e') => return MrListAction::PickPreset,
                KeyCode::Char('S') => return MrListAction::PickSortPreset,
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
    PickSortPreset,
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut MrListState,
    mrs: &[TrackedMergeRequest],
    conditions: &[FilterCondition],
    filter_focused: bool,
    filter_selected: usize,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let has_selection = state.table_state.selected().is_some();
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(if has_selection { 2 } else { 0 }),
    ])
    .split(area);

    components::filter_bar::render(
        frame,
        chunks[0],
        conditions,
        &state.active_sort,
        filter_focused,
        filter_selected,
    );

    let rows: Vec<Row> = state
        .filtered_indices
        .iter()
        .enumerate()
        .map(|(row_idx, &idx)| {
            let item = &mrs[idx];
            let source_str = {
                let p = &item.project_path;
                p.rsplit('/').next().unwrap_or(p).to_string()
            };
            let author = item
                .mr
                .author
                .as_ref()
                .map(|a| a.username.as_str())
                .unwrap_or("-");

            let pipeline_status = item
                .mr
                .head_pipeline
                .as_ref()
                .map(|p| p.status.as_str())
                .unwrap_or("-");
            let pipeline_icon = match pipeline_status {
                "success" | "passed" => styles::ICON_PIPELINE_OK,
                "failed" => styles::ICON_PIPELINE_FAIL,
                "running" => styles::ICON_PIPELINE_RUN,
                "pending" => styles::ICON_PIPELINE_WAIT,
                _ => " ",
            };

            let title = if item.mr.draft {
                format!("{} {}", styles::ICON_DRAFT, item.mr.title)
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

            let row = Row::new(vec![
                format!("!{}", item.mr.iid),
                source_str,
                title,
                author.to_string(),
                format!("{pipeline_icon} {pipeline_status}"),
                approvals,
                age,
            ]);
            if item.mr.draft {
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
        Constraint::Length(12), // Author
        Constraint::Length(12), // Pipeline
        Constraint::Length(15), // Approvals
        Constraint::Length(8),  // Age
    ];

    let header = Row::new(vec![
        "ID", "Source", "Title", "Author", "Pipeline", "Approved", "Updated",
    ])
    .style(styles::header_style())
    .bottom_margin(1);

    let table_block = if state.searching {
        let title_line = Line::from(vec![
            Span::styled(" MRs /", Style::default().fg(styles::CYAN).add_modifier(ratatui::style::Modifier::BOLD)),
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
            Span::styled(" MRs /", Style::default().fg(styles::CYAN).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(&state.search_query, Style::default().fg(styles::TEXT_BRIGHT).add_modifier(ratatui::style::Modifier::BOLD)),
            Span::styled(" ", Style::default()),
        ]);
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(styles::BORDER))
            .title(title_line)
    } else {
        styles::block("Merge Requests")
    };

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(styles::selected_style())
        .highlight_symbol(styles::ICON_SELECTOR)
        .block(table_block);

    frame.render_stateful_widget(table, chunks[1], &mut state.table_state);

    // Preview pane: show full labels and details of selected MR
    if let Some(item) = state.selected_mr(mrs) {
        let mut spans: Vec<Span> = vec![
            Span::styled(" Labels: ", styles::help_desc_style()),
        ];
        if item.mr.labels.is_empty() {
            spans.push(Span::styled("none", styles::help_desc_style()));
        } else {
            for (i, label) in item.mr.labels.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let color = label_colors.get(label.as_str()).map(|s| s.as_str());
                spans.extend(styles::label_spans(label, color));
            }
        }
        let pipeline_status = item
            .mr
            .head_pipeline
            .as_ref()
            .map(|p| p.status.as_str())
            .unwrap_or("none");
        let preview = Paragraph::new(vec![
            Line::from(spans),
            Line::from(vec![
                Span::styled(" Branch: ", styles::help_desc_style()),
                Span::styled(
                    &item.mr.source_branch,
                    ratatui::style::Style::default().fg(styles::TEAL),
                ),
                Span::styled(
                    format!(" {} ", styles::ICON_ARROW),
                    styles::help_desc_style(),
                ),
                Span::styled(
                    &item.mr.target_branch,
                    ratatui::style::Style::default().fg(styles::TEAL),
                ),
                Span::styled("  Pipeline: ", styles::help_desc_style()),
                Span::styled(pipeline_status, styles::pipeline_style(pipeline_status)),
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
