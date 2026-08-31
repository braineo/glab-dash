use crossterm::event::{KeyCode, KeyEvent};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use crate::ui::{keys, styles};

pub struct PickerState {
    pub title: String,
    pub items: Vec<String>,
    /// Optional second line per item, shown below the main text in a dimmer style.
    pub subtitles: Vec<String>,
    pub filtered: Vec<usize>,
    pub query: String,
    pub list_state: ListState,
    pub multi_select: bool,
    pub selected: Vec<bool>,
    matcher: SkimMatcherV2,
}

impl PickerState {
    pub fn new(title: &str, items: Vec<String>, multi_select: bool) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        let selected = vec![false; items.len()];
        let mut state = Self {
            title: title.to_string(),
            subtitles: Vec::new(),
            filtered,
            query: String::new(),
            list_state: ListState::default(),
            multi_select,
            selected,
            matcher: SkimMatcherV2::default(),
            items,
        };
        if !state.filtered.is_empty() {
            state.list_state.select(Some(0));
        }
        state
    }

    #[must_use]
    pub fn with_subtitles(mut self, subtitles: Vec<String>) -> Self {
        self.subtitles = subtitles;
        self
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> PickerAction {
        // Use nav variants (no j/k) since typing is active in picker
        if keys::is_nav_up(key) {
            self.move_up();
            return PickerAction::Continue;
        }
        if keys::is_nav_down(key) {
            self.move_down();
            return PickerAction::Continue;
        }
        match key.code {
            KeyCode::Esc => return PickerAction::Cancel,
            KeyCode::Enter => return self.confirm(),
            KeyCode::Char(' ') if self.multi_select => {
                if let Some(idx) = self.current_item_idx() {
                    self.toggle_label(idx);
                }
                self.move_down();
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refilter();
            }
            KeyCode::Char(c) => {
                self.query.push(c);
                self.refilter();
            }
            _ => {}
        }
        PickerAction::Continue
    }

    /// Toggle a label, enforcing scoped label mutual exclusivity.
    /// If the toggled label is a scoped label (contains `::`) and is being
    /// selected, deselect any other label with the same scope.
    fn toggle_label(&mut self, idx: usize) {
        let was_selected = self.selected[idx];
        if was_selected {
            // Just deselect
            self.selected[idx] = false;
            return;
        }
        // Selecting: check for scoped label conflict
        let label = &self.items[idx];
        if let Some(scope) = label.split_once("::").map(|(s, _)| s) {
            // Deselect any other label with the same scope
            for (i, item) in self.items.iter().enumerate() {
                if i != idx
                    && self.selected[i]
                    && item.split_once("::").map(|(s, _)| s) == Some(scope)
                {
                    self.selected[i] = false;
                }
            }
        }
        self.selected[idx] = true;
    }

    fn refilter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(usize, i64)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    self.matcher
                        .fuzzy_match(item, &self.query)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by_key(|item| std::cmp::Reverse(item.1));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }
        if self.filtered.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn move_up(&mut self) {
        if let Some(selected) = self.list_state.selected()
            && selected > 0
        {
            self.list_state.select(Some(selected - 1));
        }
    }

    fn move_down(&mut self) {
        if let Some(selected) = self.list_state.selected()
            && selected + 1 < self.filtered.len()
        {
            self.list_state.select(Some(selected + 1));
        }
    }

    fn current_item_idx(&self) -> Option<usize> {
        self.list_state
            .selected()
            .and_then(|i| self.filtered.get(i).copied())
    }

    fn confirm(&self) -> PickerAction {
        if self.multi_select {
            let chosen: Vec<String> = self
                .items
                .iter()
                .enumerate()
                .filter(|(i, _)| self.selected[*i])
                .map(|(_, s)| s.clone())
                .collect();
            PickerAction::Picked(chosen)
        } else if let Some(idx) = self.current_item_idx() {
            PickerAction::Picked(vec![self.items[idx].clone()])
        } else {
            PickerAction::Cancel
        }
    }
}

pub enum PickerAction {
    Continue,
    Picked(Vec<String>),
    Cancel,
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut PickerState,
    ctx: &crate::ui::RenderCtx<'_>,
) {
    let label_colors = ctx.label_colors;
    let popup = centered_rect(50, 60, area);
    frame.render_widget(Clear, popup);

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(popup);

    // Search input
    let search_block = styles::overlay_block(&state.title);
    let search_text = if state.query.is_empty() {
        Span::styled(
            "Type to filter...",
            styles::overlay_desc_style().add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(&state.query, styles::overlay_text_style())
    };
    let search = Paragraph::new(Line::from(search_text)).block(search_block);
    frame.render_widget(search, chunks[0]);

    // List
    let items: Vec<ListItem> = state
        .filtered
        .iter()
        .map(|&idx| {
            let mut spans = Vec::new();
            if state.multi_select {
                let (icon, style) = if state.selected[idx] {
                    (styles::ICON_CHECK, styles::source_tracking_style())
                } else {
                    (styles::ICON_UNCHECK, styles::overlay_desc_style())
                };
                spans.push(Span::styled(format!("{icon} "), style));
            }
            // Render labels with scoped styling in the Labels picker
            if state.title == "Labels" {
                let color = label_colors.get(&state.items[idx]).map(String::as_str);
                spans.extend(styles::label_spans(&state.items[idx], color));
            } else if state.title == "Set Status" {
                let name = &state.items[idx];
                let icon = styles::status_icon(name);
                let style = styles::status_style(name);
                spans.push(Span::styled(format!("{icon} "), style));
                spans.push(Span::styled(name.clone(), style));
            } else {
                spans.push(Span::styled(
                    &state.items[idx],
                    styles::overlay_text_style(),
                ));
            }
            let mut lines = vec![Line::from(spans)];
            // Show subtitle as a second line if available
            if let Some(sub) = state.subtitles.get(idx)
                && !sub.is_empty()
            {
                lines.push(Line::from(Span::styled(
                    format!("  {sub}"),
                    styles::overlay_desc_style(),
                )));
            }
            ListItem::new(lines)
        })
        .collect();

    let hint = if state.multi_select {
        "Space:toggle  Enter:confirm  Esc:cancel"
    } else {
        "Enter:select  Esc:cancel"
    };
    let list_block = styles::overlay_block(hint);
    let list = List::new(items)
        .block(list_block)
        .highlight_style(styles::selected_style().add_modifier(Modifier::BOLD))
        .highlight_symbol(styles::ICON_SELECTOR);

    frame.render_stateful_widget(list, chunks[1], &mut state.list_state);
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
