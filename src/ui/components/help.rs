use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::styles;

pub fn render(frame: &mut Frame, area: Rect, view_context: &str) {
    // Center the help overlay
    let popup = centered_rect(70, 80, area);
    frame.render_widget(Clear, popup);

    let mut lines = vec![
        Line::from(Span::styled(" Keyboard Shortcuts ", styles::title_style())),
        Line::from(""),
        Line::from(Span::styled("── Global ──", styles::title_style())),
        help_line("q", "Quit"),
        help_line("?", "Toggle help"),
        help_line("Esc", "Go back / close overlay"),
        help_line("1-9", "Switch team"),
        help_line("i", "Go to issues"),
        help_line("m", "Go to merge requests"),
        help_line("h", "Dashboard (home)"),
        Line::from(""),
    ];

    if view_context == "list" || view_context == "all" {
        lines.extend([
            Line::from(Span::styled("── List Navigation ──", styles::title_style())),
            help_line("j/k", "Move down/up"),
            help_line("g/G", "Jump to top/bottom"),
            help_line("Ctrl+d/u", "Page down/up"),
            help_line("Enter", "Open detail"),
            help_line("/", "Fuzzy search"),
            help_line("r", "Refresh data"),
            help_line("o", "Open in browser"),
            Line::from(""),
            Line::from(Span::styled("── Filtering ──", styles::title_style())),
            help_line("f", "Add filter condition"),
            help_line("F", "Clear all filters"),
            help_line("p", "Pick saved preset"),
            help_line("Tab", "Focus filter bar"),
            Line::from(""),
            Line::from(Span::styled("── Inline Actions ──", styles::title_style())),
            help_line("x", "Close / reopen"),
            help_line("l", "Set labels"),
            help_line("a", "Set assignee"),
            help_line("c", "Add comment"),
            Line::from(""),
        ]);
    }

    if view_context == "mr" || view_context == "all" {
        lines.extend([
            Line::from(Span::styled("── MR Actions ──", styles::title_style())),
            help_line("A", "Approve MR"),
            help_line("M", "Merge MR"),
            Line::from(""),
        ]);
    }

    if view_context == "detail" || view_context == "all" {
        lines.extend([
            Line::from(Span::styled("── Detail View ──", styles::title_style())),
            help_line("j/k", "Scroll"),
            help_line("c", "Add comment"),
            help_line("x", "Close / reopen"),
            help_line("l", "Set labels"),
            help_line("a", "Set assignee"),
            help_line("o", "Open in browser"),
            Line::from(""),
        ]);
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help (? to close) ")
        .title_style(styles::title_style())
        .border_style(styles::title_style());

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}

fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:>12}"), styles::help_key_style()),
        Span::raw("  "),
        Span::styled(desc, styles::help_desc_style()),
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
