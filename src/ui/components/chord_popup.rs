use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::ui::styles;

/// Home-row keys used as chord codes (9 keys: 9 single, 81 two-key combos).
const CHORD_KEYS: &[char] = &['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];

pub struct ChordState {
    pub title: String,
    pub options: Vec<(String, String)>, // (code, label)
    pub input: String,
    pub code_len: usize, // 1 or 2
}

pub enum ChordAction {
    Continue,
    Selected(String), // the label value
    Cancel,
}

impl ChordState {
    pub fn new(title: &str, labels: Vec<String>) -> Self {
        let code_len = if labels.len() <= CHORD_KEYS.len() {
            1
        } else {
            2
        };
        let codes = generate_codes(labels.len(), code_len);
        let options = codes.into_iter().zip(labels).collect();
        Self {
            title: title.to_string(),
            options,
            input: String::new(),
            code_len,
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> ChordAction {
        match key.code {
            KeyCode::Esc => ChordAction::Cancel,
            KeyCode::Char(c) => {
                if !CHORD_KEYS.contains(&c) {
                    return ChordAction::Cancel;
                }
                self.input.push(c);
                if self.input.len() >= self.code_len {
                    // Full code entered
                    if let Some((_, label)) =
                        self.options.iter().find(|(code, _)| *code == self.input)
                    {
                        ChordAction::Selected(label.clone())
                    } else {
                        // Invalid code — reset input
                        self.input.clear();
                        ChordAction::Continue
                    }
                } else {
                    // Partial code — check if any prefix matches exist
                    let has_match = self
                        .options
                        .iter()
                        .any(|(code, _)| code.starts_with(&self.input));
                    if !has_match {
                        self.input.clear();
                    }
                    ChordAction::Continue
                }
            }
            _ => ChordAction::Cancel,
        }
    }
}

fn generate_codes(count: usize, code_len: usize) -> Vec<String> {
    if code_len == 1 {
        CHORD_KEYS[..count]
            .iter()
            .map(|c| c.to_string())
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

pub fn render(frame: &mut Frame, area: Rect, state: &ChordState) {
    // Calculate column layout
    let max_label_len = state
        .options
        .iter()
        .map(|(_, l)| l.len())
        .max()
        .unwrap_or(6);
    let item_width = state.code_len + 1 + max_label_len + 2; // "aa label  "
    let usable_width = (area.width as usize).saturating_sub(6); // borders + padding
    let cols = (usable_width / item_width).clamp(1, 4);
    let rows = state.options.len().div_ceil(cols);

    let popup_width = ((item_width * cols + 4) as u16).min(area.width.saturating_sub(2));
    let popup_height = ((rows + 3) as u16).min(area.height.saturating_sub(2)); // items + border + hint

    let popup = centered_rect(popup_width, popup_height, area);
    frame.render_widget(Clear, popup);

    let block = styles::overlay_block(&state.title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::new();

    for row_idx in 0..rows {
        let mut spans = Vec::new();
        for col_idx in 0..cols {
            let item_idx = col_idx * rows + row_idx; // column-major
            if item_idx < state.options.len() {
                let (code, label) = &state.options[item_idx];
                let is_active = state.input.is_empty() || code.starts_with(&state.input);

                let code_style = if is_active {
                    Style::default()
                        .fg(styles::MAGENTA)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(styles::OVERLAY_TEXT_DIM)
                };
                let label_style = if is_active {
                    styles::overlay_text_style()
                } else {
                    Style::default().fg(styles::OVERLAY_TEXT_DIM)
                };

                spans.push(Span::styled(
                    format!("{code:>w$}", w = state.code_len),
                    code_style,
                ));
                spans.push(Span::raw(" "));
                let padded = format!("{label:<w$}", w = max_label_len + 2);
                spans.push(Span::styled(padded, label_style));
            }
        }
        lines.push(Line::from(spans));
    }

    // Hint line
    lines.push(Line::from(vec![
        if state.input.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!("{}_ ", state.input),
                Style::default()
                    .fg(styles::MAGENTA)
                    .add_modifier(Modifier::BOLD),
            )
        },
        Span::styled("Esc", styles::overlay_key_style()),
        Span::styled(":cancel", styles::overlay_desc_style()),
    ]));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(r.width), height.min(r.height))
}
