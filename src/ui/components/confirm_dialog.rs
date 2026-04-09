use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::ui::styles;

pub fn render(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    let popup = centered_rect(50, 20, area);
    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {message}"),
            styles::overlay_text_style(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", styles::overlay_key_style()),
            Span::styled(": confirm  ", styles::overlay_desc_style()),
            Span::styled("n/Esc", styles::overlay_key_style()),
            Span::styled(": cancel", styles::overlay_desc_style()),
        ]),
    ];

    let block = styles::overlay_block(title)
        .border_style(Style::default().fg(styles::ORANGE));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}
