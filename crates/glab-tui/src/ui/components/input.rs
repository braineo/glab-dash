use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use tui_textarea::TextArea;

use crate::ui::styles;

/// Result of handling a key event in the comment input.
pub enum InputAction {
    /// User pressed Ctrl+S — submit the comment.
    Submit,
    /// User pressed Esc — cancel input.
    Cancel,
    /// Key was consumed normally (text edited, cursor moved, etc.).
    Continue,
}

/// Multi-line comment input backed by `tui-textarea`.
///
/// Provides proper grapheme-cluster handling, emacs keybindings, undo/redo,
/// and word-level navigation out of the box.
pub struct CommentInput {
    textarea: TextArea<'static>,
}

impl Default for CommentInput {
    fn default() -> Self {
        let mut textarea = TextArea::default();
        apply_style(&mut textarea);
        Self { textarea }
    }
}

impl CommentInput {
    /// Handle a key event. Returns the resulting action.
    ///
    /// - **Enter** submits the comment.
    /// - **Ctrl+J** or **Shift+Enter** inserts a newline.
    /// - **Esc** cancels.
    /// - Everything else is delegated to `tui-textarea` (emacs keybindings, etc.).
    pub fn handle_key(&mut self, key: &KeyEvent) -> InputAction {
        if key.code == KeyCode::Esc {
            return InputAction::Cancel;
        }
        // Ctrl+J → newline (works on all terminals, classic Unix newline key)
        if (key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL))
            || key.code == KeyCode::Enter
        {
            self.textarea.insert_newline();
            return InputAction::Continue;
        }

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return InputAction::Submit;
        }

        self.textarea.input(*key);
        InputAction::Continue
    }

    /// Get the full text content as a single string (lines joined by `\n`).
    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Byte offset of the cursor in the flat text returned by [`text()`].
    pub fn cursor_byte_pos(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let mut pos: usize = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == row {
                // `col` is a character index — convert to byte offset within this line.
                pos += line
                    .char_indices()
                    .nth(col)
                    .map_or(line.len(), |(byte_idx, _)| byte_idx);
                break;
            }
            pos += line.len() + 1; // +1 for the '\n'
        }
        pos
    }

    /// Replace the `query_chars` characters before the cursor with `insert`,
    /// then append a space. Used to accept an autocomplete suggestion.
    pub fn replace_before_cursor(&mut self, query_chars: usize, insert: &str) {
        for _ in 0..query_chars {
            self.textarea.delete_char();
        }
        self.textarea.insert_str(format!("{insert} "));
    }
}

fn apply_style(textarea: &mut TextArea<'_>) {
    let text_style = styles::overlay_text_style().bg(styles::overlay());
    textarea.set_style(text_style);
    textarea.set_cursor_line_style(text_style);
    textarea.set_cursor_style(
        ratatui::style::Style::default()
            .fg(styles::overlay())
            .bg(styles::overlay_text()),
    );
}

pub fn render(frame: &mut Frame, area: Rect, input: &mut CommentInput, title: &str) {
    // Build the block with an owned title so it satisfies TextArea<'static>.
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(styles::border_active()))
        .title(format!(" {title} "))
        .title_style(
            ratatui::style::Style::default()
                .fg(styles::cyan())
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .style(ratatui::style::Style::default().bg(styles::overlay()));
    input.textarea.set_block(block);
    frame.render_widget(&input.textarea, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_before_cursor_swaps_query_for_completion() {
        let mut input = CommentInput::default();
        for c in "hi @jo".chars() {
            input.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        input.replace_before_cursor(2, "john.doe");
        assert_eq!(input.text(), "hi @john.doe ");
        assert_eq!(input.cursor_byte_pos(), input.text().len());
    }
}
