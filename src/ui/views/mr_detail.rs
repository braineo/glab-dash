use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::gitlab::types::{Note, TrackedMergeRequest};
use crate::ui::styles;

#[derive(Default)]
pub struct MrDetailState {
    pub scroll: u16,
    pub notes: Vec<Note>,
    pub loading_notes: bool,
}

impl MrDetailState {
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

pub fn render(frame: &mut Frame, area: Rect, item: &TrackedMergeRequest, state: &MrDetailState) {
    let chunks = Layout::vertical([Constraint::Length(6), Constraint::Min(1)]).split(area);

    // Header
    let assignees = item
        .mr
        .assignees
        .iter()
        .map(|a| a.username.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let reviewers = item
        .mr
        .reviewers
        .iter()
        .map(|r| r.username.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let approved = item
        .mr
        .approved_by
        .iter()
        .map(|a| a.user.username.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let pipeline_status = item
        .mr
        .head_pipeline
        .as_ref()
        .map(|p| p.status.as_str())
        .unwrap_or("none");

    let title_prefix = if item.mr.draft { "DRAFT: " } else { "" };
    let header_lines = vec![
        Line::from(vec![
            Span::styled(format!("!{} ", item.mr.iid), styles::title_style()),
            Span::styled(
                format!("{title_prefix}{}", item.mr.title),
                if item.mr.draft {
                    styles::draft_style()
                } else {
                    styles::title_style()
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("State: ", styles::help_desc_style()),
            Span::styled(&item.mr.state, styles::state_style(&item.mr.state)),
            Span::raw("  "),
            Span::styled("Pipeline: ", styles::help_desc_style()),
            Span::styled(pipeline_status, styles::pipeline_style(pipeline_status)),
            Span::raw("  "),
            Span::styled(
                format!("{} → {}", item.mr.source_branch, item.mr.target_branch),
                styles::help_desc_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Assignees: ", styles::help_desc_style()),
            Span::raw(if assignees.is_empty() {
                "none"
            } else {
                &assignees
            }),
            Span::raw("  "),
            Span::styled("Reviewers: ", styles::help_desc_style()),
            Span::raw(if reviewers.is_empty() {
                "none"
            } else {
                &reviewers
            }),
        ]),
        Line::from(vec![
            Span::styled("Approved by: ", styles::help_desc_style()),
            Span::styled(
                if approved.is_empty() {
                    "none".to_string()
                } else {
                    approved
                },
                styles::source_tracking_style(),
            ),
        ]),
        Line::from(vec![Span::styled(
            "  [c]omment [A]pprove [M]erge [x]close [l]abels [a]ssign [o]pen [Esc]back",
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

    if let Some(desc) = &item.mr.description {
        body_lines.push(Line::from(Span::styled(
            "── Description ──",
            styles::title_style(),
        )));
        for line in desc.lines() {
            body_lines.push(Line::from(line.to_string()));
        }
        body_lines.push(Line::from(""));
    }

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
