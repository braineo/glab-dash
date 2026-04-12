use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::View;
use crate::ui::styles;

struct Tab {
    icon: &'static str,
    label: &'static str,
    key: char,
    view: View,
}

const TABS: &[Tab] = &[
    Tab {
        icon: styles::ICON_DASHBOARD,
        label: "Dashboard",
        key: '1',
        view: View::Dashboard,
    },
    Tab {
        icon: styles::ICON_ISSUES,
        label: "Issues",
        key: '2',
        view: View::IssueList,
    },
    Tab {
        icon: styles::ICON_MRS,
        label: "MRs",
        key: '3',
        view: View::MrList,
    },
    Tab {
        icon: styles::ICON_PLANNING,
        label: "Planning",
        key: '4',
        view: View::Planning,
    },
];

/// Map detail views to their parent tab.
fn active_tab(view: View) -> View {
    match view {
        View::IssueDetail => View::IssueList,
        View::MrDetail => View::MrList,
        other => other,
    }
}

pub fn render(frame: &mut Frame, area: Rect, current_view: View) {
    let active = active_tab(current_view);
    let mut spans = Vec::new();

    for tab in TABS {
        let is_active = tab.view == active;

        if is_active {
            spans.push(Span::styled(
                format!(" {} ", tab.icon),
                Style::default()
                    .fg(styles::CYAN)
                    .bg(styles::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("{} ", tab.label),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .bg(styles::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", tab.icon),
                Style::default().fg(styles::TEXT_DIM).bg(styles::SURFACE),
            ));
            spans.push(Span::styled(
                format!("{}:{} ", tab.key, tab.label),
                Style::default().fg(styles::TEXT).bg(styles::SURFACE),
            ));
        }
    }

    // Fill remaining width
    let used: usize = spans.iter().map(Span::width).sum();
    let remaining = usize::from(area.width).saturating_sub(used);
    spans.push(Span::styled(
        " ".repeat(remaining),
        Style::default().bg(styles::SURFACE),
    ));

    let bar = Paragraph::new(Line::from(spans));
    frame.render_widget(bar, area);
}
