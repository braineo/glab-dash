use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::gitlab::types::{Note, TrackedMergeRequest};
use crate::ui::{markdown, styles};

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

pub fn render(
    frame: &mut Frame,
    area: Rect,
    item: &TrackedMergeRequest,
    state: &MrDetailState,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let chunks = Layout::vertical([Constraint::Length(7), Constraint::Min(1)]).split(area);

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
        .map_or("none", |p| p.status.as_str());

    let pipeline_icon = match pipeline_status {
        "success" | "passed" => styles::ICON_PIPELINE_OK,
        "failed" => styles::ICON_PIPELINE_FAIL,
        "running" => styles::ICON_PIPELINE_RUN,
        "pending" => styles::ICON_PIPELINE_WAIT,
        _ => " ",
    };

    let state_icon = match item.mr.state.as_str() {
        "opened" => styles::ICON_OPEN,
        "closed" => styles::ICON_CLOSED,
        "merged" => styles::ICON_MERGED,
        _ => " ",
    };

    let title_prefix = if item.mr.draft {
        format!("{} DRAFT: ", styles::ICON_DRAFT)
    } else {
        String::new()
    };
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
            Span::styled(
                format!("{state_icon} {}", item.mr.state),
                styles::state_style(&item.mr.state),
            ),
            Span::raw("  "),
            Span::styled("Pipeline: ", styles::help_desc_style()),
            Span::styled(
                format!("{pipeline_icon} {pipeline_status}"),
                styles::pipeline_style(pipeline_status),
            ),
            Span::raw("  "),
            Span::styled(&item.mr.source_branch, Style::default().fg(styles::TEAL)),
            Span::styled(
                format!(" {} ", styles::ICON_ARROW),
                styles::help_desc_style(),
            ),
            Span::styled(&item.mr.target_branch, Style::default().fg(styles::TEAL)),
        ]),
        Line::from(vec![
            Span::styled("Assignees: ", styles::help_desc_style()),
            Span::styled(
                if assignees.is_empty() {
                    "none".to_string()
                } else {
                    assignees
                },
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
            Span::raw("  "),
            Span::styled("Reviewers: ", styles::help_desc_style()),
            Span::styled(
                if reviewers.is_empty() {
                    "none".to_string()
                } else {
                    reviewers
                },
                Style::default().fg(styles::TEXT_BRIGHT),
            ),
        ]),
        {
            let mut spans = vec![Span::styled("Labels: ", styles::help_desc_style())];
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
            Line::from(spans)
        },
        Line::from(vec![
            Span::styled("Approved by: ", styles::help_desc_style()),
            Span::styled(
                if approved.is_empty() {
                    "none".to_string()
                } else {
                    format!("{} {approved}", styles::ICON_CHECK)
                },
                styles::source_tracking_style(),
            ),
        ]),
        Line::from(vec![Span::styled(
            "  [c]omment [A]pprove [M]erge [x]close [l]abels [a]ssign [o]pen [Esc]back",
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

    if let Some(desc) = &item.mr.description {
        body_lines.push(Line::from(Span::styled(
            format!(" {} Description", styles::ICON_SECTION),
            styles::section_header_style(),
        )));
        body_lines.push(Line::from(""));
        body_lines.extend(markdown::render(desc, "  "));
    }

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
