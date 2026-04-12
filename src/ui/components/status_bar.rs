use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::styles;

fn format_age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

pub struct StatusBarProps<'a> {
    pub view_name: &'a str,
    pub team_name: &'a str,
    pub item_count: usize,
    pub loading: bool,
    pub loading_msg: &'a str,
    pub error: Option<&'a str>,
    pub last_fetched_at: Option<u64>,
    pub hints: &'a [(&'a str, &'a str)],
}

pub fn render(frame: &mut Frame, area: Rect, props: &StatusBarProps) {
    let view_icon = match props.view_name {
        "Dashboard" => styles::ICON_DASHBOARD,
        "Issues" => styles::ICON_ISSUES,
        "Merge Requests" => styles::ICON_MRS,
        "Planning" => styles::ICON_PLANNING,
        _ => "›",
    };

    let mut spans = vec![
        Span::styled(
            format!(" {view_icon} {} ", props.view_name),
            styles::title_style().bg(styles::HIGHLIGHT),
        ),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
        Span::styled(props.team_name.to_string(), styles::source_tracking_style()),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
    ];

    if props.loading {
        let msg = if props.loading_msg.is_empty() {
            "Loading..."
        } else {
            props.loading_msg
        };
        spans.push(Span::styled(
            format!("{} {msg}", styles::ICON_LOADING),
            styles::draft_style(),
        ));
    } else if let Some(err) = props.error {
        spans.push(Span::styled(
            format!("{} {err}", styles::ICON_CLOSED),
            styles::error_style(),
        ));
    } else {
        spans.push(Span::styled(
            format!("{} items", props.item_count),
            ratatui::style::Style::default().fg(styles::TEXT),
        ));
    }

    if let Some(ts) = props.last_fetched_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = now.saturating_sub(ts);
        spans.push(Span::styled(
            format!(" ({})", format_age(age)),
            styles::help_desc_style(),
        ));
    }

    // Right-aligned hints (dynamic per view)
    let mut hints_spans = Vec::new();
    for (key, desc) in props.hints {
        hints_spans.push(Span::styled(*key, styles::help_key_style()));
        hints_spans.push(Span::styled(format!(":{desc} "), styles::help_desc_style()));
    }
    let hints_width: usize = hints_spans.iter().map(Span::width).sum();
    let left_width: usize = spans.iter().map(Span::width).sum();
    let padding = usize::from(area.width).saturating_sub(left_width + hints_width);
    spans.push(Span::raw(" ".repeat(padding)));
    spans.extend(hints_spans);

    let bar = Paragraph::new(Line::from(spans)).style(styles::status_bar_style());
    frame.render_widget(bar, area);
}
