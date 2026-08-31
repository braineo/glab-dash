use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::View;
use crate::keybindings;
use crate::ui::styles;

pub fn render(frame: &mut Frame, area: Rect, view: View) {
    let popup = centered_rect(70, 80, area);
    frame.render_widget(Clear, popup);

    let groups = keybindings::binding_groups_for_view(view);

    let section_style = styles::section_header_style().bg(styles::OVERLAY);
    let mut lines = vec![Line::from("")];

    for group in groups {
        lines.push(Line::from(Span::styled(
            format!(" {} {}", group.icon, group.title),
            section_style,
        )));
        for binding in group.bindings {
            if binding.visible_in_help() {
                lines.push(help_line(binding.label, binding.description));
            }
        }
        lines.push(Line::from(""));
    }

    let block = styles::overlay_block("Help  ?:close");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}

fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:>12}"), styles::overlay_key_style()),
        Span::styled("  ·  ", styles::overlay_desc_style()),
        Span::styled(desc, styles::overlay_desc_style()),
    ])
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
