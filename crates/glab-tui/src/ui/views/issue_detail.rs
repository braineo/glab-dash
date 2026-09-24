use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::Overlay;
use crate::binding_group;
use crate::cmd::{Effects, EventResult};
use crate::keybindings::KeyAction;
use crate::ui::components::conversation::{self, Conversation};
use crate::ui::components::detail_body::DetailBody;
use crate::ui::components::related;
use crate::ui::styles;
use crate::ui::views::DetailCtx;
use glab_core::domain::{Issue, Item};
use glab_core::domain::{ItemRef, RelatedItem};

binding_group! {
    /// An issue only, so it sits ahead of the shared conversation group rather
    /// than inside it.
    pub ISSUE_LINK_GROUP: "Linked Issues" {
        ('L') => AddLink | "L" "Link an issue",
        ('d') => RemoveLink | "d" "Unlink (on a link row)",
        (key Enter) => OpenLink | "Enter" "Open the linked item",
    }
}

#[derive(Default)]
pub struct IssueDetailState {
    pub id: String,
    pub project: String,
    pub iid: String,
    /// The rows and the cursor; the sections only fill them.
    pub body: DetailBody,
    pub conversation: Conversation,
}

impl IssueDetailState {
    /// What its keys act on.
    pub fn item(&self) -> ItemRef {
        ItemRef::issue(&self.project, &self.iid)
    }

    /// Offered to the sections in the order they are drawn.
    pub fn handle_key(
        &mut self,
        action: Option<KeyAction>,
        cx: &DetailCtx<'_>,
        overlay: &mut Overlay,
        fx: &mut Effects<'_>,
    ) -> EventResult {
        let Some(action) = action else {
            return EventResult::Bubble;
        };
        if self.body.handle_key(action) {
            return EventResult::Consumed;
        }
        if related::handle_key(action, cx, &self.body, overlay, fx).handled() {
            return EventResult::Consumed;
        }
        self.conversation
            .handle_key(action, &mut self.body, overlay)
    }

    pub fn reset(&mut self) {
        self.project.clear();
        self.iid.clear();
        self.body = DetailBody::default();
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
    related: &[RelatedItem],
    state: &mut IssueDetailState,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(area);
    render_header(frame, chunks[0], item, ctx);

    // What it says, what holds it up, then what was said about it.
    let width = usize::from(chunks[1].width);
    state.body.begin();
    state.body.description(item.description.as_deref(), width);
    related::push(&mut state.body, related, width);
    conversation::push(&mut state.body, &state.conversation, width);
    state.body.render(frame, chunks[1]);
    conversation::render_sticky_head(frame, chunks[1], &state.conversation, &state.body);
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
                .fg(styles::text_dim())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            item.title.clone(),
            Style::default()
                .fg(styles::text_bright())
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
            Style::default().fg(styles::text()),
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
            Style::default().fg(styles::text_bright()),
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
        Paragraph::new(vec![title, Line::from(meta)]).style(Style::default().bg(styles::surface())),
        area,
    );
}
