use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::ui::styles;

#[derive(Debug, Default)]
pub struct InputState {
    pub value: String,
    pub cursor: usize,
}

impl InputState {
    pub fn new(initial: &str) -> Self {
        Self {
            cursor: initial.len(),
            value: initial.to_string(),
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
            }
            KeyCode::End => {
                self.cursor = self.value.len();
            }
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &InputState, title: &str) {
    let block = styles::overlay_block(title);

    let display = if state.value.is_empty() {
        String::from("_")
    } else {
        let mut s = state.value.clone();
        if state.cursor <= s.len() {
            s.insert(state.cursor, '|');
        }
        s
    };

    let paragraph = Paragraph::new(display).block(block);
    frame.render_widget(paragraph, area);
}
