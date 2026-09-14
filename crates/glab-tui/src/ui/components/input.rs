use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use tui_textarea::{CursorMove, TextArea, WrapMode};

use crate::ui::styles;

/// What a draft does to the conversation when it is submitted.
#[derive(Debug, PartialEq, Eq)]
pub enum CommentTarget {
    /// Open a new top-level thread.
    NewThread,
    /// Reply into the thread with this discussion id.
    Reply(String),
    /// Rewrite the note with this id, which is what the draft started from.
    Edit(u64),
}

/// Result of handling a key event in the comment input.
pub enum InputAction {
    /// User pressed Ctrl+C — submit the comment.
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
    /// Emacs-style incremental search query, `Some` while isearch is active.
    isearch: Option<String>,
}

impl Default for CommentInput {
    fn default() -> Self {
        Self::with_text("")
    }
}

impl CommentInput {
    /// A draft that starts from `text`, with the cursor at its end — what an
    /// edit opens with, so the existing comment is there to change rather than
    /// retype.
    pub fn with_text(text: &str) -> Self {
        let mut input = Self {
            textarea: TextArea::new(text.lines().map(String::from).collect()),
            isearch: None,
        };
        apply_style(&mut input.textarea);
        input.textarea.move_cursor(CursorMove::Bottom);
        input.textarea.move_cursor(CursorMove::End);
        input.refresh_highlights();
        input
    }

    /// Handle a key event. Returns the resulting action.
    ///
    /// - **Ctrl+Enter** (or **Ctrl+C**) submits, **Esc** cancels.
    /// - **Ctrl+Space** sets the mark, **Alt+W** copies the region.
    /// - **Ctrl+S** starts emacs-style incremental search (see [`Self::handle_isearch_key`]).
    /// - Everything else is delegated to `tui-textarea`, whose default emacs
    ///   bindings stay intact — including Enter (newline), Ctrl+J (kill to start
    ///   of line), Ctrl+K (kill to end), Ctrl+U/Ctrl+R (undo/redo), Ctrl+W (kill word).
    pub fn handle_key(&mut self, key: &KeyEvent) -> InputAction {
        if self.isearch.is_some() {
            return self.handle_isearch_key(key);
        }
        if key.code == KeyCode::Esc {
            return InputAction::Cancel;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                // Ctrl+C is the fallback: terminals without keyboard enhancement
                // report Ctrl+Enter as a plain Enter, which inserts a newline.
                KeyCode::Enter | KeyCode::Char('c') => return InputAction::Submit,
                KeyCode::Char('s') => {
                    self.start_isearch();
                    return InputAction::Continue;
                }
                // Ctrl+Space: set the mark, or drop it if one is already set.
                KeyCode::Char(' ') | KeyCode::Null => {
                    if self.textarea.is_selecting() {
                        self.textarea.cancel_selection();
                    } else {
                        self.textarea.start_selection();
                    }
                    return InputAction::Continue;
                }
                _ => {}
            }
        }
        // Alt+W: copy the region into the yank buffer (paste with Ctrl+Y).
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Char('w') {
            self.textarea.copy();
            self.textarea.cancel_selection();
            return InputAction::Continue;
        }

        let key = if self.textarea.is_selecting() {
            extend_selection(key)
        } else {
            *key
        };
        self.textarea.input(key);
        self.refresh_highlights();
        InputAction::Continue
    }

    /// Keys while incremental search is active:
    ///
    /// - **printable chars** extend the query and jump to the nearest match
    /// - **Ctrl+S** / **Ctrl+R** step to the next match forward / backward
    /// - **Backspace** shortens the query
    /// - **anything else** ends the search, leaving the cursor on the match, and is
    ///   then handled as a normal key (emacs isearch behavior)
    fn handle_isearch_key(&mut self, key: &KeyEvent) -> InputAction {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('s') if ctrl => {
                self.textarea.search_forward(false);
            }
            KeyCode::Char('r') if ctrl => {
                self.textarea.search_back(false);
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                let query = self.isearch.get_or_insert_with(String::new);
                query.push(c);
                self.apply_isearch(true);
            }
            KeyCode::Backspace => {
                if let Some(query) = self.isearch.as_mut() {
                    query.pop();
                }
                self.apply_isearch(true);
            }
            _ => {
                self.end_isearch();
                // A bare Esc or Enter only dismisses the search; every other key
                // (Ctrl+Enter included) carries on as a normal edit.
                let dismiss_only =
                    matches!(key.code, KeyCode::Esc | KeyCode::Enter) && key.modifiers.is_empty();
                if !dismiss_only {
                    return self.handle_key(key);
                }
            }
        }
        InputAction::Continue
    }

    fn start_isearch(&mut self) {
        self.isearch = Some(String::new());
    }

    fn end_isearch(&mut self) {
        self.isearch = None;
        let _ = self.textarea.set_search_pattern("");
    }

    /// Push the current query to the textarea as a literal (non-regex) pattern.
    fn apply_isearch(&mut self, match_cursor: bool) {
        let Some(query) = self.isearch.clone() else {
            return;
        };
        // The textarea searches by regex; isearch is literal, so escape the query.
        if self
            .textarea
            .set_search_pattern(escape_regex(&query))
            .is_ok()
        {
            self.textarea.search_forward(match_cursor);
        }
    }

    /// The isearch prompt to show in place of the title, if searching.
    pub fn isearch_prompt(&self) -> Option<String> {
        self.isearch.as_ref().map(|q| format!("I-search: {q}"))
    }

    pub fn is_searching(&self) -> bool {
        self.isearch.is_some()
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
        self.refresh_highlights();
    }

    /// Paint `@user`, `#123` and `!456` references so you can see what will
    /// resolve before submitting. Reruns after each key to follow the text.
    fn refresh_highlights(&mut self) {
        let spans: Vec<((usize, usize), (usize, usize))> = self
            .textarea
            .lines()
            .iter()
            .enumerate()
            .flat_map(|(row, line)| {
                reference_spans(line).map(move |(start, end)| ((row, start), (row, end)))
            })
            .collect();

        self.textarea.clear_custom_highlight();
        let style = styles::overlay_text_style()
            .bg(styles::overlay())
            .fg(styles::cyan());
        for span in spans {
            self.textarea.custom_highlight(span, style, 10);
        }
    }
}

