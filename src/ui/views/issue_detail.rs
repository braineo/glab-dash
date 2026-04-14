use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::cmd::EventResult;
use crate::gitlab::types::{Discussion, TrackedIssue};
use crate::keybindings::{self, KeyAction};
use crate::ui::{markdown, styles};

#[derive(Default)]
pub struct IssueDetailState {
    pub project: String,
    pub iid: u64,
    pub scroll: u16,
    pub discussions: Vec<Discussion>,
    pub loading_notes: bool,
}

impl IssueDetailState {
    /// Handle keys for the detail view.  Scroll is the detail's domain;
    /// everything else (item actions, global) bubbles.
    pub fn handle_key(&mut self, key: &crossterm::event::KeyEvent) -> EventResult {
        let Some(action) = keybindings::match_group(keybindings::DETAIL_NAV_BINDINGS, key) else {
            return EventResult::Bubble;
        };
        match action {
            KeyAction::MoveDown => self.scroll_down(),
            KeyAction::MoveUp => self.scroll_up(),
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(3);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(3);
    }

    pub fn reset(&mut self) {
        self.project.clear();
        self.iid = 0;
        self.scroll = 0;
        self.discussions.clear();
        self.loading_notes = false;
    }

    pub fn open(&mut self, project: &str, iid: u64) {
        self.reset();
        self.project = project.to_string();
        self.iid = iid;
        self.loading_notes = true;
    }

    /// Build picker items for thread selection: (discussion_id, display label).
    /// Build thread metadata for the reply picker.
    pub fn thread_picker_items(&self) -> Vec<crate::app::ThreadPickerInfo> {
        self.discussions
            .iter()
            .filter_map(|d| {
                let non_system: Vec<_> = d.notes.iter().filter(|n| !n.system).collect();
                let first = non_system.first()?;
                let reply_count = non_system.len() - 1;
                let (last_author, last_preview) = if reply_count > 0 {
                    let last = non_system.last().unwrap();
                    (
                        Some(last.author.username.clone()),
                        Some(last.body.lines().next().unwrap_or("").to_string()),
                    )
                } else {
                    (None, None)
                };
                Some(crate::app::ThreadPickerInfo {
                    discussion_id: d.id.clone(),
                    author: first.author.username.clone(),
                    preview: first.body.lines().next().unwrap_or("").to_string(),
                    last_author,
                    last_preview,
                    reply_count,
                })
            })
            .collect()
    }

    fn non_system_note_count(&self) -> usize {
        self.discussions
            .iter()
            .flat_map(|d| &d.notes)
            .filter(|n| !n.system)
            .count()
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    item: &TrackedIssue,
    state: &IssueDetailState,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let chunks = Layout::vertical([
        Constraint::Length(5), // Header
        Constraint::Min(1),    // Body + comments
    ])
    .split(area);

    // Header
    let assignees = item
        .issue
        .assignees
        .iter()
        .map(|a| a.username.as_str())
        .collect::<Vec<_>>()
        .join(", ");
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

    let mut labels_line_spans = vec![Span::styled("Labels: ", styles::help_desc_style())];
    if item.issue.labels.is_empty() {
        labels_line_spans.push(Span::styled("none", styles::help_desc_style()));
    } else {
        for (i, label) in item.issue.labels.iter().enumerate() {
            if i > 0 {
                labels_line_spans.push(Span::raw(" "));
            }
            let color = label_colors.get(label.as_str()).map(String::as_str);
            labels_line_spans.extend(styles::label_spans(label, color));
        }
    }
    labels_line_spans.push(Span::raw("  "));
    labels_line_spans.push(Span::styled("Source: ", styles::help_desc_style()));
    labels_line_spans.push(Span::styled(
        item.project_path.clone(),
        Style::default().fg(styles::TEXT),
    ));

    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!("#{} ", item.issue.iid), styles::title_style()),
            Span::styled(&item.issue.title, styles::title_style()),
        ]),
        Line::from(vec![
            Span::styled("Status: ", styles::help_desc_style()),
            Span::styled(
                format!("{state_icon} {state_text}"),
                if item.issue.custom_status.is_some() {
                    styles::status_style(&state_text)
                } else {
                    styles::state_style(&item.issue.state)
                },
            ),
            Span::raw("  "),
            Span::styled("Author: ", styles::help_desc_style()),
            Span::styled(
                item.issue
                    .author
                    .as_ref()
                    .map_or("-", |a| a.username.as_str()),
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
            Span::raw("  "),
            Span::styled("Assignees: ", styles::help_desc_style()),
            Span::styled(
                if assignees.is_empty() {
                    "none".to_string()
                } else {
                    assignees
                },
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
        ]),
        Line::from(labels_line_spans),
        Line::from(vec![Span::styled(
            "  [s]tatus [x]close/reopen [c]omment [r]eply [l]abels [a]ssign [o]pen [Esc]back",
            styles::help_desc_style(),
        )]),
    ];

    let header = Paragraph::new(header_lines)
        .style(Style::default().bg(styles::SURFACE))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(styles::BORDER)),
        );
    frame.render_widget(header, chunks[0]);

    // Body + comments
    let mut body_lines: Vec<Line> = Vec::new();

    // Description
    if let Some(desc) = &item.issue.description {
        body_lines.push(Line::from(Span::styled(
            format!(" {} Description", styles::ICON_SECTION),
            styles::section_header_style(),
        )));
        body_lines.push(Line::from(""));
        body_lines.extend(markdown::render(desc, "  "));
    }

    // Comments (threaded)
    if state.loading_notes {
        body_lines.push(Line::from(Span::styled(
            "\u{27F3} Loading comments...",
            styles::draft_style(),
        )));
    } else if state.non_system_note_count() > 0 {
        body_lines.push(Line::from(Span::styled(
            format!(
                " {} Comments ({})",
                styles::ICON_SECTION,
                state.non_system_note_count()
            ),
            styles::section_header_style(),
        )));
        body_lines.push(Line::from(""));
        render_discussions(&mut body_lines, state);
    }

    let body = Paragraph::new(body_lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((state.scroll, 0));
    frame.render_widget(body, chunks[1]);
}

fn render_discussions(lines: &mut Vec<Line<'_>>, state: &IssueDetailState) {
    for disc in &state.discussions {
        let non_system_notes: Vec<_> = disc.notes.iter().filter(|n| !n.system).collect();
        if non_system_notes.is_empty() {
            continue;
        }
        for (i, note) in non_system_notes.iter().enumerate() {
            let is_reply = i > 0;

            let prefix = if is_reply {
                "  \u{2502}   \u{21B3} "
            } else {
                "  \u{2502} "
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, styles::help_desc_style()),
                Span::styled(
                    format!("@{}", note.author.username),
                    styles::help_key_style(),
                ),
                Span::styled(
                    format!("  {}", note.created_at.format("%Y-%m-%d %H:%M")),
                    styles::help_desc_style(),
                ),
            ]));

            let rendered = markdown::render_comment(&note.body);
            if is_reply {
                for line in rendered {
                    let mut spans = vec![Span::raw("      ".to_string())];
                    spans.extend(line.spans);
                    lines.push(Line::from(spans));
                }
            } else {
                lines.extend(rendered);
            }
            lines.push(Line::from(""));
        }
    }
}
