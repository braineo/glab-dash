use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::ui::styles;

/// Home-row keys used as chord codes for sequential generation (9 keys).
const CHORD_KEYS: &[char] = &['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];

/// Divider sentinel — labels starting with this are rendered as separator lines.
pub const DIVIDER: &str = "───";

/// Section header sentinel — labels starting with this are rendered as bold titles.
pub const HEADER: &str = "§ ";

/// Dim color for inactive/non-matching chord items (against overlay bg #343b58).
pub const CHORD_DIM: Color = Color::Rgb(80, 87, 120);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ChordKind {
    #[default]
    Generic,
    Status,
}

pub struct ChordState {
    pub title: String,
    pub options: Vec<(String, String)>, // (code, label)
    pub input: String,
    /// Longest code length (for display alignment).
    pub max_code_len: usize,
    pub kind: ChordKind,
}

pub enum ChordAction {
    Continue,
    Selected(String), // the label value
    Cancel,
}

impl ChordState {
    /// Create a chord with cascading name-derived keys (avy/easymotion style).
    ///
    /// - Unique first letter → 1-char code (instant select)
    /// - Shared first letter → 2-char codes: first letter + distinguishing char
    /// - Divider labels (starting with `DIVIDER`) get empty codes and render as separators.
    pub fn new_for_names(title: &str, labels: Vec<String>) -> Self {
        // Separate real labels from dividers/headers for code generation
        let real_labels: Vec<String> = labels
            .iter()
            .filter(|l| !l.starts_with(DIVIDER) && !l.starts_with(HEADER))
            .cloned()
            .collect();
        let real_codes = generate_name_codes(&real_labels);
        let mut real_iter = real_codes.into_iter();

        let options: Vec<(String, String)> = labels
            .into_iter()
            .map(|l| {
                if l.starts_with(DIVIDER) || l.starts_with(HEADER) {
                    (String::new(), l)
                } else {
                    (real_iter.next().unwrap_or_default(), l)
                }
            })
            .collect();

        let max_code_len = options.iter().map(|(c, _)| c.len()).max().unwrap_or(1);
        Self {
            title: title.to_string(),
            options,
            input: String::new(),
            max_code_len,
            kind: ChordKind::Generic,
        }
    }

    /// Create a chord with pre-computed (code, label) pairs.
    pub fn from_options(title: &str, options: Vec<(String, String)>, max_code_len: usize) -> Self {
        Self {
            title: title.to_string(),
            options,
            input: String::new(),
            max_code_len,
            kind: ChordKind::Generic,
        }
    }

    pub fn with_kind(mut self, kind: ChordKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> ChordAction {
        match key.code {
            KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                let c = c.to_ascii_lowercase();
                let mut test = self.input.clone();
                test.push(c);

                // Exact code match → select immediately
                if let Some((_, label)) = self.options.iter().find(|(code, _)| *code == test) {
                    return ChordAction::Selected(label.clone());
                }
                // Valid prefix → narrow candidates
                if self.options.iter().any(|(code, _)| code.starts_with(&test)) {
                    self.input = test;
                    return ChordAction::Continue;
                }
                // No match → avy-style: silently ignore wrong letter
                ChordAction::Continue
            }
            KeyCode::Backspace if !self.input.is_empty() => {
                self.input.pop();
                ChordAction::Continue
            }
            _ => ChordAction::Cancel,
        }
    }
}

/// Generate sequential home-row codes (a, s, d, ... or aa, as, ad, ...).
fn generate_codes(count: usize, code_len: usize) -> Vec<String> {
    if code_len == 1 {
        CHORD_KEYS[..count]
            .iter()
            .map(ToString::to_string)
            .collect()
    } else {
        let mut codes = Vec::with_capacity(count);
        'outer: for &first in CHORD_KEYS {
            for &second in CHORD_KEYS {
                codes.push(format!("{first}{second}"));
                if codes.len() >= count {
                    break 'outer;
                }
            }
        }
        codes
    }
}

/// Generate cascading name-derived chord codes (for labels/assignees).
///
/// Names with a unique first letter get a 1-char code.
/// Names sharing a first letter ALL get 2-char codes: shared letter +
/// a distinguishing character derived from the name. This avoids the
/// problem where a 1-char code blocks selection of longer prefixes.
pub fn generate_name_codes(labels: &[String]) -> Vec<String> {
    let n = labels.len();
    if n == 0 {
        return Vec::new();
    }
    // Fall back to sequential for very large sets
    if n > 26 {
        return generate_codes(n, 2);
    }

    let (alpha_chars, groups, first_letter_of) = group_by_first_letter(labels);

    let mut codes = vec![String::new(); n];

    for group in &groups {
        if group.len() == 1 {
            // Unique first letter → single-char code
            codes[group[0]] = first_letter_of[group[0]].to_string();
        } else {
            // Shared first letter: ALL items get 2-char codes so that
            // typing the first letter narrows candidates instead of
            // immediately selecting one.
            let first = first_letter_of[group[0]];

            let mut used_second: Vec<char> = Vec::new();
            for &idx in group {
                let mut assigned = false;
                for &c in alpha_chars[idx].iter().skip(1) {
                    if !used_second.contains(&c) {
                        codes[idx] = format!("{first}{c}");
                        used_second.push(c);
                        assigned = true;
                        break;
                    }
                }
                if !assigned {
                    for c in 'a'..='z' {
                        if !used_second.contains(&c) {
                            codes[idx] = format!("{first}{c}");
                            used_second.push(c);
                            break;
                        }
                    }
                }
            }
        }
    }

    codes
}

