use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, TableState};

use crate::keybindings::KeyAction;
use crate::ui::keys;
use crate::ui::styles;
use glab_core::domain::Item;
use glab_core::filter::FilterCondition;
use glab_core::sort::SortSpec;

use crate::binding_group;

binding_group! {
    pub LIST_NAV_GROUP: "List Navigation" {
        ('j') => MoveDown | "j/k" "Move down/up",
        (key Down) => MoveDown,
        (ctrl 'n') => MoveDown,
        ('k') => MoveUp,
        (key Up) => MoveUp,
        (ctrl 'p') => MoveUp,
        ('g') => Top | "g/G" "Jump to top/bottom",
        ('G') => Bottom,
        (ctrl 'v') => PageDown | "^v/M-v" "Page down/up",
        (alt 'v') => PageUp,
        (key Enter) => OpenDetail | "Enter" "Open detail",
    }
}

binding_group! {
    pub FILTER_GROUP: "Filtering" {
        ('/') => StartSearch | "/" "Fuzzy search",
        ('f') => FilterMenu | "f" "Filter menu",
        ('F') => ClearFilters | "F" "Clear all filters",
        ('S') => SortByField | "S" "Sort by field",
        (key Tab) => FocusFilterBar | "Tab" "Focus filter bar",
    }
}

pub struct ItemList<T> {
    pub table_state: TableState,
    pub indices: Vec<usize>,
    /// Body rows at the last render; 0 until the first one.
    limit: usize,
    _phantom: std::marker::PhantomData<fn() -> T>,
}

impl<T> Default for ItemList<T> {
    fn default() -> Self {
        Self {
            table_state: TableState::default(),
            indices: Vec::new(),
            limit: 0,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> ItemList<T> {
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table_state
            .selected()
            .and_then(|sel| self.indices.get(sel).copied())
    }

    pub fn selected_item<'a>(&self, items: &'a [T]) -> Option<&'a T> {
        self.selected_index().and_then(|idx| items.get(idx))
    }

    pub fn nav(&mut self, action: KeyAction) -> Option<bool> {
        let cur = self.table_state.selected().unwrap_or(0);
        let last = self.indices.len().saturating_sub(1);
        let page = self.limit.max(1);
        let next = match action {
            KeyAction::MoveDown => (cur + 1).min(last),
            KeyAction::MoveUp => cur.saturating_sub(1),
            KeyAction::Top => 0,
            KeyAction::Bottom => last,
            KeyAction::PageDown => (cur + page).min(last),
            KeyAction::PageUp => cur.saturating_sub(page),
            _ => return None,
        };
        if self.indices.is_empty() {
            return Some(false);
        }
        self.table_state.select(Some(next));
        Some(next != cur)
    }

    /// Call before rendering, with the table's height minus block borders and
    /// header. Ratatui only scrolls to keep the cursor visible and never
    /// backfills: an offset near the end of a list that shrank would hide rows
    /// above a blank tail.
    pub fn set_limit(&mut self, rows: u16) {
        self.limit = usize::from(rows);
        let max = self.indices.len().saturating_sub(self.limit);
        let offset = self.table_state.offset_mut();
        *offset = (*offset).min(max);
    }

    pub fn clamp_selection(&mut self) {
        if self.indices.is_empty() {
            self.table_state.select(None);
        } else if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        } else if let Some(sel) = self.table_state.selected()
            && sel >= self.indices.len()
        {
            self.table_state.select(Some(self.indices.len() - 1));
        }
    }
}

impl<T: Item> ItemList<T> {
    /// Call before a rebuild invalidates `indices`.
    pub fn anchor(&self, items: &[T]) -> Option<String> {
        self.selected_item(items)
            .map(|item| item.reference().to_string())
    }

    /// An item that is gone leaves the cursor on whichever row inherited its
    /// index, so closing the one under the cursor steps to its successor.
    pub fn restore(&mut self, items: &[T], anchor: Option<&str>) {
        let pos = anchor.and_then(|reference| {
            self.indices
                .iter()
                .position(|&i| items[i].reference() == reference)
        });
        if let Some(pos) = pos {
            self.table_state.select(Some(pos));
        }
        self.clamp_selection();
    }
}

#[derive(Default)]
pub struct UserFilter {
    pub conditions: Vec<FilterCondition>,
    pub sort_specs: Vec<SortSpec>,
    pub fuzzy_query: String,
    pub fuzzy_active: bool,
    pub bar_focused: bool,
    pub bar_selected: usize,
}

