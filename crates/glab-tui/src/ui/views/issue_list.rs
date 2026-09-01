use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use std::collections::HashMap;

use crate::cmd::{Cmd, Dirty, EventResult};
use crate::ui::views::list_model::{self, FilterBarAction, ItemList, UserFilter};
use crate::ui::{components, styles};
use glab_core::domain::Issue;
use glab_core::filter::matches_issue;
use glab_core::sort;

#[derive(Default)]
pub struct IssueListState {
    pub list: ItemList<Issue>,
    pub filter: UserFilter,
}

impl IssueListState {
    // ── Key handling ────────────────────────────────────────────────

    /// Handle keys for the issue list view.
    ///
    /// Delegates to focused children first (filter bar → fuzzy → list nav),
    /// then handles view-level keys (start search).  Bubbles everything
    /// else (item actions, global nav) to the parent.
    pub fn handle_key(
        &mut self,
        key: &KeyEvent,
        dirty: &mut Dirty,
        cmds: &mut Vec<Cmd>,
        needs_redraw: &mut bool,
    ) -> EventResult {
        // 1. Filter bar owns its keys when focused
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

        // 2. Fuzzy search owns its keys when active
        if self.filter.is_searching() {
            let is_exit = matches!(key.code, KeyCode::Enter | KeyCode::Esc);
            if self.filter.handle_fuzzy_input(key) == Some(true) {
                dirty.view_state = true;
            }
            if is_exit {
                cmds.push(Cmd::PersistViewState);
            }
            dirty.selection = true;
            return EventResult::Consumed;
        }

        // 3. List owns navigation (j/k/g/G/pgup/pgdn)
        if let Some(moved) = self.list.handle_nav_key(key) {
            if moved {
                dirty.selection = true;
            } else {
                *needs_redraw = false;
            }
            return EventResult::Consumed;
        }

        // 4. View-level: start search (only the view knows which filter to activate)
        if key.code == KeyCode::Char('/') {
            self.filter.start_search();
            dirty.selection = true;
            return EventResult::Consumed;
        }

        // Everything else (item actions, filter menu, global nav) bubbles
        EventResult::Bubble
    }

    // ── Filtering ───────────────────────────────────────────────────

    pub fn apply_filters(
        &mut self,
        issues: &[Issue],
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
                    || item.assignees.is_empty()
                    || item
                        .assignees
                        .iter()
                        .any(|a| team_members.contains(&a.username))
            })
            .filter(|(_, item)| matches_issue(item, &self.filter.conditions, me, team_members))
            .filter(|(_, item)| {
                let mut haystack = item.title.to_lowercase();
                for a in &item.assignees {
                    haystack.push(' ');
                    haystack.push_str(&a.username.to_lowercase());
                }
                for l in &item.labels {
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

    pub fn selected_issue<'a>(&self, issues: &'a [Issue]) -> Option<&'a Issue> {
        self.list.selected_item(issues)
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut IssueListState,
    issues: &[Issue],
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
                let p = item.project_path();
                let short = p.rsplit('/').next().unwrap_or(p);
                Span::styled(short.to_string(), styles::source_external_style())
            };
            let assignees = item
                .assignees
                .iter()
                .map(|a| a.username.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let author = item.author.as_ref().map_or("-", |a| a.username.as_str());
            let labels = styles::labels_compact(&item.labels, 30, label_colors);
            let age = list_model::format_age(&item.updated_at, now);

            // Show custom status if available, otherwise fall back to state
            let (state_icon, state_text) = if let Some(status) = item.status_name() {
                (styles::status_icon(status), status.to_string())
            } else {
                let icon = match item.state.as_str() {
                    "opened" => styles::ICON_OPEN,
                    "closed" => styles::ICON_CLOSED,
                    _ => " ",
                };
                (icon, item.state.clone())
            };

            let state_style = if item.status_name().is_some() {
                styles::status_style(&state_text)
            } else {
                styles::state_style(&item.state)
            };

            let row = Row::new([
                Cell::from(Span::styled(
                    format!("#{}", item.iid),
                    Style::default().fg(styles::TEXT_DIM),
                )),
                Cell::from(Span::styled(
                    source_span.to_string(),
                    styles::source_external_style(),
                )),
                Cell::from(item.title.clone()),
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
            let is_closed = item.state == "closed";
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
        if item.labels.is_empty() {
            spans.push(Span::styled("none", styles::help_desc_style()));
        } else {
            for (i, label) in item.labels.iter().enumerate() {
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
                    item.assignees
                        .iter()
                        .map(|a| a.username.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    ratatui::style::Style::default().fg(styles::TEXT_BRIGHT),
                ),
                Span::styled("  Source: ", styles::help_desc_style()),
                Span::styled(
                    item.project_path().to_string(),
                    ratatui::style::Style::default().fg(styles::TEXT),
                ),
            ]),
        ])
        .style(ratatui::style::Style::default().bg(styles::SURFACE));
        frame.render_widget(preview, chunks[2]);
    }
}