/// With a mark set, movement extends the region: the textarea only keeps a
/// selection alive across *shifted* movement, while emacs keeps it across any.
fn extend_selection(key: &KeyEvent) -> KeyEvent {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let moves = matches!(
        key.code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
    ) || (ctrl
        && matches!(
            key.code,
            KeyCode::Char('f' | 'b' | 'n' | 'p' | 'a' | 'e' | 'v')
        ))
        || (alt && matches!(key.code, KeyCode::Char('f' | 'b' | 'v' | '<' | '>')));

    let mut key = *key;
    if moves {
        key.modifiers |= KeyModifiers::SHIFT;
    }
    key
}

/// Escape a literal string for use as a regex pattern.
fn escape_regex(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for c in query.chars() {
        if c.is_ascii() && !c.is_alphanumeric() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Byte spans of `@user` / `#123` / `!456` references within one line.
fn reference_spans(line: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    line.split_whitespace().filter_map(|word| {
        if word.strip_prefix(['@', '#', '!'])?.is_empty() {
            return None;
        }
        let start = word.as_ptr() as usize - line.as_ptr() as usize;
        Some((start, start + word.len()))
    })
}

fn apply_style(textarea: &mut TextArea<'_>) {
    // Soft-wrap long comments instead of scrolling sideways, and let Ctrl+Z undo
    // a burst of typing rather than one character.
    textarea.set_wrap_mode(WrapMode::Word);
    textarea.set_undo_coalescing(true);
    textarea.set_placeholder_text("C-⏎ submit · C-s search · C-space mark · M-w copy");
    textarea.set_placeholder_style(
        ratatui::style::Style::default()
            .fg(styles::overlay_text_dim())
            .bg(styles::overlay()),
    );

    let text_style = styles::overlay_text_style().bg(styles::overlay());
    textarea.set_style(text_style);
    textarea.set_cursor_line_style(text_style);
    textarea.set_selection_style(
        ratatui::style::Style::default()
            .fg(styles::text_bright())
            .bg(styles::highlight()),
    );
    textarea.set_search_style(
        ratatui::style::Style::default()
            .fg(styles::overlay())
            .bg(styles::cyan()),
    );
    textarea.set_cursor_style(
        ratatui::style::Style::default()
            .fg(styles::overlay())
            .bg(styles::overlay_text()),
    );
}

pub fn render(frame: &mut Frame, area: Rect, input: &mut CommentInput, title: &str) {
    let title = input.isearch_prompt().unwrap_or_else(|| title.to_string());
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

    fn type_all(input: &mut CommentInput, text: &str) {
        for c in text.chars() {
            input.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn mark_copy_and_paste_round_trip() {
        let mut input = CommentInput::default();
        type_all(&mut input, "hello");
        // Ctrl+Space at the end, select back over "llo", Alt+W to copy it.
        input.handle_key(&ctrl(' '));
        for _ in 0..3 {
            input.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }
        input.handle_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
        assert_eq!(input.text(), "hello", "copying must not edit the text");

        // Ctrl+Y pastes the yanked region at the cursor.
        input.handle_key(&ctrl('y'));
        assert_eq!(input.text(), "hellollo");
    }

    #[test]
    fn ctrl_enter_submits_while_plain_enter_inserts_a_newline() {
        let mut input = CommentInput::default();
        type_all(&mut input, "hi");
        input.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(input.text(), "hi\n");
        assert!(matches!(
            input.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            InputAction::Submit
        ));
    }

    #[test]
    fn isearch_jumps_between_matches_and_escapes_the_query() {
        let mut input = CommentInput::default();
        type_all(&mut input, "a c++ b");
        input.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        type_all(&mut input, "and c++ again");

        input.handle_key(&ctrl('s'));
        assert_eq!(input.isearch_prompt().as_deref(), Some("I-search: "));
        // `+` is a regex metacharacter: searching is literal, so this must match.
        type_all(&mut input, "c++");
        assert_eq!(input.cursor_byte_pos(), 2);

        // Ctrl+S again steps to the next match, and Esc leaves the cursor there.
        input.handle_key(&ctrl('s'));
        assert_eq!(input.cursor_byte_pos(), 12);
        input.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!input.is_searching());
        assert_eq!(input.cursor_byte_pos(), 12);
        assert_eq!(input.text(), "a c++ b\nand c++ again");
    }

    #[test]
    fn references_are_spanned_and_bare_triggers_are_not() {
        let line = "cc @john.doe about #42 and ! alone";
        let spans: Vec<&str> = reference_spans(line).map(|(s, e)| &line[s..e]).collect();
        assert_eq!(spans, ["@john.doe", "#42"]);
    }

    #[test]
    fn replace_before_cursor_swaps_query_for_completion() {
        let mut input = CommentInput::default();
        type_all(&mut input, "hi @jo");
        input.replace_before_cursor(2, "john.doe");
        assert_eq!(input.text(), "hi @john.doe ");
        assert_eq!(input.cursor_byte_pos(), input.text().len());
    }
}
