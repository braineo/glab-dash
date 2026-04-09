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
    let mut spans = vec![
        Span::styled(
            format!(" {view_name} "),
            styles::title_style().bg(ratatui::style::Color::Rgb(40, 40, 60)),
        ),
        Span::raw(" "),
        Span::styled(format!("[{team_name}]"), styles::source_tracking_style()),
        Span::raw("  "),
    ];

    if loading {
        spans.push(Span::styled("Loading...", styles::draft_style()));
    } else if let Some(err) = error {
        spans.push(Span::styled(format!("Error: {err}"), styles::error_style()));
    } else {
        spans.push(Span::raw(format!("{item_count} items")));
    }

    // Right-aligned hints
    let hints = " q:quit ?:help i:issues m:mrs h:home ";
    let hints_width = hints.len() as u16;
    let left_width = spans.iter().map(|s| s.width()).sum::<usize>() as u16;
    let padding = area.width.saturating_sub(left_width + hints_width);
    spans.push(Span::raw(" ".repeat(padding as usize)));
    spans.push(Span::styled(hints, styles::help_desc_style()));

    let bar = Paragraph::new(Line::from(spans)).style(styles::status_bar_style());
    frame.render_widget(bar, area);
}
