use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::View;
use crate::ui::styles;

pub fn render(frame: &mut Frame, area: Rect, view: &View) {
    let popup = centered_rect(70, 80, area);
    frame.render_widget(Clear, popup);

    let section_style = styles::section_header_style().bg(styles::OVERLAY);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" {} Global", styles::ICON_SECTION),
            section_style,
        )),
        help_line("q", "Back / Quit"),
        help_line("?", "Toggle help"),
        help_line("Esc", "Go back / close overlay"),
        help_line("1-9", "Switch team"),
        help_line("h", "Dashboard (home)"),
        help_line("i", "Go to issues"),
        help_line("m", "Go to merge requests"),
        help_line("p", "Go to planning"),
        Line::from(""),
    ];

    let is_list = matches!(view, View::IssueList | View::MrList);
    let is_issue = matches!(view, View::IssueList | View::IssueDetail | View::Planning);
    let is_mr = matches!(view, View::MrList | View::MrDetail);
    let is_detail = matches!(view, View::IssueDetail | View::MrDetail);
    let is_planning = matches!(view, View::Planning);

    if is_list {
        lines.extend([
            Line::from(Span::styled(
                format!(" {} List Navigation", styles::ICON_SECTION),
                section_style,
            )),
            help_line("j/k", "Move down/up"),
            help_line("g/G", "Jump to top/bottom"),
            help_line("Ctrl+d/u", "Page down/up"),
            help_line("Enter", "Open detail"),
            help_line("/", "Fuzzy search"),
            help_line("r", "Refresh data"),
            help_line("o", "Open in browser"),
            Line::from(""),
            Line::from(Span::styled(
                format!(" {} Filtering", styles::ICON_SECTION),
                section_style,
            )),
            help_line("f", "Add filter condition"),
            help_line("F", "Clear all filters"),
            help_line("e", "Pick saved preset"),
            help_line("S", "Pick sort preset"),
            help_line("Tab", "Focus filter bar"),
            Line::from(""),
        ]);
    }

    if is_detail {
        lines.extend([
            Line::from(Span::styled(
                format!(" {} Detail", styles::ICON_SECTION),
                section_style,
            )),
            help_line("j/k", "Scroll down/up"),
            help_line("o", "Open in browser"),
            Line::from(""),
        ]);
    }

    if is_issue {
        lines.extend([
            Line::from(Span::styled(
                format!(" {} Issue Actions", styles::ICON_SECTION),
                section_style,
            )),
            help_line("s", "Set status"),
            help_line("x", "Close / Reopen"),
            help_line("l", "Set labels"),
            help_line("a", "Set assignee"),
            help_line("c", "Add comment"),
            Line::from(""),
        ]);
    }

    if is_planning {
        lines.extend([
            Line::from(Span::styled(
                format!(" {} Planning Navigation", styles::ICON_SECTION),
                section_style,
            )),
            help_line("h/l", "Move between columns"),
            help_line("j/k", "Move within column"),
            help_line("Enter", "Open issue detail"),
            help_line(">", "Move issue to next iteration"),
            help_line("<", "Move issue to previous iteration"),
            help_line("[/]", "Toggle prev/next column"),
            help_line("v", "Toggle 3-col / 2-col layout"),
            help_line("H", "Dashboard (home)"),
            help_line("/", "Search"),
            help_line("r", "Refresh"),
            Line::from(""),
        ]);
    }

    if is_mr {
        lines.extend([
            Line::from(Span::styled(
                format!(" {} MR Actions", styles::ICON_SECTION),
                section_style,
            )),
            help_line("A", "Approve MR"),
            help_line("M", "Merge MR"),
            help_line("x", "Close MR"),
            help_line("l", "Set labels"),
            help_line("a", "Set assignee"),
            help_line("c", "Add comment"),
            Line::from(""),
        ]);
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
