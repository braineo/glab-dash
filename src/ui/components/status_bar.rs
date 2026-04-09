use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::styles;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    view_name: &str,
    team_name: &str,
    item_count: usize,
    loading: bool,
    error: Option<&str>,
) {
    let view_icon = match view_name {
        "Dashboard" => "◈",
        "Issues" => "●",
        "Merge Requests" => "⑂",
        _ => "›",
    };

    let mut spans = vec![
        Span::styled(
            format!(" {view_icon} {view_name} "),
            styles::title_style().bg(styles::HIGHLIGHT),
        ),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
        Span::styled(format!("{team_name}"), styles::source_tracking_style()),
        Span::styled(styles::ICON_SEPARATOR, styles::help_desc_style()),
    ];

    if loading {
        spans.push(Span::styled("⟳ Loading...", styles::draft_style()));
    } else if let Some(err) = error {
        spans.push(Span::styled(format!("✗ {err}"), styles::error_style()));
    } else {
        spans.push(Span::styled(
            format!("{item_count} items"),
            ratatui::style::Style::default().fg(styles::TEXT),
        ));
    }

    // Right-aligned hints
    let hints_spans = vec![
        Span::styled("q", styles::help_key_style()),
        Span::styled(":quit ", styles::help_desc_style()),
        Span::styled("?", styles::help_key_style()),
        Span::styled(":help ", styles::help_desc_style()),
        Span::styled("i", styles::help_key_style()),
        Span::styled(":issues ", styles::help_desc_style()),
        Span::styled("m", styles::help_key_style()),
        Span::styled(":mrs ", styles::help_desc_style()),
        Span::styled("h", styles::help_key_style()),
        Span::styled(":home ", styles::help_desc_style()),
    ];
    let hints_width: usize = hints_spans.iter().map(|s| s.width()).sum();
    let left_width: usize = spans.iter().map(|s| s.width()).sum();
    let padding = (area.width as usize).saturating_sub(left_width + hints_width);
    spans.push(Span::raw(" ".repeat(padding)));
    spans.extend(hints_spans);

    let bar = Paragraph::new(Line::from(spans)).style(styles::status_bar_style());
    frame.render_widget(bar, area);
}
