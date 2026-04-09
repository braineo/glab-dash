use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn is_quit(key: &KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::NONE,
            ..
        } | KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
    )
}

pub fn is_back(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc
}

pub fn is_up(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('k') | KeyCode::Up)
}

pub fn is_down(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('j') | KeyCode::Down)
}

pub fn is_left(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('h') | KeyCode::Left)
}

pub fn is_right(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('l') | KeyCode::Right)
}

pub fn is_top(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('g') && key.modifiers == KeyModifiers::NONE
}

pub fn is_bottom(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('G')
}

pub fn is_page_down(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('d') && key.modifiers == KeyModifiers::CONTROL
}

pub fn is_page_up(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('u') && key.modifiers == KeyModifiers::CONTROL
}

pub fn is_enter(key: &KeyEvent) -> bool {
    key.code == KeyCode::Enter
}

pub fn is_tab(key: &KeyEvent) -> bool {
    key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE
}

pub fn is_backtab(key: &KeyEvent) -> bool {
    key.code == KeyCode::BackTab
}
