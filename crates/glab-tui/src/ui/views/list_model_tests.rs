use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use super::*;
use glab_core::domain::{ItemKind, User};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

// ── ItemList tests ──

#[test]
fn test_item_list_default_is_empty() {
    let list: ItemList<u32> = ItemList::default();
    assert_eq!(list.len(), 0);
    assert_eq!(list.selected_index(), None);
}

#[test]
fn test_item_list_selected_item() {
    let items = vec![10, 20, 30];
    let mut list: ItemList<i32> = ItemList {
        indices: vec![2, 0], // points to items[2]=30, items[0]=10
        ..Default::default()
    };
    list.table_state.select(Some(0));

    assert_eq!(list.selected_item(&items), Some(&30));
    list.table_state.select(Some(1));
    assert_eq!(list.selected_item(&items), Some(&10));
}

#[test]
fn test_item_list_selected_index_out_of_bounds() {
    let mut list: ItemList<u32> = ItemList {
        indices: vec![5],
        ..Default::default()
    };
    list.table_state.select(Some(3)); // beyond indices len
    assert_eq!(list.selected_index(), None);
}

#[test]
fn test_clamp_selection_empty() {
    let mut list: ItemList<u32> = ItemList::default();
    list.table_state.select(Some(5));
    list.clamp_selection();
    assert_eq!(list.table_state.selected(), None);
}

#[test]
fn test_clamp_selection_none_to_first() {
    let mut list: ItemList<u32> = ItemList {
        indices: vec![0, 1, 2],
        ..Default::default()
    };
    list.clamp_selection();
    assert_eq!(list.table_state.selected(), Some(0));
}

#[test]
fn test_clamp_selection_past_end() {
    let mut list: ItemList<u32> = ItemList {
        indices: vec![0, 1],
        ..Default::default()
    };
    list.table_state.select(Some(5));
    list.clamp_selection();
    assert_eq!(list.table_state.selected(), Some(1));
}

#[test]
fn test_clamp_selection_valid_unchanged() {
    let mut list: ItemList<u32> = ItemList {
        indices: vec![0, 1, 2],
        ..Default::default()
    };
    list.table_state.select(Some(1));
    list.clamp_selection();
    assert_eq!(list.table_state.selected(), Some(1));
}

// ── UserFilter tests ──

#[test]
fn test_user_filter_default() {
    let f = UserFilter::default();
    assert!(!f.is_searching());
    assert!(!f.has_query());
    assert!(f.conditions.is_empty());
    assert!(f.sort_specs.is_empty());
}

#[test]
fn test_fuzzy_matches_empty_query() {
    let f = UserFilter::default();
    assert!(f.fuzzy_matches("anything"));
}

#[test]
fn test_fuzzy_matches_single_word() {
    let f = UserFilter {
        fuzzy_query: "bug".to_string(),
        ..Default::default()
    };

    assert!(f.fuzzy_matches("fix bug in parser"));
    assert!(!f.fuzzy_matches("fix issue in parser"));
}

#[test]
fn test_fuzzy_matches_multiple_words() {
    let f = UserFilter {
        fuzzy_query: "bug parser".to_string(),
        ..Default::default()
    };

    assert!(f.fuzzy_matches("fix bug in parser"));
    assert!(f.fuzzy_matches("parser has a bug"));
    assert!(!f.fuzzy_matches("fix bug in lexer"));
}

#[test]
fn test_fuzzy_matches_case_insensitive() {
    let f = UserFilter {
        fuzzy_query: "BUG".to_string(),
        ..Default::default()
    };

    assert!(f.fuzzy_matches("Fix Bug in Parser"));
}

#[test]
fn test_handle_fuzzy_input_not_active() {
    let mut f = UserFilter::default();
    assert_eq!(f.handle_fuzzy_input(&key(KeyCode::Char('a'))), None);
}

#[test]
fn test_handle_fuzzy_input_char() {
    let mut f = UserFilter::default();
    f.start_search();

    assert_eq!(f.handle_fuzzy_input(&key(KeyCode::Char('a'))), Some(true));
    assert_eq!(f.fuzzy_query, "a");
    assert!(f.is_searching());

    assert_eq!(f.handle_fuzzy_input(&key(KeyCode::Char('b'))), Some(true));
    assert_eq!(f.fuzzy_query, "ab");
}

#[test]
fn test_handle_fuzzy_input_backspace() {
    let mut f = UserFilter::default();
    f.start_search();
    f.fuzzy_query = "abc".to_string();

    assert_eq!(f.handle_fuzzy_input(&key(KeyCode::Backspace)), Some(true));
    assert_eq!(f.fuzzy_query, "ab");
}

#[test]
fn test_handle_fuzzy_input_enter_confirms() {
    let mut f = UserFilter::default();
    f.start_search();
    f.fuzzy_query = "test".to_string();

    assert_eq!(f.handle_fuzzy_input(&key(KeyCode::Enter)), Some(false));
    assert!(!f.is_searching());
    assert_eq!(f.fuzzy_query, "test"); // query preserved
}

#[test]
fn test_handle_fuzzy_input_esc_cancels() {
    let mut f = UserFilter::default();
    f.start_search();
    f.fuzzy_query = "test".to_string();

    assert_eq!(f.handle_fuzzy_input(&key(KeyCode::Esc)), Some(true));
    assert!(!f.is_searching());
    assert_eq!(f.fuzzy_query, ""); // query cleared
}

#[test]
fn test_start_search() {
    let mut f = UserFilter::default();
    assert!(!f.is_searching());
    f.start_search();
    assert!(f.is_searching());
}

// ── format_age tests ──

#[test]
fn test_format_age_days() {
    let now = chrono::Utc::now();
    let dt = now - chrono::Duration::days(3);
    assert_eq!(format_age(&dt, now), "3d");
}

