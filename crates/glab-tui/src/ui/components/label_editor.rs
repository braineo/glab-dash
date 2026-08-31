use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::chord_popup;
use crate::ui::{keys, styles};

// ── Public types ──

pub enum LabelEditorAction {
    Continue,
    Confirmed(Vec<String>),
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelEditorMode {
    Chord,
    Search,
}

pub struct LabelEditorState {
    all_labels: Vec<String>,
    selected: Vec<bool>,
    pub mode: LabelEditorMode,
    /// Indices into `all_labels` shown in chord view.
    pinned: Vec<usize>,
    /// Chord codes parallel to `pinned`.
    chord_codes: Vec<String>,
    max_code_len: usize,
    chord_input: String,
    search_query: String,
    search_filtered: Vec<usize>,
    search_cursor: usize,
    matcher: SkimMatcherV2,
}

// ── Construction ──

impl LabelEditorState {
    pub fn new(
        all_labels: Vec<String>,
        current_labels: &[String],
        label_usage: &HashMap<String, u32>,
        issue_labels: &[Vec<String>],
        max_pinned: usize,
    ) -> Self {
        let selected: Vec<bool> = all_labels
            .iter()
            .map(|l| current_labels.contains(l))
            .collect();

        let pinned = select_pinned(
            &all_labels,
            current_labels,
            label_usage,
            issue_labels,
            max_pinned,
        );

        let pinned_names: Vec<String> = pinned.iter().map(|&i| all_labels[i].clone()).collect();
        let chord_codes = chord_popup::generate_name_codes(&pinned_names);
        let max_code_len = chord_codes.iter().map(String::len).max().unwrap_or(1);

        Self {
            all_labels,
            selected,
            mode: LabelEditorMode::Chord,
            pinned,
            chord_codes,
            max_code_len,
            chord_input: String::new(),
            search_query: String::new(),
            search_filtered: Vec::new(),
            search_cursor: 0,
            matcher: SkimMatcherV2::default(),
        }
    }

    // ── Input handling ──

    pub fn handle_key(&mut self, key: &KeyEvent) -> LabelEditorAction {
        match self.mode {
            LabelEditorMode::Chord => self.handle_chord_key(key),
            LabelEditorMode::Search => self.handle_search_key(key),
        }
    }

    fn handle_chord_key(&mut self, key: &KeyEvent) -> LabelEditorAction {
        match key.code {
            KeyCode::Char('/') => {
                self.mode = LabelEditorMode::Search;
                self.search_query.clear();
                self.refilter_search();
                LabelEditorAction::Continue
            }
            KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                let c = c.to_ascii_lowercase();
                let mut test = self.chord_input.clone();
                test.push(c);

                // Exact match → toggle
                if let Some(pos) = self.chord_codes.iter().position(|code| *code == test) {
                    let label_idx = self.pinned[pos];
                    self.toggle_label(label_idx);
                    self.chord_input.clear();
                    return LabelEditorAction::Continue;
                }
                // Valid prefix → narrow
                if self.chord_codes.iter().any(|code| code.starts_with(&test)) {
                    self.chord_input = test;
                    return LabelEditorAction::Continue;
                }
                // No match → ignore
                LabelEditorAction::Continue
            }
            KeyCode::Backspace => {
                self.chord_input.pop();
                LabelEditorAction::Continue
            }
            KeyCode::Enter => {
                let labels = self.confirmed_labels();
                LabelEditorAction::Confirmed(labels)
            }
            KeyCode::Esc => LabelEditorAction::Cancel,
            _ => LabelEditorAction::Continue,
        }
    }

    fn handle_search_key(&mut self, key: &KeyEvent) -> LabelEditorAction {
        if keys::is_nav_up(key) {
            if self.search_cursor > 0 {
                self.search_cursor -= 1;
            }
            return LabelEditorAction::Continue;
        }
        if keys::is_nav_down(key) {
            if self.search_cursor + 1 < self.search_filtered.len() {
                self.search_cursor += 1;
            }
            return LabelEditorAction::Continue;
        }
        match key.code {
            KeyCode::Esc => {
                self.mode = LabelEditorMode::Chord;
                self.chord_input.clear();
                LabelEditorAction::Continue
            }
            KeyCode::Enter => {
                let maybe_idx = self.search_filtered.get(self.search_cursor).copied();
                if let Some(idx) = maybe_idx {
                    self.toggle_label(idx);
                    self.rebuild_pinned();
                }
                self.mode = LabelEditorMode::Chord;
                self.chord_input.clear();
                LabelEditorAction::Continue
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.refilter_search();
                LabelEditorAction::Continue
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.refilter_search();
                LabelEditorAction::Continue
            }
            _ => LabelEditorAction::Continue,
        }
    }

