use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::keybindings::{self, BindingGroup};
use crate::ui::styles;

pub fn render(frame: &mut Frame, area: Rect, chain: &[&'static BindingGroup]) {
    let popup = centered_rect(70, 80, area);
    frame.render_widget(Clear, popup);

    let section_style = styles::section_header_style().bg(styles::overlay());
    let mut lines = vec![Line::from("")];

    // Only what can actually fire: `active_bindings` drops any key an earlier
    // group already claimed, so a shadowed row is never advertised.
    for (group, bindings) in keybindings::active_bindings(chain) {
        let shown: Vec<_> = bindings
            .into_iter()
            .filter(|b| b.visible_in_help())
            .collect();
        if shown.is_empty() {
            continue;
        }
        lines.push(Line::from(Span::styled(
            format!(" {} {}", styles::ICON_SECTION, group.title),
            section_style,
        )));
        lines.extend(shown.iter().map(|b| help_line(b.label, b.description)));
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
