use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::filter::FilterCondition;
use crate::sort::SortSpec;
use crate::ui::styles;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    conditions: &[FilterCondition],
    sort_specs: &[SortSpec],
    focused: bool,
    selected_chip: usize,
) {
    if conditions.is_empty() && sort_specs.is_empty() && !focused {
        let hint = Paragraph::new(Line::from(vec![
            Span::styled(" f", styles::help_key_style()),
            Span::styled(":filter ", styles::help_desc_style()),
            Span::styled("S", styles::help_key_style()),
            Span::styled(":sort ", styles::help_desc_style()),
        ]));
        frame.render_widget(hint, area);
        return;
    }

    let mut spans = vec![Span::raw(" ")];

    for (i, cond) in conditions.iter().enumerate() {
        let text = format!(" {} ", cond.display());
        let style = if focused && i == selected_chip {
            styles::filter_chip_selected_style()
        } else {
            styles::filter_chip_style()
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
    }

    // Sort chips
    for spec in sort_specs {
        let text = format!(" {} ", spec.display());
        spans.push(Span::styled(text, styles::sort_chip_style()));
        spans.push(Span::raw(" "));
    }

    if focused {
        spans.push(Span::styled(
            " [x:remove Enter:edit Esc:back] ",
            styles::help_desc_style(),
        ));
    } else if !conditions.is_empty() || !sort_specs.is_empty() {
        spans.push(Span::styled("f", styles::help_key_style()));
        spans.push(Span::styled(":filter ", styles::help_desc_style()));
        if !sort_specs.is_empty() {
            spans.push(Span::styled("S", styles::help_key_style()));
            spans.push(Span::styled(":change-sort ", styles::help_desc_style()));
        }
    }

    let bar = Paragraph::new(Line::from(spans));
    frame.render_widget(bar, area);
}