    // ── Helpers ──

    /// Rebuild pinned list to include all currently selected labels,
    /// so labels toggled via search become visible in chord mode.
    fn rebuild_pinned(&mut self) {
        for (i, &sel) in self.selected.iter().enumerate() {
            if sel && !self.pinned.contains(&i) {
                self.pinned.push(i);
            }
        }
        self.pinned.sort_by(|&a, &b| {
            label_scope(&self.all_labels[a]).cmp(label_scope(&self.all_labels[b]))
        });
        let pinned_names: Vec<String> = self
            .pinned
            .iter()
            .map(|&i| self.all_labels[i].clone())
            .collect();
        self.chord_codes = chord_popup::generate_name_codes(&pinned_names);
        self.max_code_len = self.chord_codes.iter().map(String::len).max().unwrap_or(1);
    }

    /// Toggle a label, enforcing scoped-label mutual exclusivity.
    fn toggle_label(&mut self, idx: usize) {
        if self.selected[idx] {
            self.selected[idx] = false;
            return;
        }
        // Selecting: deselect any other label with the same scope
        let label = &self.all_labels[idx];
        if let Some(scope) = label.split_once("::").map(|(s, _)| s) {
            for (i, item) in self.all_labels.iter().enumerate() {
                if i != idx
                    && self.selected[i]
                    && item.split_once("::").map(|(s, _)| s) == Some(scope)
                {
                    self.selected[i] = false;
                }
            }
        }
        self.selected[idx] = true;
    }

    fn confirmed_labels(&self) -> Vec<String> {
        self.all_labels
            .iter()
            .enumerate()
            .filter(|(i, _)| self.selected[*i])
            .map(|(_, s)| s.clone())
            .collect()
    }

    fn refilter_search(&mut self) {
        if self.search_query.is_empty() {
            self.search_filtered = (0..self.all_labels.len()).collect();
        } else {
            let mut scored: Vec<(usize, i64)> = self
                .all_labels
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    self.matcher
                        .fuzzy_match(item, &self.search_query)
                        .map(|score| (i, score))
                })
                .collect();
            scored.sort_by_key(|item| std::cmp::Reverse(item.1));
            self.search_filtered = scored.into_iter().map(|(i, _)| i).collect();
        }
        self.search_cursor = 0;
    }
}

// ── Pinned label selection ──

fn select_pinned(
    all_labels: &[String],
    current_labels: &[String],
    label_usage: &HashMap<String, u32>,
    issue_labels: &[Vec<String>],
    max_pinned: usize,
) -> Vec<usize> {
    // Currently-applied labels are always pinned
    let mut pinned: Vec<usize> = all_labels
        .iter()
        .enumerate()
        .filter(|(_, name)| current_labels.contains(name))
        .map(|(i, _)| i)
        .collect();

    // Build effective frequency: explicit usage + issue occurrence for cold start
    let effective_usage = if label_usage.is_empty() {
        // Cold start: count occurrences across open issues
        let mut counts: HashMap<&str, u32> = HashMap::new();
        for labels in issue_labels {
            for label in labels {
                *counts.entry(label.as_str()).or_insert(0) += 1;
            }
        }
        all_labels
            .iter()
            .map(|l| (l.clone(), counts.get(l.as_str()).copied().unwrap_or(0)))
            .collect::<HashMap<String, u32>>()
    } else {
        label_usage.clone()
    };

    // Sort remaining by usage count descending
    let mut by_usage: Vec<(usize, u32)> = all_labels
        .iter()
        .enumerate()
        .filter(|(i, _)| !pinned.contains(i))
        .map(|(i, name)| (i, effective_usage.get(name).copied().unwrap_or(0)))
        .filter(|(_, count)| *count > 0)
        .collect();
    by_usage.sort_by_key(|item| std::cmp::Reverse(item.1));

    let remaining = max_pinned.saturating_sub(pinned.len());
    pinned.extend(by_usage.iter().take(remaining).map(|(i, _)| *i));

    // Sort so labels with the same scope (e.g. priority::high, priority::low)
    // are grouped together. Within a scope group, preserve original order.
    pinned.sort_by(|&a, &b| {
        let scope_a = label_scope(&all_labels[a]);
        let scope_b = label_scope(&all_labels[b]);
        scope_a.cmp(scope_b)
    });

    pinned
}