/// Generate priority-aware single-char chord codes (for statuses).
///
/// Every label gets a single-char code when possible (≤ 26 items).
/// Labels with a unique first letter claim it directly. When multiple
/// labels share a first letter, the first one (by input order / priority)
/// keeps it and the rest are reassigned to a unique character derived
/// from later letters in their name.
pub fn generate_priority_codes(labels: &[String]) -> Vec<String> {
    let n = labels.len();
    if n == 0 {
        return Vec::new();
    }
    if n > 26 {
        return generate_codes(n, 2);
    }

    let (alpha_chars, groups, first_letter_of) = group_by_first_letter(labels);

    let mut codes = vec![String::new(); n];
    let mut used: Vec<char> = Vec::new();

    // Pass 1: assign first-letter codes to unique groups and primaries
    for group in &groups {
        let first = first_letter_of[group[0]];
        let primary = *group.iter().min().unwrap_or(&group[0]);
        codes[primary] = first.to_string();
        used.push(first);
    }

    // Pass 2: displaced items get a single unique char from their name
    for group in &groups {
        if group.len() == 1 {
            continue;
        }
        let primary = *group.iter().min().unwrap_or(&group[0]);
        for &idx in group {
            if idx == primary {
                continue;
            }
            let mut assigned = false;
            for &c in alpha_chars[idx].iter().skip(1) {
                if !used.contains(&c) {
                    codes[idx] = c.to_string();
                    used.push(c);
                    assigned = true;
                    break;
                }
            }
            if !assigned {
                for c in 'a'..='z' {
                    if !used.contains(&c) {
                        codes[idx] = c.to_string();
                        used.push(c);
                        break;
                    }
                }
            }
        }
    }

    codes
}

/// Shared helper: extract alpha chars and group label indices by first letter.
fn group_by_first_letter(labels: &[String]) -> (Vec<Vec<char>>, Vec<Vec<usize>>, Vec<char>) {
    let n = labels.len();
    let alpha_chars: Vec<Vec<char>> = labels
        .iter()
        .map(|l| {
            l.chars()
                .filter(char::is_ascii_alphabetic)
                .map(|c| c.to_ascii_lowercase())
                .collect()
        })
        .collect();

    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut first_letter_of: Vec<char> = vec!['a'; n];
    {
        let mut pairs: Vec<(char, usize)> = alpha_chars
            .iter()
            .enumerate()
            .filter_map(|(i, chars)| chars.first().map(|&c| (c, i)))
            .collect();
        pairs.sort_by_key(|&(c, _)| c);

        let mut i = 0;
        while i < pairs.len() {
            let ch = pairs[i].0;
            let mut group = Vec::new();
            while i < pairs.len() && pairs[i].0 == ch {
                let idx = pairs[i].1;
                first_letter_of[idx] = ch;
                group.push(idx);
                i += 1;
            }
            groups.push(group);
        }
    }

    (alpha_chars, groups, first_letter_of)
}

// ── Rendering ──

pub fn render(frame: &mut Frame, area: Rect, state: &ChordState) {
    let has_sections = state
        .options
        .iter()
        .any(|(_, l)| l.starts_with(HEADER) || l.starts_with(DIVIDER));

    if has_sections {
        render_sectioned(frame, area, state);
    } else {
        render_grid(frame, area, state);
    }
}

