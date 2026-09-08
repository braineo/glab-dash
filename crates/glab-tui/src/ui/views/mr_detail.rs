use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::Overlay;
use crate::cmd::EventResult;
use crate::keybindings::KeyAction;
use crate::ui::components::conversation::{self, Conversation};
use crate::ui::styles;
use glab_core::domain::MergeRequest;

#[derive(Default)]
pub struct MrDetailState {
    pub project: String,
    pub iid: String,
    pub conversation: Conversation,
}

impl MrDetailState {
    /// Handle keys for the detail view.  See
    /// [`IssueDetailState::handle_key`](super::issue_detail::IssueDetailState::handle_key)
    /// — a merge request's conversation answers to exactly the same keys.
    pub fn handle_key(&mut self, action: Option<KeyAction>, overlay: &mut Overlay) -> EventResult {
        let Some(action) = action else {
            return EventResult::Bubble;
        };
        match action {
            KeyAction::MoveDown => self.conversation.move_down(),
            KeyAction::MoveUp => self.conversation.move_up(),
            KeyAction::Top => self.conversation.move_top(),
            KeyAction::Bottom => self.conversation.move_bottom(),
            KeyAction::PageDown => self.conversation.page_down(),
            KeyAction::PageUp => self.conversation.page_up(),
            KeyAction::NextUnresolved => self.conversation.move_unresolved(true),
            KeyAction::PrevUnresolved => self.conversation.move_unresolved(false),
            KeyAction::ToggleThread => self.conversation.toggle_fold(),
            KeyAction::ReplyThread => *overlay = conversation::draft_reply(&self.conversation),
            KeyAction::NewThread => *overlay = conversation::draft_new_thread(),
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    pub fn reset(&mut self) {
        self.project.clear();
        self.iid.clear();
        self.conversation.reset();
    }

    pub fn open(&mut self, project: &str, iid: &str) {
        self.reset();
        self.project = project.to_string();
        self.iid = iid.to_string();
        self.conversation.loading = true;
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    item: &MergeRequest,
    state: &mut MrDetailState,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    render_header(frame, chunks[0], item, ctx);
    conversation::render(
        frame,
        chunks[1],
        &mut state.conversation,
        item.description.as_deref(),
    );
}

/// Three filled rows: what it is, where it stands, and who is on it.  A merge
/// request earns the extra row over an issue's two — its pipeline, its branches
/// and its approvals are all things a reviewer scans before reading a word.
fn render_header(
    frame: &mut Frame,
    area: Rect,
    item: &MergeRequest,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let pipeline = item.pipeline_status().unwrap_or("none");
    let pipeline_icon = match pipeline {
        "success" | "passed" => styles::ICON_PIPELINE_OK,
        "failed" => styles::ICON_PIPELINE_FAIL,
        "running" => styles::ICON_PIPELINE_RUN,
        "pending" => styles::ICON_PIPELINE_WAIT,
        _ => " ",
    };
    let state_icon = match item.state.as_str() {
        "opened" => styles::ICON_OPEN,
        "closed" => styles::ICON_CLOSED,
        "merged" => styles::ICON_MERGED,
        _ => " ",
    };

    let mut title = vec![Span::styled(
        format!(" !{}  ", item.iid),
        Style::default()
            .fg(styles::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    )];
    if item.draft {
        title.push(Span::styled(
            format!("{} DRAFT ", styles::ICON_DRAFT),
            styles::draft_style(),
        ));
    }
    title.push(Span::styled(
        item.title.clone(),
        if item.draft {
            styles::draft_style()
        } else {
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD)
        },
    ));

    let mut status = vec![
        Span::raw(" "),
        Span::styled(
            format!("{state_icon} {}", item.state),
            styles::state_style(&item.state),
        ),
        styles::chip_sep(),
        Span::styled(
            format!("{pipeline_icon} {pipeline}"),
            styles::pipeline_style(pipeline),
        ),
        styles::chip_sep(),
        Span::styled(
            item.source_branch.clone(),
            Style::default().fg(styles::TEAL),
        ),
        Span::styled(
            format!(" {} ", styles::ICON_ARROW),
            styles::help_desc_style(),
        ),
        Span::styled(
            item.target_branch.clone(),
            Style::default().fg(styles::TEAL),
        ),
    ];
    let approved: Vec<&str> = item
        .approved_by
        .iter()
        .map(|a| a.username.as_str())
        .collect();
    if !approved.is_empty() {
        status.push(styles::chip_sep());
        status.push(Span::styled(
            format!("{} @{}", styles::ICON_CHECK, approved.join(" @")),
            styles::source_tracking_style(),
        ));
    }

    let mut people = vec![Span::raw(" ")];
    let assignees: Vec<&str> = item.assignees.iter().map(|a| a.username.as_str()).collect();
    let reviewers: Vec<&str> = item.reviewers.iter().map(|r| r.username.as_str()).collect();
    if assignees.is_empty() && reviewers.is_empty() {
        people.push(Span::styled("unassigned", styles::help_desc_style()));
    } else {
        if !assignees.is_empty() {
            people.push(Span::styled(
                format!("@{}", assignees.join(" @")),
                Style::default().fg(styles::TEXT_BRIGHT),
            ));
        }
        if !reviewers.is_empty() {
            if !assignees.is_empty() {
                people.push(styles::chip_sep());
            }
            people.push(Span::styled("review ", styles::help_desc_style()));
            people.push(Span::styled(
                format!("@{}", reviewers.join(" @")),
                Style::default().fg(styles::TEXT_BRIGHT),
            ));
        }
    }
    if !item.labels.is_empty() {
        people.push(Span::raw("  "));
        for label in &item.labels {
            let color = ctx.label_colors.get(label.as_str()).map(String::as_str);
            people.extend(styles::label_spans(label, color));
            people.push(Span::raw(" "));
        }
    }

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(title),
            Line::from(status),
            Line::from(people),
        ])
        .style(Style::default().bg(styles::SURFACE)),
        area,
    );
}