pub enum FilterBarAction {
    Consumed,
    Unfocused,
    /// The caller refilters and persists.
    Deleted,
}

impl UserFilter {
    pub fn handle_bar_key(&mut self, key: &KeyEvent) -> FilterBarAction {
        if keys::is_back(key) || keys::is_tab(key) {
            self.bar_focused = false;
            return FilterBarAction::Unfocused;
        }
        if keys::is_left(key) {
            self.bar_selected = self.bar_selected.saturating_sub(1);
            return FilterBarAction::Consumed;
        }
        if keys::is_right(key)
            && !self.conditions.is_empty()
            && self.bar_selected + 1 < self.conditions.len()
        {
            self.bar_selected += 1;
            return FilterBarAction::Consumed;
        }
        if matches!(key.code, KeyCode::Char('x' | 'd')) && !self.conditions.is_empty() {
            self.conditions.remove(self.bar_selected);
            if self.bar_selected > 0 && self.bar_selected >= self.conditions.len() {
                self.bar_selected -= 1;
            }
            if self.conditions.is_empty() {
                self.bar_focused = false;
            }
            return FilterBarAction::Deleted;
        }
        FilterBarAction::Consumed
    }

    pub fn is_searching(&self) -> bool {
        self.fuzzy_active
    }

    pub fn has_query(&self) -> bool {
        !self.fuzzy_query.is_empty()
    }

    /// `Some(true)` when a refilter is needed, `Some(false)` when the key was
    /// handled without one, `None` when not in search mode.
    pub fn handle_fuzzy_input(&mut self, key: &KeyEvent) -> Option<bool> {
        if !self.fuzzy_active {
            return None;
        }
        match key.code {
            KeyCode::Esc => {
                self.fuzzy_active = false;
                self.fuzzy_query.clear();
                Some(true)
            }
            KeyCode::Enter => {
                self.fuzzy_active = false;
                Some(false)
            }
            KeyCode::Backspace => {
                self.fuzzy_query.pop();
                Some(true)
            }
            KeyCode::Char(c) => {
                self.fuzzy_query.push(c);
                Some(true)
            }
            _ => Some(false),
        }
    }

    pub fn start_search(&mut self) {
        self.fuzzy_active = true;
    }

    /// Every word in the query must appear in the haystack.
    pub fn fuzzy_matches(&self, haystack: &str) -> bool {
        if self.fuzzy_query.is_empty() {
            return true;
        }
        let lower = haystack.to_lowercase();
        self.fuzzy_query
            .to_lowercase()
            .split_whitespace()
            .all(|word| lower.contains(word))
    }
}

/// Titled for the search state: cyan with a cursor and hints while searching,
/// the query alone when one is set, plain when it is not.
pub fn search_block<'a>(label: &'a str, filter: &'a UserFilter) -> Block<'a> {
    if filter.fuzzy_active {
        let title_line = Line::from(vec![
            Span::styled(
                format!(" {label} /"),
                Style::default()
                    .fg(styles::cyan())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                filter.fuzzy_query.as_str(),
                Style::default()
                    .fg(styles::text_bright())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled("\u{258e}", Style::default().fg(styles::cyan())),
            Span::styled(
                " Enter",
                Style::default()
                    .fg(styles::yellow())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(":accept ", Style::default().fg(styles::text_dim())),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(styles::yellow())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(":cancel ", Style::default().fg(styles::text_dim())),
        ]);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::cyan()))
            .title(title_line)
    } else if filter.has_query() {
        let title_line = Line::from(vec![
            Span::styled(
                format!(" {label} /"),
                Style::default()
                    .fg(styles::cyan())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                filter.fuzzy_query.as_str(),
                Style::default()
                    .fg(styles::text_bright())
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(" ", Style::default()),
        ]);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::border()))
            .title(title_line)
    } else {
        styles::block(label)
    }
}

/// As "3d", "5h", "12m".
pub fn format_age(
    dt: &chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let diff = now.signed_duration_since(*dt);
    if diff.num_days() > 0 {
        format!("{}d", diff.num_days())
    } else if diff.num_hours() > 0 {
        format!("{}h", diff.num_hours())
    } else {
        format!("{}m", diff.num_minutes())
    }
}

#[cfg(test)]
#[path = "list_model_tests.rs"]
mod tests;
