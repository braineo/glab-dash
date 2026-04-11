use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row, Table};

use std::collections::HashMap;

use crate::filter::matches_mr;
use crate::gitlab::types::TrackedMergeRequest;
use crate::sort;
use crate::ui::views::list_model::{self, ItemList, UserFilter};
use crate::ui::{components, styles};

#[derive(Default)]
pub struct MrListState {
    pub list: ItemList<TrackedMergeRequest>,
    pub filter: UserFilter,
}

impl MrListState {
    pub fn apply_filters(
        &mut self,
        mrs: &[TrackedMergeRequest],
        me: &str,
        team_members: &[String],
        label_orders: &HashMap<String, Vec<String>>,
    ) {
        self.list.indices = mrs
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                // Implicit team filter: when a team is selected, only show items
                // assigned to team members or unassigned items.
                team_members.is_empty()
                    || item.mr.assignees.is_empty()
                    || item
                        .mr
                        .assignees
                        .iter()
                        .any(|a| team_members.contains(&a.username))
            })
            .filter(|(_, item)| matches_mr(item, &self.filter.conditions, me, team_members))
            .filter(|(_, item)| {
                let mut haystack = item.mr.title.to_lowercase();
                if let Some(a) = &item.mr.author {
                    haystack.push(' ');
                    haystack.push_str(&a.username.to_lowercase());
                }
                for a in &item.mr.assignees {
                    haystack.push(' ');
                    haystack.push_str(&a.username.to_lowercase());
                }
                self.filter.fuzzy_matches(&haystack)
            })
            .map(|(i, _)| i)
            .collect();

        sort::sort_mrs(
            &mut self.list.indices,
            mrs,
            &self.filter.sort_specs,
            label_orders,
        );

        self.list.clamp_selection();
    }

    pub fn selected_mr<'a>(
        &self,
        mrs: &'a [TrackedMergeRequest],
    ) -> Option<&'a TrackedMergeRequest> {
        self.list.selected_item(mrs)
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut MrListState,
    mrs: &[TrackedMergeRequest],
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let has_selection = state.list.table_state.selected().is_some();
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(if has_selection { 2 } else { 0 }),
    ])
    .split(area);

    components::filter_bar::render(
        frame,
        chunks[0],
        &state.filter.conditions,
        &state.filter.sort_specs,
        state.filter.bar_focused,
        state.filter.bar_selected,
    );

    let rows: Vec<Row> = state
        .list
        .indices
        .iter()
        .enumerate()
        .map(|(row_idx, &idx)| {
            let item = &mrs[idx];
            let source_str = {
                let p = &item.project_path;
                p.rsplit('/').next().unwrap_or(p).to_string()
            };
            let author = item.mr.author.as_ref().map_or("-", |a| a.username.as_str());

            let pipeline_status = item
                .mr
                .head_pipeline
                .as_ref()
                .map_or("-", |p| p.status.as_str());
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
            let age = list_model::format_age(&item.mr.updated_at);

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

    let table_block = list_model::search_block("Merge Requests", &state.filter);

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(styles::selected_style())
        .highlight_symbol(styles::ICON_SELECTOR)
        .block(table_block);

    frame.render_stateful_widget(table, chunks[1], &mut state.list.table_state);

    // Preview pane: show full labels and details of selected MR
    if let Some(item) = state.list.selected_item(mrs) {
        let mut spans: Vec<Span> = vec![Span::styled(" Labels: ", styles::help_desc_style())];
        if item.mr.labels.is_empty() {
            spans.push(Span::styled("none", styles::help_desc_style()));
        } else {
            for (i, label) in item.mr.labels.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                let color = label_colors.get(label.as_str()).map(String::as_str);
                spans.extend(styles::label_spans(label, color));
            }
        }
        let pipeline_status = item
            .mr
            .head_pipeline
            .as_ref()
            .map_or("none", |p| p.status.as_str());
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