/// Render as a multi-column grid (default for simple chord lists).
fn render_grid(frame: &mut Frame, area: Rect, state: &ChordState) {
    let max_label_len = state
        .options
        .iter()
        .map(|(_, l)| l.len())
        .max()
        .unwrap_or(6);
    // Status icons add "◌ " prefix (2 display chars) to each label
    let icon_width = if state.kind == ChordKind::Status {
        2
    } else {
        0
    };
    let item_width = state.max_code_len + 1 + icon_width + max_label_len + 2;
    let usable_width = usize::from(area.width).saturating_sub(6);
    let cols = (usable_width / item_width).clamp(1, 4);
    let rows = state.options.len().div_ceil(cols);

    let popup_width = u16::try_from(item_width * cols + 4)
        .unwrap_or(u16::MAX)
        .min(area.width.saturating_sub(2));
    let popup_height = u16::try_from(rows + 3)
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(2));

    let popup = centered_rect(popup_width, popup_height, area);
    frame.render_widget(Clear, popup);

    let block = styles::overlay_block(&state.title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::new();
    let typed_len = state.input.len();

    for row_idx in 0..rows {
        let mut spans = Vec::new();
        for col_idx in 0..cols {
            let item_idx = col_idx * rows + row_idx; // column-major
            if item_idx < state.options.len() {
                let (code, label) = &state.options[item_idx];
                let is_active = state.input.is_empty() || code.starts_with(&state.input);

                // ── Code: avy-style progressive highlight ──
                render_code(&mut spans, code, state.max_code_len, typed_len, is_active);
                spans.push(Span::raw(" "));

                // ── Label ──
                if state.kind == ChordKind::Status && is_active {
                    let icon = styles::status_icon(label);
                    let sty = styles::status_style(label);
                    let padded = format!("{icon} {label:<w$}", w = max_label_len + 2);
                    spans.push(Span::styled(padded, sty));
                } else {
                    let label_style = if is_active {
                        styles::overlay_text_style()
                    } else {
                        Style::default().fg(CHORD_DIM)
                    };
                    let prefix = if state.kind == ChordKind::Status {
                        "  "
                    } else {
                        ""
                    };
                    let padded = format!("{prefix}{label:<w$}", w = max_label_len + 2);
                    spans.push(Span::styled(padded, label_style));
                }
            }
        }
        lines.push(Line::from(spans));
    }

    // ── Hint line ──
    lines.push(render_hint(state, typed_len));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render as a single-column sectioned list with headers and dividers.
fn render_sectioned(frame: &mut Frame, area: Rect, state: &ChordState) {
    let max_label_len = state
        .options
        .iter()
        .filter(|(c, _)| !c.is_empty())
        .map(|(_, l)| l.len())
        .max()
        .unwrap_or(6);
    let item_width = state.max_code_len + 1 + max_label_len + 2;

    let popup_width = u16::try_from(item_width + 4)
        .unwrap_or(u16::MAX)
        .min(area.width.saturating_sub(2));
    let popup_height = u16::try_from(state.options.len() + 3)
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(2));

    let popup = centered_rect(popup_width, popup_height, area);
    frame.render_widget(Clear, popup);

    let block = styles::overlay_block(&state.title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::new();
    let typed_len = state.input.len();

    for (code, label) in &state.options {
        // Section header
        if let Some(title) = label.strip_prefix(HEADER) {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    title.to_string(),
                    Style::default()
                        .fg(styles::TEXT_DIM)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            continue;
        }

        // Divider
        if label.starts_with(DIVIDER) {
            let line = "─".repeat(item_width);
            lines.push(Line::from(Span::styled(
                line,
                Style::default().fg(CHORD_DIM),
            )));
            continue;
        }

        // Normal item
        let is_active = state.input.is_empty() || code.starts_with(&state.input);
        let mut spans = Vec::new();
        render_code(&mut spans, code, state.max_code_len, typed_len, is_active);
        spans.push(Span::raw(" "));

        let label_style = if is_active {
            styles::overlay_text_style()
        } else {
            Style::default().fg(CHORD_DIM)
        };
        spans.push(Span::styled(label.clone(), label_style));
        lines.push(Line::from(spans));
    }

    // ── Hint line ──
    lines.push(render_hint(state, typed_len));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render a chord code with avy-style progressive dimming/highlighting.
///
/// Active + partial input: typed prefix muted, remaining chars bright.
/// Active + no input: full code in accent color.
/// Inactive: fully dimmed.
pub fn render_code(
    spans: &mut Vec<Span<'static>>,
    code: &str,
    max_width: usize,
    typed: usize,
    active: bool,
) {
    // Right-align padding for variable-length codes
    let pad = max_width.saturating_sub(code.len());
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }

    if active && typed > 0 {
        // Typed prefix → muted; remaining → bright hint target
        spans.push(Span::styled(
            code[..typed].to_string(),
            Style::default().fg(styles::OVERLAY_TEXT_DIM),
        ));
        spans.push(Span::styled(
            code[typed..].to_string(),
            Style::default()
                .fg(styles::YELLOW)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        let sty = if active {
            Style::default()
                .fg(styles::MAGENTA)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CHORD_DIM)
        };
        spans.push(Span::styled(code.to_string(), sty));
    }
}

/// Render the bottom hint line showing typing progress and key hints.
fn render_hint(state: &ChordState, typed: usize) -> Line<'static> {
    if state.input.is_empty() {
        return Line::from(vec![
            Span::styled("Esc", styles::overlay_key_style()),
            Span::styled(" cancel", styles::overlay_desc_style()),
        ]);
    }
    let remaining_dots = state.max_code_len.saturating_sub(typed);
    Line::from(vec![
        Span::styled(
            state.input.clone(),
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
    ])
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(r.width), height.min(r.height))
}
