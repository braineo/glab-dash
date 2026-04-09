use crossterm::event::{KeyCode, KeyEvent};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::ui::styles;

pub struct PickerState {
    pub title: String,
    pub items: Vec<String>,
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

    pub fn with_pre_selected(mut self, pre: &[String]) -> Self {
        for (i, item) in self.items.iter().enumerate() {
            if pre.iter().any(|p| p == item) {
                self.selected[i] = true;
            }
        }
        self
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> PickerAction {
        match key.code {
            KeyCode::Esc => return PickerAction::Cancel,
            KeyCode::Enter => return self.confirm(),
            KeyCode::Char(' ') if self.multi_select => {
                if let Some(idx) = self.current_item_idx() {
                    self.selected[idx] = !self.selected[idx];
                }
                self.move_down();
            }
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
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
            scored.sort_by(|a, b| b.1.cmp(&a.1));
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

pub fn render(frame: &mut Frame, area: Rect, state: &mut PickerState) {
    let popup = centered_rect(50, 60, area);
    frame.render_widget(Clear, popup);

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(popup);

    // Search input
    let search_block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", state.title))
        .title_style(styles::title_style())
        .border_style(styles::title_style());
    let search_text = if state.query.is_empty() {
        "Type to filter...".to_string()
    } else {
        state.query.clone()
    };
    let search = Paragraph::new(search_text).block(search_block);
    frame.render_widget(search, chunks[0]);

    // List
    let items: Vec<ListItem> = state
        .filtered
        .iter()
        .map(|&idx| {
            let mut spans = Vec::new();
            if state.multi_select {
                let check = if state.selected[idx] { "[x] " } else { "[ ] " };
                spans.push(Span::styled(check, styles::help_key_style()));
            }
            spans.push(Span::raw(&state.items[idx]));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_style(styles::title_style());
    let list = List::new(items)
        .block(list_block)
        .highlight_style(styles::selected_style().add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");

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
