use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::config::Config;
use crate::gitlab::types::{Iteration, TrackedIssue};
use crate::sort::{self, SortDirection, SortField, SortSpec};
use crate::ui::views::list_model::{ItemList, UserFilter};
use crate::ui::{RenderCtx, components, styles};

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum PlanningLayout {
    ThreeColumn,
    TwoColumn,
}

pub struct PlanningColumn {
    pub list: ItemList<TrackedIssue>,
    pub filter: UserFilter,
}

pub struct PlanningViewState {
    pub focused_column: usize,
    pub columns: [PlanningColumn; 3],
    pub column_visible: [bool; 3],
    pub prev_iteration: Option<Iteration>,
    pub current_iteration: Option<Iteration>,
    pub next_iteration: Option<Iteration>,
    pub layout_mode: PlanningLayout,
}

fn default_planning_filter() -> UserFilter {
    UserFilter {
        sort_specs: vec![
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
        ],
        ..UserFilter::default()
    }
}

impl Default for PlanningColumn {
    fn default() -> Self {
        Self {
            list: ItemList::default(),
            filter: default_planning_filter(),
        }
    }
}

impl Default for PlanningViewState {
    fn default() -> Self {
        Self {
            focused_column: 1, // start on current
            columns: [
                PlanningColumn::default(),
                PlanningColumn::default(),
                PlanningColumn::default(),
            ],
            column_visible: [true, true, true],
            prev_iteration: None,
            current_iteration: None,
            next_iteration: None,
            layout_mode: PlanningLayout::ThreeColumn,
        }
    }
}

impl PlanningViewState {
    pub fn selected_issue<'a>(&self, issues: &'a [TrackedIssue]) -> Option<&'a TrackedIssue> {
        self.columns[self.focused_column].list.selected_item(issues)
    }

    fn visible_columns(&self) -> Vec<usize> {
        (0..3).filter(|&i| self.column_visible[i]).collect()
    }

    pub fn move_focus_left(&mut self) {
        let visible = self.visible_columns();
        if let Some(pos) = visible.iter().position(|&c| c == self.focused_column)
            && pos > 0
        {
            self.focused_column = visible[pos - 1];
        }
    }

    pub fn move_focus_right(&mut self) {
        let visible = self.visible_columns();
        if let Some(pos) = visible.iter().position(|&c| c == self.focused_column)
            && pos + 1 < visible.len()
        {
            self.focused_column = visible[pos + 1];
        }
    }

    pub fn clamp_focus(&mut self) {
        let visible = self.visible_columns();
        if !visible.is_empty() && !visible.contains(&self.focused_column) {
            self.focused_column = visible[visible.len() / 2];
        }
    }

    pub fn toggle_layout(&mut self) {
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

    /// Partition issues into columns based on iteration (prefilter),
    /// then apply each column's fuzzy search and sort.
    pub fn partition_issues(
        &mut self,
        issues: &[TrackedIssue],
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        for col in &mut self.columns {
            col.list.indices.clear();
        }

        let current_id = self.current_iteration.as_ref().map(|i| i.id.as_str());

        // Step 1: prefilter by iteration into columns
        match self.layout_mode {
            PlanningLayout::ThreeColumn => {
                let prev_id = self.prev_iteration.as_ref().map(|i| i.id.as_str());
                let next_id = self.next_iteration.as_ref().map(|i| i.id.as_str());

                for (i, item) in issues.iter().enumerate() {
                    let iter_id = item.issue.iteration.as_ref().map(|it| it.id.as_str());
                    if iter_id == prev_id && prev_id.is_some() {
                        self.columns[0].list.indices.push(i);
                    } else if iter_id == current_id && current_id.is_some() {
                        self.columns[1].list.indices.push(i);
                    } else if iter_id == next_id && next_id.is_some() {
                        self.columns[2].list.indices.push(i);
                    }
                }
            }
            PlanningLayout::TwoColumn => {
                for (i, item) in issues.iter().enumerate() {
                    let iter_id = item.issue.iteration.as_ref().map(|it| it.id.as_str());
                    if iter_id == current_id && current_id.is_some() {
                        self.columns[1].list.indices.push(i);
                    } else {
                        self.columns[0].list.indices.push(i);
                    }
                }
            }
        }

        // Step 2: per-column fuzzy filter and sort
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
                col.filter.fuzzy_matches(&haystack)
            });
            sort::sort_issues(
                &mut col.list.indices,
                issues,
                &col.filter.sort_specs,
                label_orders,
            );
            col.list.clamp_selection();
        }
    }
}

