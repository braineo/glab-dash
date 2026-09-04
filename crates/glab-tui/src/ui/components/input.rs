use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use tui_textarea::{CursorMove, TextArea};

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
        if key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.textarea.insert_newline();
            return InputAction::Continue;
        }
        if key.code == KeyCode::Enter {
            if key.modifiers.intersects(KeyModifiers::SHIFT) {
                // Shift+Enter → newline (requires Kitty keyboard protocol)
                self.textarea.insert_newline();
            } else {
                return InputAction::Submit;
            }
            return InputAction::Continue;
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

    /// Replace the entire text and reposition the cursor at the given byte offset.
    pub fn set_text_and_cursor(&mut self, text: &str, byte_pos: usize) {
        let lines: Vec<&str> = text.split('\n').collect();

        // Convert byte_pos to (row, char_col).
        let mut remaining = byte_pos;
        let mut target_row: u16 = 0;
        let mut target_col: u16 = 0;
        for (i, line) in lines.iter().enumerate() {
            if remaining <= line.len() {
                target_row = u16::try_from(i).unwrap_or(u16::MAX);
                target_col = u16::try_from(line[..remaining].chars().count()).unwrap_or(u16::MAX);
                break;
            }
            remaining -= line.len() + 1; // +1 for '\n'
        }

        let owned_lines: Vec<String> = lines.into_iter().map(String::from).collect();
        self.textarea = TextArea::new(owned_lines);
        apply_style(&mut self.textarea);
        self.textarea
            .move_cursor(CursorMove::Jump(target_row, target_col));
    }
}

fn apply_style(textarea: &mut TextArea<'_>) {
    let text_style = styles::overlay_text_style().bg(styles::OVERLAY);
    textarea.set_style(text_style);
    textarea.set_cursor_line_style(text_style);
    textarea.set_cursor_style(
        ratatui::style::Style::default()
            .fg(styles::OVERLAY)
            .bg(styles::OVERLAY_TEXT),
    );
}

pub fn render(frame: &mut Frame, area: Rect, input: &mut CommentInput, title: &str) {
    // Build the block with an owned title so it satisfies TextArea<'static>.
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER_ACTIVE))
        .title(format!(" {title} "))
        .title_style(
            ratatui::style::Style::default()
                .fg(styles::CYAN)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )
        .style(ratatui::style::Style::default().bg(styles::OVERLAY));
    input.textarea.set_block(block);
    frame.render_widget(&input.textarea, area);
}
