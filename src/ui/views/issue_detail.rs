use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::gitlab::types::{Note, TrackedIssue};
use crate::ui::{markdown, styles};

#[derive(Default)]
pub struct IssueDetailState {
    pub scroll: u16,
    pub notes: Vec<Note>,
    pub loading_notes: bool,
}

impl IssueDetailState {
    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(3);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(3);
    }

    pub fn reset(&mut self) {
        self.scroll = 0;
        self.notes.clear();
        self.loading_notes = false;
    }
}

pub fn render(frame: &mut Frame, area: Rect, item: &TrackedIssue, state: &IssueDetailState) {
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
                labels_line_spans.push(Span::styled(", ", Style::default().fg(styles::TEXT_DIM)));
            }
            labels_line_spans.extend(styles::label_spans(label));
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
            "  [c]omment [x]close/reopen [l]abels [a]ssign [o]pen [Esc]back",
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

    // Comments
    if state.loading_notes {
        body_lines.push(Line::from(Span::styled(
            "⟳ Loading comments...",
            styles::draft_style(),
        )));
    } else if !state.notes.is_empty() {
        body_lines.push(Line::from(Span::styled(
            format!(" {} Comments ({})", styles::ICON_SECTION, state.notes.len()),
            styles::section_header_style(),
        )));
        body_lines.push(Line::from(""));
        for note in &state.notes {
            if note.system {
                continue;
            }
            body_lines.push(Line::from(vec![
                Span::styled("  │ ", styles::help_desc_style()),
                Span::styled(
                    format!("@{}", note.author.username),
                    styles::help_key_style(),
                ),
                Span::styled(
                    format!("  {}", note.created_at.format("%Y-%m-%d %H:%M")),
                    styles::help_desc_style(),
                ),
            ]));
            body_lines.extend(markdown::render_comment(&note.body));
            body_lines.push(Line::from(""));
        }
    }

    let body = Paragraph::new(body_lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((state.scroll, 0));
    frame.render_widget(body, chunks[1]);
}
