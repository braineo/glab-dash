use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use tui_textarea::{CursorMove, TextArea, WrapMode};

use crate::ui::styles;

#[derive(Debug, PartialEq, Eq)]
pub enum CommentTarget {
    NewThread,
    Reply(String),
    Edit(u64),
}

pub enum InputAction {
    Submit,
    Cancel,
    Continue,
}

pub struct CommentInput {
    textarea: TextArea<'static>,
    /// `Some` while isearch is active.
    isearch: Option<String>,
}

impl Default for CommentInput {
    fn default() -> Self {
        Self::with_text("")
    }
}

impl CommentInput {
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

    /// Ctrl+Enter submits, Esc cancels, Ctrl+Space marks, Alt+W copies and
    /// Ctrl+S starts isearch.  Everything else goes to `tui-textarea`, whose
    /// emacs bindings stay intact.
    pub fn handle_key(&mut self, key: &KeyEvent) -> InputAction {
        if self.isearch.is_some() {
            return self.handle_isearch_key(key);
        }
        if key.code == KeyCode::Esc {
            return InputAction::Cancel;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Enter => return InputAction::Submit,
                KeyCode::Char('s') => {
                    self.start_isearch();
                    return InputAction::Continue;
                }
                // Set the mark, or drop one already set.
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
        // Copy the region into the yank buffer, pasted with Ctrl+Y.
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

    /// Ctrl+S and Ctrl+R step between matches; anything else ends the search,
    /// leaves the cursor on the match, and is then handled as a normal key.
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
                // A bare Esc or Enter only dismisses the search; every other
                // key, Ctrl+Enter included, carries on as a normal edit.
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

    fn apply_isearch(&mut self, match_cursor: bool) {
        let Some(query) = self.isearch.clone() else {
            return;
        };
        // The textarea searches by regex; isearch is literal.
        if self
            .textarea
            .set_search_pattern(escape_regex(&query))
            .is_ok()
        {
            self.textarea.search_forward(match_cursor);
        }
    }

    pub fn isearch_prompt(&self) -> Option<String> {
        self.isearch.as_ref().map(|q| format!("I-search: {q}"))
    }

    pub fn is_searching(&self) -> bool {
        self.isearch.is_some()
    }

    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Into the flat text [`Self::text`] returns, not into a single line.
    pub fn cursor_byte_pos(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let mut pos: usize = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == row {
                // `col` is a character index.
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

    /// Appends a space afterwards.  `chars` counts the trigger character along
    /// with the query, since a completion brings its own sigil.
    pub fn replace_before_cursor(&mut self, chars: usize, insert: &str) {
        for _ in 0..chars {
            self.textarea.delete_char();
        }
        self.textarea.insert_str(format!("{insert} "));
        self.refresh_highlights();
    }

    /// Reruns after each key, so the painting follows the text.
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

/// The textarea only keeps a selection alive across *shifted* movement, emacs
/// across any.
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

fn reference_spans(line: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    line.split_whitespace().filter_map(|word| {
        if word.split_once(['@', '#', '!'])?.1.is_empty() {
            return None;
        }
        let start = word.as_ptr() as usize - line.as_ptr() as usize;
        Some((start, start + word.len()))
    })
}

fn apply_style(textarea: &mut TextArea<'_>) {
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
        input.handle_key(&ctrl(' '));
        for _ in 0..3 {
            input.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        }
        input.handle_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
        assert_eq!(input.text(), "hello", "copying must not edit the text");

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
        // `+` is a regex metacharacter, and searching is literal.
        type_all(&mut input, "c++");
        assert_eq!(input.cursor_byte_pos(), 2);

        input.handle_key(&ctrl('s'));
        assert_eq!(input.cursor_byte_pos(), 12);
        input.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!input.is_searching());
        assert_eq!(input.cursor_byte_pos(), 12);
        assert_eq!(input.text(), "a c++ b\nand c++ again");
    }

    #[test]
    fn references_are_spanned_and_bare_triggers_are_not() {
        let line = "cc @john.doe about team/app#42 and ! alone";
        let spans: Vec<&str> = reference_spans(line).map(|(s, e)| &line[s..e]).collect();
        assert_eq!(spans, ["@john.doe", "team/app#42"]);
    }

    #[test]
    fn replace_before_cursor_swaps_query_for_completion() {
        let mut input = CommentInput::default();
        type_all(&mut input, "hi @jo");
        input.replace_before_cursor(3, "@john.doe");
        assert_eq!(input.text(), "hi @john.doe ");
        assert_eq!(input.cursor_byte_pos(), input.text().len());

        let mut input = CommentInput::default();
        type_all(&mut input, "see #42");
        input.replace_before_cursor(3, "team/app#42");
        assert_eq!(input.text(), "see team/app#42 ");
    }
}
