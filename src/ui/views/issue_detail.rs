use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::gitlab::types::{Note, TrackedIssue};
use crate::ui::styles;

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
    let labels = item.issue.labels.join(", ");
    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!("#{} ", item.issue.iid), styles::title_style()),
            Span::styled(&item.issue.title, styles::title_style()),
        ]),
        Line::from(vec![
            Span::styled("State: ", styles::help_desc_style()),
            Span::styled(&item.issue.state, styles::state_style(&item.issue.state)),
            Span::raw("  "),
            Span::styled("Assignees: ", styles::help_desc_style()),
            Span::raw(if assignees.is_empty() {
                "none"
            } else {
                &assignees
            }),
        ]),
        Line::from(vec![
            Span::styled("Labels: ", styles::help_desc_style()),
            Span::styled(
                if labels.is_empty() {
                    "none".to_string()
                } else {
                    labels
                },
                styles::label_style(),
            ),
            Span::raw("  "),
            Span::styled("Source: ", styles::help_desc_style()),
            Span::raw(item.source.to_string()),
        ]),
        Line::from(vec![Span::styled(
            "  [c]omment [x]close/reopen [l]abels [a]ssign [o]pen [Esc]back",
            styles::help_desc_style(),
        )]),
    ];

    let header = Paragraph::new(header_lines).block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(styles::title_style()),
    );
    frame.render_widget(header, chunks[0]);

    // Body + comments
    let mut body_lines: Vec<Line> = Vec::new();

    // Description
    if let Some(desc) = &item.issue.description {
        body_lines.push(Line::from(Span::styled(
            "── Description ──",
            styles::title_style(),
        )));
        for line in desc.lines() {
            body_lines.push(Line::from(line.to_string()));
        }
        body_lines.push(Line::from(""));
    }

    // Comments
    if state.loading_notes {
        body_lines.push(Line::from(Span::styled(
            "Loading comments...",
            styles::draft_style(),
        )));
    } else if !state.notes.is_empty() {
        body_lines.push(Line::from(Span::styled(
            format!("── Comments ({}) ──", state.notes.len()),
            styles::title_style(),
        )));
        for note in &state.notes {
            if note.system {
                continue;
            }
            body_lines.push(Line::from(vec![
                Span::styled(
                    format!("@{}", note.author.username),
                    styles::help_key_style(),
                ),
                Span::styled(
                    format!("  {}", note.created_at.format("%Y-%m-%d %H:%M")),
                    styles::help_desc_style(),
                ),
            ]));
            for line in note.body.lines() {
                body_lines.push(Line::from(format!("  {line}")));
            }
            body_lines.push(Line::from(""));
        }
    }

    let body = Paragraph::new(body_lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((state.scroll, 0));
    frame.render_widget(body, chunks[1]);
}
