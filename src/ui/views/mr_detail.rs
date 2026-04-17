use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::app::{Overlay, ThreadPickerInfo};
use crate::cmd::EventResult;
use crate::gitlab::types::{Discussion, TrackedMergeRequest};
use crate::keybindings::{self, KeyAction};
use crate::ui::components::input::CommentInput;
use crate::ui::components::picker;
use crate::ui::views::issue_detail::build_thread_picker_display;
use crate::ui::{markdown, styles};

#[derive(Default)]
pub struct MrDetailState {
    pub project: String,
    pub iid: u64,
    pub scroll: u16,
    pub discussions: Vec<Discussion>,
    pub loading_notes: bool,
}

impl MrDetailState {
    pub fn handle_key(
        &mut self,
        key: &crossterm::event::KeyEvent,
        overlay: &mut Overlay,
    ) -> EventResult {
        let Some(action) = keybindings::match_group(keybindings::DETAIL_NAV_BINDINGS, key) else {
            return EventResult::Bubble;
        };
        match action {
            KeyAction::MoveDown => self.scroll_down(),
            KeyAction::MoveUp => self.scroll_up(),
            KeyAction::ReplyThread => {
                self.start_reply(overlay);
            }
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    fn start_reply(&self, overlay: &mut Overlay) {
        let infos = self.thread_picker_items();
        if infos.is_empty() {
            // No threads yet — fall back to new comment.
            *overlay = Overlay::CommentInput {
                input: CommentInput::default(),
                autocomplete: Box::default(),
                reply_discussion_id: None,
            };
            return;
        }
        let (labels, subtitles) = build_thread_picker_display(&infos);
        *overlay = Overlay::Picker {
            state: picker::PickerState::new("Reply to thread", labels.clone(), false)
                .with_subtitles(subtitles),
            on_complete: Box::new(move |values, app| {
                if let Some(picked_label) = values.first()
                    && let Some(idx) = labels.iter().position(|item| item == picked_label)
                    && let Some(info) = infos.get(idx)
                {
                    app.ui.overlay = Overlay::CommentInput {
                        input: CommentInput::default(),
                        autocomplete: Box::default(),
                        reply_discussion_id: Some(info.discussion_id.clone()),
                    };
                }
            }),
        };
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
    pub fn thread_picker_items(&self) -> Vec<ThreadPickerInfo> {
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
                Some(ThreadPickerInfo {
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
            "  [c]omment [r]eply [A]pprove [M]erge [x]close [l]abels [a]ssign [o]pen [Esc]back",
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

fn render_discussions(lines: &mut Vec<Line<'_>>, state: &MrDetailState) {
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
