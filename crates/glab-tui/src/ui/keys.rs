use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn is_back(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc
}

/// Includes `k`, so not for a typing context.
pub fn is_up(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('k') | KeyCode::Up)
        || (key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::CONTROL)
}

/// Includes `j`, so not for a typing context.
pub fn is_down(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('j') | KeyCode::Down)
        || (key.code == KeyCode::Char('n') && key.modifiers == KeyModifiers::CONTROL)
}

pub fn is_nav_up(key: &KeyEvent) -> bool {
    key.code == KeyCode::Up
        || (key.code == KeyCode::Char('p') && key.modifiers == KeyModifiers::CONTROL)
}

pub fn is_nav_down(key: &KeyEvent) -> bool {
    key.code == KeyCode::Down
        || (key.code == KeyCode::Char('n') && key.modifiers == KeyModifiers::CONTROL)
}

pub fn is_left(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('h') | KeyCode::Left)
}

pub fn is_right(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('l') | KeyCode::Right)
}

pub fn is_tab(key: &KeyEvent) -> bool {
    key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE
}
