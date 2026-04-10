use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::styles;

#[derive(Debug, Default)]
pub struct InputState {
    pub value: String,
    /// Byte offset into `value` where the cursor sits.
    pub cursor: usize,
}

impl InputState {
    #[allow(dead_code)]
    pub fn new(initial: &str) -> Self {
        Self {
            cursor: initial.len(),
            value: initial.to_string(),
        }
    }

    /// Handle a key event. Returns `true` if the key was a submit action (Ctrl+S).
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        // Ctrl+S = submit
        if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        match key.code {
            KeyCode::Enter => {
                self.value.insert(self.cursor, '\n');
                self.cursor += 1;
            }
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Backspace if self.cursor > 0 => {
                // Find the previous char boundary
                let prev = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
                self.value.remove(prev);
                self.cursor = prev;
            }
            KeyCode::Delete if self.cursor < self.value.len() => {
                self.value.remove(self.cursor);
            }
            KeyCode::Left if self.cursor > 0 => {
                let prev = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
                self.cursor = prev;
            }
            KeyCode::Right if self.cursor < self.value.len() => {
                let ch = self.value[self.cursor..].chars().next().unwrap_or(' ');
                self.cursor += ch.len_utf8();
            }
            KeyCode::Up => {
                self.move_cursor_vertical(-1);
            }
            KeyCode::Down => {
                self.move_cursor_vertical(1);
            }
            KeyCode::Home => {
                // Move to start of current line
                self.cursor = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
            }
            KeyCode::End => {
                // Move to end of current line
                self.cursor = self.value[self.cursor..]
                    .find('\n')
                    .map_or(self.value.len(), |i| self.cursor + i);
            }
            _ => {}
        }
        false
    }

    fn move_cursor_vertical(&mut self, direction: i32) {
        let (row, col) = self.cursor_row_col();
        let lines: Vec<&str> = self.value.split('\n').collect();
        let new_row = if direction < 0 {
            row.saturating_sub(1)
        } else {
            (row + 1).min(lines.len().saturating_sub(1))
        };
        if new_row == row {
            return;
        }
        let new_line = lines[new_row];
        let new_col = col.min(new_line.len());
        // Calculate byte offset for the new position
        let mut offset = 0;
        for line in &lines[..new_row] {
            offset += line.len() + 1; // +1 for '\n'
        }
        offset += new_col;
        self.cursor = offset;
    }

    /// Returns (row, col) of the cursor, both 0-indexed.
    pub fn cursor_row_col(&self) -> (usize, usize) {
        let before = &self.value[..self.cursor];
        let row = before.matches('\n').count();
        let col = before
            .rfind('\n')
            .map_or(self.cursor, |i| self.cursor - i - 1);
        (row, col)
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &InputState, title: &str) {
    let block = styles::overlay_block(title);
    let inner = block.inner(area);

    let lines: Vec<&str> = state.value.split('\n').collect();
    let (cursor_row, cursor_col) = state.cursor_row_col();

    let display_lines: Vec<Line<'_>> = lines
        .iter()
        .enumerate()
        .map(|(row, line)| {
            if row == cursor_row {
                if line.is_empty() {
                    Line::from(Span::styled("\u{2502}", styles::overlay_text_style()))
                } else {
                    let mut spans = Vec::new();
                    let before = &line[..cursor_col.min(line.len())];
                    spans.push(Span::styled(
                        before.to_string(),
                        styles::overlay_text_style(),
                    ));
                    spans.push(Span::styled("\u{2502}", styles::overlay_text_style()));
                    if cursor_col < line.len() {
                        let after = &line[cursor_col..];
                        spans.push(Span::styled(
                            after.to_string(),
                            styles::overlay_text_style(),
                        ));
                    }
                    Line::from(spans)
                }
            } else {
                Line::from(Span::styled(line.to_string(), styles::overlay_text_style()))
            }
        })
        .collect();

    // If the input is completely empty, show a cursor placeholder
    let display_lines = if state.value.is_empty() {
        vec![Line::from(Span::styled(
            "\u{2502}",
            styles::overlay_text_style(),
        ))]
    } else {
        display_lines
    };

    // Scroll: if cursor row is beyond visible area, scroll down
    let visible_height = inner.height as usize;
    let scroll = if cursor_row >= visible_height {
        (cursor_row - visible_height + 1) as u16
    } else {
        0
    };

    let paragraph = Paragraph::new(display_lines)
        .block(block)
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}
