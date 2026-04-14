use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, TableState};

use crate::filter::FilterCondition;
use crate::keybindings::{self, KeyAction};
use crate::sort::SortSpec;
use crate::ui::keys;
use crate::ui::styles;

// ── ListCursor — non-generic borrowed handle for navigation ──

/// Which navigation operation to perform on a list.
#[derive(Clone, Copy)]
pub enum NavOp {
    Next,
    Prev,
    First,
    Last,
    PageDown,
    PageUp,
}

/// A borrowed cursor into any `ItemList<T>`, carrying only the two fields
/// that navigation needs (`table_state` + length).  Because it is concrete
/// (not generic over `T`), callers can obtain one from *any* list without
/// dynamic dispatch.
pub struct ListCursor<'a> {
    table_state: &'a mut TableState,
    len: usize,
}

impl ListCursor<'_> {
    /// Apply a navigation operation.  Returns `true` if the selection moved.
    pub fn apply(&mut self, op: NavOp) -> bool {
        if self.len == 0 {
            return false;
        }
        let cur = self.table_state.selected().unwrap_or(0);
        let next = match op {
            NavOp::Next => (cur + 1).min(self.len - 1),
            NavOp::Prev => cur.saturating_sub(1),
            NavOp::First => 0,
            NavOp::Last => self.len - 1,
            NavOp::PageDown => (cur + 20).min(self.len - 1),
            NavOp::PageUp => cur.saturating_sub(20),
        };
        self.table_state.select(Some(next));
        next != cur
    }
}

// ── ItemList ──

/// A list of items with table selection state and indices into a source slice.
/// Generic over the item type — works for both issues and merge requests.
pub struct ItemList<T> {
    pub table_state: TableState,
    pub indices: Vec<usize>,
    _phantom: std::marker::PhantomData<fn() -> T>,
}

impl<T> Default for ItemList<T> {
    fn default() -> Self {
        Self {
            table_state: TableState::default(),
            indices: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> ItemList<T> {
    /// Borrow a non-generic cursor for navigation dispatch.
    pub fn cursor(&mut self) -> ListCursor<'_> {
        ListCursor {
            table_state: &mut self.table_state,
            len: self.indices.len(),
        }
    }

    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table_state
            .selected()
            .and_then(|sel| self.indices.get(sel).copied())
    }

    pub fn selected_item<'a>(&self, items: &'a [T]) -> Option<&'a T> {
        self.selected_index().and_then(|idx| items.get(idx))
    }

    /// Handle list navigation keys (j/k/g/G/pgup/pgdn/arrows).
    /// Returns `Some(true)` if selection moved, `Some(false)` if at boundary,
    /// `None` if the key is not a nav key.
    pub fn handle_nav_key(&mut self, key: &KeyEvent) -> Option<bool> {
        let action = keybindings::match_group(keybindings::LIST_NAV_BINDINGS, key)?;
        let op = match action {
            KeyAction::MoveDown => NavOp::Next,
            KeyAction::MoveUp => NavOp::Prev,
            KeyAction::Top => NavOp::First,
            KeyAction::Bottom => NavOp::Last,
            KeyAction::PageDown => NavOp::PageDown,
            KeyAction::PageUp => NavOp::PageUp,
            _ => return None,
        };
        Some(self.cursor().apply(op))
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

// ── UserFilter ──

/// Bundle of user-input filter, sort, and fuzzy search state.
#[derive(Default)]
pub struct UserFilter {
    pub conditions: Vec<FilterCondition>,
    pub sort_specs: Vec<SortSpec>,
    pub fuzzy_query: String,
    pub fuzzy_active: bool,
    pub bar_focused: bool,
    pub bar_selected: usize,
}

/// Result of filter bar handling a key.
pub enum FilterBarAction {
    /// Key consumed, no external effect.
    Consumed,
    /// User exited the filter bar (Esc/Tab).
    Unfocused,
    /// A filter condition was deleted — caller should refilter + persist.
    Deleted,
}

impl UserFilter {
    /// Handle keys when the filter bar is focused.
    /// The filter bar owns its navigation and condition deletion.
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

    /// Handle fuzzy search input (Esc/Enter/Backspace/Char).
    /// Returns `Some(true)` if refilter needed, `Some(false)` if handled but no refilter,
    /// `None` if not in search mode (key not consumed).
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

    /// Start fuzzy search input mode.
    pub fn start_search(&mut self) {
        self.fuzzy_active = true;
    }

    /// Multi-word fuzzy match: all words in the query must appear in the haystack.
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

// ── Shared rendering helpers ──

/// Build a block with search-mode title. Three states:
/// 1. Actively searching: cyan border, query with cursor, Enter/Esc hints
/// 2. Has query but not searching: normal border, query shown
/// 3. No query: plain block
pub fn search_block<'a>(label: &'a str, filter: &'a UserFilter) -> Block<'a> {
    if filter.fuzzy_active {
        let title_line = Line::from(vec![
            Span::styled(
                format!(" {label} /"),
                Style::default()
                    .fg(styles::CYAN)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                filter.fuzzy_query.as_str(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled("\u{258e}", Style::default().fg(styles::CYAN)),
            Span::styled(
                " Enter",
                Style::default()
                    .fg(styles::YELLOW)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(":accept ", Style::default().fg(styles::TEXT_DIM)),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(styles::YELLOW)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(":cancel ", Style::default().fg(styles::TEXT_DIM)),
        ]);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::CYAN))
            .title(title_line)
    } else if filter.has_query() {
        let title_line = Line::from(vec![
            Span::styled(
                format!(" {label} /"),
                Style::default()
                    .fg(styles::CYAN)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                filter.fuzzy_query.as_str(),
                Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(" ", Style::default()),
        ]);
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(styles::BORDER))
            .title(title_line)
    } else {
        styles::block(label)
    }
}

/// Format a timestamp as relative age (e.g. "3d", "5h", "12m").
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