// ── Rendering ──

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut PlanningViewState,
    issues: &[TrackedIssue],
    config: &Config,
    active_team: Option<usize>,
    ctx: &RenderCtx,
) {
    let visible: Vec<usize> = state.visible_columns();
    if visible.is_empty() {
        let msg = Paragraph::new("No columns visible. Press [ or ] to show columns.");
        frame.render_widget(msg, area);
        return;
    }

    // Split into equal-width columns
    let constraints: Vec<Constraint> = visible
        .iter()
        .map(|_| Constraint::Ratio(1, u32::try_from(visible.len()).unwrap_or(u32::MAX)))
        .collect();
    let col_rects = Layout::horizontal(constraints).split(area);

    let team_members = match active_team {
        Some(idx) => config.team_members(idx),
        None => config.all_members(),
    };

    for (vi, &col_idx) in visible.iter().enumerate() {
        let is_focused = col_idx == state.focused_column;
        render_column(
            frame,
            col_rects[vi],
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
    let col = &state.columns[col_idx];
    let title = column_title(state, col_idx);
    let count = col.list.len();
    let total_weight: u32 = col
        .list
        .indices
        .iter()
        .filter_map(|&i| issues.get(i))
        .filter_map(|item| item.issue.weight)
        .sum();
    let header_text = format!("{title}  ({count} issues, {total_weight}w)");

    let border_style = if is_focused {
        Style::default()
            .fg(styles::BLUE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(styles::TEXT_DIM)
    };

    // Use search_block style when column has active search
    let block = if col.filter.is_searching() || col.filter.has_query() {
        let mut spans = vec![Span::styled(
            format!(" {header_text} /"),
            Style::default()
                .fg(styles::CYAN)
                .add_modifier(Modifier::BOLD),
        )];
        spans.push(Span::styled(
            col.filter.fuzzy_query.as_str(),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ));
        if col.filter.is_searching() {
            spans.push(Span::styled("\u{258e}", Style::default().fg(styles::CYAN)));
        }
        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused {
                ratatui::widgets::BorderType::Thick
            } else {
                ratatui::widgets::BorderType::Rounded
            })
            .border_style(if col.filter.is_searching() {
                Style::default().fg(styles::CYAN)
            } else {
                border_style
            })
            .title(Line::from(spans))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_type(if is_focused {
                ratatui::widgets::BorderType::Thick
            } else {
                ratatui::widgets::BorderType::Rounded
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
            ))
    };

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 {
        return;
    }

    // Layout: filter bar (1 line) + table + member stats footer
    let workload = member_stats(&col.list.indices, issues, team_members);
    let stats_height = if workload.is_empty() {
        0
    } else {
        u16::try_from(workload.len())
            .unwrap_or(u16::MAX)
            .min(inner.height.saturating_sub(2))
    };

    let has_filter_bar = !col.filter.conditions.is_empty() || !col.filter.sort_specs.is_empty();
    let filter_bar_height = u16::from(has_filter_bar);

    let parts = Layout::vertical([
        Constraint::Length(filter_bar_height), // filter/sort bar
        Constraint::Min(1),                    // table
        Constraint::Length(stats_height),      // member stats
    ])
    .split(inner);

    // Filter + sort bar
    if has_filter_bar {
        components::filter_bar::render(
            frame,
            parts[0],
            &col.filter.conditions,
            &col.filter.sort_specs,
            col.filter.bar_focused,
            col.filter.bar_selected,
        );
    }

    // Issue table
    let rows: Vec<Row> = col
        .list
        .indices
        .iter()
        .filter_map(|&i| issues.get(i))
        .map(|item| {
            let status_icon = item
                .issue
                .custom_status
                .as_deref()
                .map_or(styles::ICON_UNCHECK, styles::status_icon);
            let iid = format!("#{}", item.issue.iid);
            let assignee = item
                .issue
                .assignees
                .first()
                .map_or("-", |u| u.username.as_str());
            let weight = item
                .issue
                .weight
                .map(|w| format!("{w}w"))
                .unwrap_or_default();

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

    frame.render_stateful_widget(
        table,
        parts[1],
        &mut state.columns[col_idx].list.table_state,
    );

    // Member stats footer
    if stats_height > 0 {
        let stat_lines: Vec<Line> = workload
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
        frame.render_widget(stats_widget, parts[2]);
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
            0 => state.prev_iteration.as_ref().map_or_else(
                || "\u{25c1} Previous".to_string(),
                |i| format!("\u{25c1} {}", iteration_label(i)),
            ),
            1 => state.current_iteration.as_ref().map_or_else(
                || "\u{25cf} Current".to_string(),
                |i| format!("\u{25cf} {}", iteration_label(i)),
            ),
            2 => state.next_iteration.as_ref().map_or_else(
                || "\u{25b7} Next".to_string(),
                |i| format!("\u{25b7} {}", iteration_label(i)),
            ),
            _ => String::new(),
        },
        PlanningLayout::TwoColumn => match col_idx {
            0 => "Other".to_string(),
            1 => state.current_iteration.as_ref().map_or_else(
                || "\u{25cf} Current".to_string(),
                |i| format!("\u{25cf} {}", iteration_label(i)),
            ),
            _ => String::new(),
        },
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
