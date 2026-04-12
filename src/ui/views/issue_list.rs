use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use std::collections::HashMap;

use crate::filter::matches_issue;
use crate::gitlab::types::TrackedIssue;
use crate::sort;
use crate::ui::views::list_model::{self, ItemList, UserFilter};
use crate::ui::{components, styles};

#[derive(Default)]
pub struct IssueListState {
    pub list: ItemList<TrackedIssue>,
    pub filter: UserFilter,
}

impl IssueListState {
    pub fn apply_filters(
        &mut self,
        issues: &[TrackedIssue],
        me: &str,
        team_members: &[String],
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        self.list.indices = issues
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                // Implicit team filter: when a team is selected, only show items
                // assigned to team members or unassigned items.
                team_members.is_empty()
                    || item.issue.assignees.is_empty()
                    || item
                        .issue
                        .assignees
                        .iter()
                        .any(|a| team_members.contains(&a.username))
            })
            .filter(|(_, item)| matches_issue(item, &self.filter.conditions, me, team_members))
            .filter(|(_, item)| {
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
            })
            .map(|(i, _)| i)
            .collect();

        sort::sort_issues(
            &mut self.list.indices,
            issues,
            &self.filter.sort_specs,
            label_orders,
        );

        self.list.clamp_selection();
    }

    pub fn selected_issue<'a>(&self, issues: &'a [TrackedIssue]) -> Option<&'a TrackedIssue> {
        self.list.selected_item(issues)
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut IssueListState,
    issues: &[TrackedIssue],
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let has_selection = state.list.table_state.selected().is_some();
    let chunks = Layout::vertical([
        Constraint::Length(1),                                 // Filter bar
        Constraint::Min(1),                                    // Table
        Constraint::Length(if has_selection { 2 } else { 0 }), // Preview
    ])
    .split(area);

    // Filter + sort bar
    components::filter_bar::render(
        frame,
        chunks[0],
        &state.filter.conditions,
        &state.filter.sort_specs,
        state.filter.bar_focused,
        state.filter.bar_selected,
    );

    // Build table rows
    let now = chrono::Utc::now();
    let selected_idx = state.list.table_state.selected();
    let rows: Vec<Row> = state
        .list
        .indices
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
            let author = item
                .issue
                .author
                .as_ref()
                .map_or("-", |a| a.username.as_str());
            let labels = styles::labels_compact(&item.issue.labels, 30, label_colors);
            let age = list_model::format_age(&item.issue.updated_at, now);

            // Show custom status if available, otherwise fall back to state
            let (state_icon, state_text) = if let Some(ref status) = item.issue.custom_status {
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
                Cell::from(Span::styled(
                    format!("#{}", item.issue.iid),
                    Style::default().fg(styles::TEXT_DIM),
                )),
                Cell::from(Span::styled(
                    source_span.to_string(),
                    styles::source_external_style(),
                )),
                Cell::from(item.issue.title.clone()),
                Cell::from(Line::from(Span::styled(
                    format!("{state_icon} {state_text}"),
                    state_style,
                ))),
                Cell::from(Span::styled(
                    author.to_string(),
                    Style::default().fg(styles::CYAN),
                )),
                Cell::from(Span::styled(
                    assignees,
                    Style::default().fg(styles::MAGENTA),
                )),
                Cell::from(labels),
                Cell::from(Span::styled(age, Style::default().fg(styles::TEXT_DIM))),
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
        Constraint::Length(12), // Author
        Constraint::Length(15), // Assignees
        Constraint::Length(32), // Labels
        Constraint::Length(8),  // Age
    ];

    let header = Row::new(vec![
        "ID", "Source", "Title", "State", "Author", "Assignee", "Labels", "Updated",
    ])
    .style(styles::header_style())
    .bottom_margin(1);

    let table_block = list_model::search_block("Issues", &state.filter);

    let table = Table::new(rows, widths)
        .header(header)
        .highlight_symbol(styles::ICON_SELECTOR)
        .block(table_block);

    frame.render_stateful_widget(table, chunks[1], &mut state.list.table_state);

    // Preview pane: show full labels of selected item
    if let Some(item) = state.list.selected_item(issues) {
        let mut spans: Vec<Span> = vec![Span::styled(" Labels: ", styles::help_desc_style())];
        if item.issue.labels.is_empty() {
            spans.push(Span::styled("none", styles::help_desc_style()));
        } else {
            for (i, label) in item.issue.labels.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let color = label_colors.get(label.as_str()).map(String::as_str);
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