#[test]
fn test_format_age_hours() {
    let now = chrono::Utc::now();
    let dt = now - chrono::Duration::hours(5);
    assert_eq!(format_age(&dt, now), "5h");
}

#[test]
fn test_format_age_minutes() {
    let now = chrono::Utc::now();
    let dt = now - chrono::Duration::minutes(42);
    assert_eq!(format_age(&dt, now), "42m");
}

fn cond() -> FilterCondition {
    FilterCondition {
        field: glab_core::filter::Field::Label,
        op: glab_core::filter::Op::Eq,
        value: "x".to_string(),
    }
}

// ── Filter bar focus lifecycle ──
//
// The bar is reachable only through `KeyAction::FocusFilterBar`; these cover
// what it does once focused, so the wiring is not the only thing under test.

#[test]
fn the_filter_bar_walks_and_deletes_conditions() {
    let mut f = UserFilter {
        conditions: vec![cond(), cond(), cond()],
        bar_focused: true,
        ..UserFilter::default()
    };

    // Right walks up to the last chip and stops there; left saturates at 0.
    for expected in [1, 2, 2] {
        f.handle_bar_key(&key(KeyCode::Right));
        assert_eq!(f.bar_selected, expected);
    }
    for expected in [1, 0, 0] {
        f.handle_bar_key(&key(KeyCode::Left));
        assert_eq!(f.bar_selected, expected);
    }

    // `x` removes the selected chip and reports it so the view repersists.
    assert!(matches!(
        f.handle_bar_key(&key(KeyCode::Char('x'))),
        FilterBarAction::Deleted
    ));
    assert_eq!(f.conditions.len(), 2);
    assert!(f.bar_focused);
}

#[test]
fn the_filter_bar_releases_focus_on_esc_tab_and_the_last_delete() {
    for k in [key(KeyCode::Esc), key(KeyCode::Tab)] {
        let mut f = UserFilter {
            conditions: vec![cond()],
            bar_focused: true,
            ..UserFilter::default()
        };
        assert!(matches!(f.handle_bar_key(&k), FilterBarAction::Unfocused));
        assert!(!f.bar_focused);
    }

    // Deleting the last condition leaves nothing to walk, so focus drops.
    let mut f = UserFilter {
        conditions: vec![cond()],
        bar_focused: true,
        ..UserFilter::default()
    };
    f.handle_bar_key(&key(KeyCode::Char('d')));
    assert!(f.conditions.is_empty());
    assert!(!f.bar_focused);
}

// ── Cursor reconciliation ──

/// All `restore` looks at is the reference, so an item is one here.
struct Row(&'static str);

impl Item for Row {
    fn kind(&self) -> ItemKind {
        ItemKind::Issue
    }
    fn gid(&self) -> &str {
        self.0
    }
    fn iid(&self) -> &str {
        self.0
    }
    fn reference(&self) -> &str {
        self.0
    }
    fn title(&self) -> &str {
        self.0
    }
    fn state(&self) -> &'static str {
        "opened"
    }
    fn web_url(&self) -> Option<&str> {
        None
    }
    fn labels(&self) -> &[String] {
        &[]
    }
    fn assignees(&self) -> &[User] {
        &[]
    }
}

#[test]
fn a_rebuild_keeps_the_cursor_on_its_item_wherever_it_lands() {
    let items = [Row("p#1"), Row("p#2"), Row("p#3")];
    let mut list: ItemList<Row> = ItemList {
        indices: vec![0, 1, 2],
        ..Default::default()
    };
    list.table_state.select(Some(1));

    let anchor = list.anchor(&items);
    assert_eq!(anchor.as_deref(), Some("p#2"));

    // A sort moved it to the end; the cursor follows the item, not the row.
    list.indices = vec![2, 0, 1];
    list.restore(&items, anchor.as_deref());
    assert_eq!(list.selected_item(&items).map(|r| r.0), Some("p#2"));

    // Gone — filtered out or closed: the cursor holds its row, which the
    // successor now occupies.
    list.indices = vec![0, 2];
    list.table_state.select(Some(1));
    list.restore(&items, anchor.as_deref());
    assert_eq!(list.selected_item(&items).map(|r| r.0), Some("p#3"));

    // Gone from the last row: the cursor clamps onto the new last row.
    list.indices = vec![0];
    list.restore(&items, anchor.as_deref());
    assert_eq!(list.selected_item(&items).map(|r| r.0), Some("p#1"));

    // Nothing left to point at.
    list.indices.clear();
    list.restore(&items, anchor.as_deref());
    assert_eq!(list.table_state.selected(), None);
}

#[test]
fn a_list_with_no_cursor_yet_starts_at_the_top() {
    let items = [Row("p#1"), Row("p#2")];
    let mut list: ItemList<Row> = ItemList {
        indices: vec![0, 1],
        ..Default::default()
    };

    assert_eq!(list.anchor(&items), None);
    list.restore(&items, None);
    assert_eq!(list.table_state.selected(), Some(0));
}

#[test]
fn a_rebuild_drops_a_stale_scroll_offset() {
    let items = [Row("p#1"), Row("p#2"), Row("p#3")];
    let mut list: ItemList<Row> = ItemList {
        indices: vec![0, 1, 2],
        ..Default::default()
    };
    list.table_state.select(Some(2));
    *list.table_state.offset_mut() = 2;

    // Narrowed to one match: an offset of 2 would have ratatui start the
    // viewport past the only row left.
    list.indices = vec![1];
    list.restore(&items, Some("p#2"));
    assert_eq!(list.table_state.offset(), 0);
    assert_eq!(list.selected_item(&items).map(|r| r.0), Some("p#2"));
}
