use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

use crate::ui::styles;

pub fn render(frame: &mut Frame, area: Rect, message: &str) {
    let popup = centered_rect(70, 40, area);
    frame.render_widget(Clear, popup);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("  ✗ Error", styles::error_style())),
        Line::from(""),
    ];

    for line in message.lines() {
        lines.push(Line::from(Span::styled(
            format!("  {line}"),
            styles::overlay_text_style(),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Press any key to dismiss",
        styles::overlay_desc_style(),
    )));

    let block = styles::overlay_block("Error")
        .border_style(Style::default().fg(styles::RED));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
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
