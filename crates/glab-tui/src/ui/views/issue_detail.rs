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
use glab_core::domain::Issue;

#[derive(Default)]
pub struct IssueDetailState {
    pub id: String,
    pub project: String,
    pub iid: String,
    pub conversation: Conversation,
}

impl IssueDetailState {
    /// Handle keys for the detail view.  The conversation under the cursor is
    /// the detail's domain: moving through it, folding a thread, and drafting a
    /// reply into the one the cursor is on.  Resolving needs the API client, so
    /// it bubbles to the focused item, as does everything else.
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

    pub fn open(&mut self, id: &str, project: &str, iid: &str) {
        self.reset();
        self.id = id.to_string();
        self.project = project.to_string();
        self.iid = iid.to_string();
        self.conversation.loading = true;
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    item: &Issue,
    state: &mut IssueDetailState,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(area);
    render_header(frame, chunks[0], item, ctx);
    conversation::render(
        frame,
        chunks[1],
        &mut state.conversation,
        item.description.as_deref(),
    );
}

/// The item's identity in two filled rows: what it is, then its state and the
/// people and labels on it as chips.  A field with nothing to say is left out
/// rather than printed as "none", which is what keeps this to two rows.
fn render_header(frame: &mut Frame, area: Rect, item: &Issue, ctx: &crate::ui::RenderCtx<'_>) {
    let (icon, text, style) = if let Some(status) = item.status_name() {
        (
            styles::status_icon(status),
            status.to_string(),
            styles::status_style(status),
        )
    } else {
        let icon = match item.state.as_str() {
            "opened" => styles::ICON_OPEN,
            "closed" => styles::ICON_CLOSED,
            _ => " ",
        };
        (icon, item.state.clone(), styles::state_style(&item.state))
    };

    let title = Line::from(vec![
        Span::styled(
            format!(" #{}  ", item.iid),
            Style::default()
                .fg(styles::TEXT_DIM)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            item.title.clone(),
            Style::default()
                .fg(styles::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let mut meta = vec![
        Span::raw(" "),
        Span::styled(format!("{icon} {text}"), style),
    ];
    if let Some(author) = &item.author {
        meta.push(styles::chip_sep());
        meta.push(Span::styled(
            format!("@{}", author.username),
            Style::default().fg(styles::TEXT),
        ));
    }
    let assignees: Vec<&str> = item.assignees.iter().map(|a| a.username.as_str()).collect();
    if !assignees.is_empty() {
        meta.push(Span::styled(
            format!(" {} ", styles::ICON_ARROW),
            styles::help_desc_style(),
        ));
        meta.push(Span::styled(
            format!("@{}", assignees.join(" @")),
            Style::default().fg(styles::TEXT_BRIGHT),
        ));
    }
    meta.push(styles::chip_sep());
    meta.push(Span::styled(
        item.project_path().to_string(),
        styles::help_desc_style(),
    ));
    if !item.labels.is_empty() {
        meta.push(Span::raw(" "));
        for label in &item.labels {
            let color = ctx.label_colors.get(label.as_str()).map(String::as_str);
            meta.extend(styles::label_spans(label, color));
            meta.push(Span::raw(" "));
        }
    }

    frame.render_widget(
        Paragraph::new(vec![title, Line::from(meta)]).style(Style::default().bg(styles::SURFACE)),
        area,
    );
}