/// Extract the scope prefix of a scoped label, or the full name for unscoped labels.
fn label_scope(label: &str) -> &str {
    label.split_once("::").map_or(label, |(scope, _)| scope)
}

// ── Rendering ──

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &LabelEditorState,
    label_colors: &styles::LabelColors,
) {
    match state.mode {
        LabelEditorMode::Chord => render_chord_mode(frame, area, state, label_colors),
        LabelEditorMode::Search => render_search_mode(frame, area, state, label_colors),
    }
}

fn render_chord_mode(
    frame: &mut Frame,
    area: Rect,
    state: &LabelEditorState,
    label_colors: &styles::LabelColors,
) {
    if state.pinned.is_empty() {
        // No pinned labels — show a hint to search
        let popup = centered_rect(40, 20, area);
        frame.render_widget(Clear, popup);
        let block = styles::overlay_block("Labels");
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let lines = vec![
            Line::from(Span::styled(
                "No frequent labels yet",
                styles::overlay_desc_style(),
            )),
            Line::from(vec![
                Span::styled("/", styles::overlay_key_style()),
                Span::styled(" search all labels", styles::overlay_desc_style()),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    // Measure widths for grid layout
    let max_label_display: usize = state
        .pinned
        .iter()
        .map(|&i| label_display_width(&state.all_labels[i]))
        .max()
        .unwrap_or(8);
    // checkbox(2) + space + code(max_code_len) + space + label + padding
    let item_width = 2 + 1 + state.max_code_len + 1 + max_label_display + 2;
    let usable_width = usize::from(area.width).saturating_sub(6);
    let cols = (usable_width / item_width).clamp(1, 4);
    let rows = state.pinned.len().div_ceil(cols);

    let popup_width = u16::try_from(item_width * cols + 4)
        .unwrap_or(u16::MAX)
        .min(area.width.saturating_sub(2));
    let popup_height = u16::try_from(rows + 4)
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(2));

    let popup = centered_rect_abs(popup_width, popup_height, area);
    frame.render_widget(Clear, popup);

    let block = styles::overlay_block("Labels");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let typed_len = state.chord_input.len();
    let mut lines = Vec::new();

    for row_idx in 0..rows {
        let mut spans = Vec::new();
        for col_idx in 0..cols {
            let pin_idx = col_idx * rows + row_idx; // column-major
            if pin_idx < state.pinned.len() {
                let label_idx = state.pinned[pin_idx];
                let code = &state.chord_codes[pin_idx];
                let label = &state.all_labels[label_idx];
                let is_selected = state.selected[label_idx];
                let is_active =
                    state.chord_input.is_empty() || code.starts_with(&state.chord_input);

                // Checkbox
                let (icon, icon_style) = if is_selected {
                    (
                        styles::ICON_CHECK,
                        if is_active {
                            styles::source_tracking_style()
                        } else {
                            Style::default().fg(chord_popup::CHORD_DIM)
                        },
                    )
                } else {
                    (
                        styles::ICON_UNCHECK,
                        if is_active {
                            styles::overlay_desc_style()
                        } else {
                            Style::default().fg(chord_popup::CHORD_DIM)
                        },
                    )
                };
                spans.push(Span::styled(format!("{icon} "), icon_style));

                // Chord code (avy-style)
                chord_popup::render_code(
                    &mut spans,
                    code,
                    state.max_code_len,
                    typed_len,
                    is_active,
                );
                spans.push(Span::raw(" "));

                // Label chip
                if is_active {
                    let color = label_colors.get(label).map(String::as_str);
                    spans.extend(styles::label_spans(label, color));
                    // Pad after chip
                    let chip_w = label_display_width(label);
                    let pad = max_label_display.saturating_sub(chip_w) + 1;
                    spans.push(Span::raw(" ".repeat(pad)));
                } else {
                    let padded = format!("{label:<w$}", w = max_label_display + 2);
                    spans.push(Span::styled(
                        padded,
                        Style::default().fg(chord_popup::CHORD_DIM),
                    ));
                }
            }
        }
        lines.push(Line::from(spans));
    }

    // Hint line
    let hint_spans = if state.chord_input.is_empty() {
        vec![
            Span::styled("/", styles::overlay_key_style()),
            Span::styled(" search  ", styles::overlay_desc_style()),
            Span::styled("Enter", styles::overlay_key_style()),
            Span::styled(" apply  ", styles::overlay_desc_style()),
            Span::styled("Esc", styles::overlay_key_style()),
            Span::styled(" cancel", styles::overlay_desc_style()),
        ]
    } else {
        let remaining_dots = state.max_code_len.saturating_sub(typed_len);
        vec![
            Span::styled(
                state.chord_input.clone(),
                Style::default()
                    .fg(styles::MAGENTA)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "\u{00B7}".repeat(remaining_dots),
                Style::default()
                    .fg(styles::YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Bksp", styles::overlay_key_style()),
            Span::styled(" undo  ", styles::overlay_desc_style()),
            Span::styled("Esc", styles::overlay_key_style()),
            Span::styled(" cancel", styles::overlay_desc_style()),
        ]
    };
    lines.push(Line::from(vec![])); // spacer
    lines.push(Line::from(hint_spans));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_search_mode(
    frame: &mut Frame,
    area: Rect,
    state: &LabelEditorState,
    label_colors: &styles::LabelColors,
) {
    let popup = centered_rect(50, 60, area);
    frame.render_widget(Clear, popup);

    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(popup);

    // Search input
    let search_block = styles::overlay_block("Labels (search)");
    let search_text = if state.search_query.is_empty() {
        Span::styled(
            "Type to filter...",
            styles::overlay_desc_style().add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(&state.search_query, styles::overlay_text_style())
    };
    let search = Paragraph::new(Line::from(search_text)).block(search_block);
    frame.render_widget(search, chunks[0]);

    // Filtered list (rendered as Paragraph, no List highlight_style)
    let visible_height = usize::from(chunks[1].height).saturating_sub(3); // borders + hint
    let scroll_offset = state
        .search_cursor
        .saturating_sub(visible_height.saturating_sub(1));

    let mut lines = Vec::new();
    for (vi, &idx) in state.search_filtered.iter().enumerate().skip(scroll_offset) {
        if lines.len() >= visible_height {
            break;
        }
        let is_focused = vi == state.search_cursor;
        let label = &state.all_labels[idx];

        let mut spans = Vec::new();

        // Cursor indicator
        if is_focused {
            spans.push(Span::styled(
                "\u{25B8} ",
                Style::default()
                    .fg(styles::CYAN)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw("  "));
        }

        // Label with powerline chip
        let color = label_colors.get(label).map(String::as_str);
        spans.extend(styles::label_spans(label, color));

        lines.push(Line::from(spans));
    }

    // Hint line
    lines.push(Line::from(vec![])); // spacer
    lines.push(Line::from(vec![
        Span::styled("Enter", styles::overlay_key_style()),
        Span::styled(" select  ", styles::overlay_desc_style()),
        Span::styled("Esc", styles::overlay_key_style()),
        Span::styled(" back", styles::overlay_desc_style()),
    ]));

    let list_block = styles::overlay_block("");
    let list_inner = list_block.inner(chunks[1]);
    frame.render_widget(list_block, chunks[1]);
    frame.render_widget(Paragraph::new(lines), list_inner);
}

/// Visual width of a label chip (segments + powerline arrows).
fn label_display_width(label: &str) -> usize {
    let segments: Vec<&str> = label.split("::").collect();
    let text_w: usize = segments.iter().map(|s| s.len()).sum();
    text_w + segments.len() // each segment gets a trailing arrow
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

fn centered_rect_abs(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(r.width), height.min(r.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::collections::HashMap;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn make_state(all: &[&str], current: &[&str]) -> LabelEditorState {
        let all_labels: Vec<String> = all.iter().map(std::string::ToString::to_string).collect();
        let current_labels: Vec<String> = current
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        LabelEditorState::new(all_labels, &current_labels, &HashMap::new(), &[], 20)
    }

    #[test]
    fn chord_toggle_and_confirm() {
        // "A" and "B" are current labels → both pinned and pre-selected
        let mut state = make_state(&["A", "B", "C"], &["A", "B"]);
        assert_eq!(state.mode, LabelEditorMode::Chord);
        assert!(state.selected[0], "A starts selected");
        assert!(state.selected[1], "B starts selected");

        // Toggle first pinned label OFF (deselect "A")
        let a_pin_idx = state
            .pinned
            .iter()
            .position(|&i| state.all_labels[i] == "A")
            .unwrap();
        let code = state.chord_codes[a_pin_idx].clone();
        for c in code.chars() {
            state.handle_key(&key(KeyCode::Char(c)));
        }
        assert!(
            !state.selected[0],
            "A should be deselected after chord toggle"
        );

        // Confirm
        let action = state.handle_key(&key(KeyCode::Enter));
        match action {
            LabelEditorAction::Confirmed(labels) => {
                assert!(
                    labels.contains(&"B".to_string()),
                    "B should still be confirmed"
                );
                assert!(!labels.contains(&"A".to_string()), "A should be deselected");
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn search_space_types_into_query() {
        let mut state = make_state(&["Alpha", "Beta", "Gamma"], &[]);

        state.handle_key(&key(KeyCode::Char('/')));
        assert_eq!(state.mode, LabelEditorMode::Search);

        // Space should type into the search query, not toggle
        state.handle_key(&key(KeyCode::Char(' ')));
        assert_eq!(state.mode, LabelEditorMode::Search);
        assert_eq!(state.search_query, " ");
    }

    #[test]
    fn search_enter_selects_and_returns_to_chord() {
        let mut state = make_state(&["Alpha", "Beta", "Gamma"], &[]);

        // Enter search mode
        state.handle_key(&key(KeyCode::Char('/')));
        assert_eq!(state.mode, LabelEditorMode::Search);
        assert_eq!(state.search_filtered.len(), 3);

        // Enter on first item → toggle + back to chord (not confirm)
        let first_idx = state.search_filtered[0];
        let action = state.handle_key(&key(KeyCode::Enter));
        assert!(matches!(action, LabelEditorAction::Continue));
        assert_eq!(state.mode, LabelEditorMode::Chord);
        assert!(state.selected[first_idx], "Label should be selected");
        assert!(state.pinned.contains(&first_idx), "Label should be pinned");

        // Enter in chord mode confirms
        let action = state.handle_key(&key(KeyCode::Enter));
        match action {
            LabelEditorAction::Confirmed(labels) => {
                assert_eq!(labels.len(), 1);
                assert_eq!(labels[0], "Alpha");
            }
            _ => panic!("Expected Confirmed"),
        }
    }

    #[test]
    fn search_enter_then_search_again_for_multi_label() {
        let mut state = make_state(&["Alpha", "Beta", "Gamma"], &[]);

        // Search and select Alpha
        state.handle_key(&key(KeyCode::Char('/')));
        state.handle_key(&key(KeyCode::Enter)); // selects Alpha, back to chord
        assert_eq!(state.mode, LabelEditorMode::Chord);

        // Search again and select Beta
        state.handle_key(&key(KeyCode::Char('/')));
        assert_eq!(state.mode, LabelEditorMode::Search);
        // Navigate down to Beta
        state.handle_key(&key(KeyCode::Down));
        state.handle_key(&key(KeyCode::Enter)); // selects Beta, back to chord
        assert_eq!(state.mode, LabelEditorMode::Chord);

        // Confirm
        let action = state.handle_key(&key(KeyCode::Enter));
        match action {
            LabelEditorAction::Confirmed(labels) => {
                assert!(labels.contains(&"Alpha".to_string()));
                assert!(labels.contains(&"Beta".to_string()));
                assert!(!labels.contains(&"Gamma".to_string()));
            }
            _ => panic!("Expected Confirmed"),
        }
    }
}
