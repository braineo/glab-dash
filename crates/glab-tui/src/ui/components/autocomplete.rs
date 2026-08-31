use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

use crate::gitlab::types::{TrackedIssue, TrackedMergeRequest};
use crate::ui::styles;

const MAX_VISIBLE: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionKind {
    User,
    Issue,
    MergeRequest,
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub insert: String,
}

#[derive(Default)]
pub struct AutocompleteState {
    pub active: bool,
    pub kind: Option<CompletionKind>,
    pub trigger_pos: usize,
    pub query: String,
    pub items: Vec<CompletionItem>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    matcher: SkimMatcherV2,
}

impl AutocompleteState {
    pub fn update(
        &mut self,
        text: &str,
        cursor_byte_pos: usize,
        members: &[String],
        issues: &[TrackedIssue],
        mrs: &[TrackedMergeRequest],
    ) {
        // Scan backwards from cursor to find a trigger character
        let before = &text[..cursor_byte_pos];
        let mut found_trigger = None;

        for (i, ch) in before.char_indices().rev() {
            if ch.is_whitespace() || ch == '\n' {
                break;
            }
            if ch == '@' || ch == '#' || ch == '!' {
                found_trigger = Some((i, ch));
                break;
            }
        }

        let Some((pos, trigger_char)) = found_trigger else {
            self.dismiss();
            return;
        };

        let kind = match trigger_char {
            '@' => CompletionKind::User,
            '#' => CompletionKind::Issue,
            '!' => CompletionKind::MergeRequest,
            _ => unreachable!(),
        };

        let query = &text[pos + trigger_char.len_utf8()..cursor_byte_pos];

        // Rebuild items if kind or trigger position changed
        if self.kind.as_ref() != Some(&kind) || self.trigger_pos != pos {
            self.kind = Some(kind.clone());
            self.trigger_pos = pos;
            self.items = match kind {
                CompletionKind::User => members
                    .iter()
                    .map(|m| CompletionItem {
                        label: m.clone(),
                        insert: m.clone(),
                    })
                    .collect(),
                CompletionKind::Issue => issues
                    .iter()
                    .map(|ti| CompletionItem {
                        label: format!("{} {}", ti.issue.iid, ti.issue.title),
                        insert: ti.issue.iid.to_string(),
                    })
                    .collect(),
                CompletionKind::MergeRequest => mrs
                    .iter()
                    .map(|tm| CompletionItem {
                        label: format!("{} {}", tm.mr.iid, tm.mr.title),
                        insert: tm.mr.iid.to_string(),
                    })
                    .collect(),
            };
        }

        self.query = query.to_string();
        self.refilter();
        self.active = !self.filtered.is_empty();
    }

    fn refilter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(usize, i64)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    self.matcher
                        .fuzzy_match(&item.label, &self.query)
                        .map(|s| (i, s))
                })
                .collect();
            scored.sort_by_key(|&(_, s)| std::cmp::Reverse(s));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }
        // Truncate to reasonable size
        self.filtered.truncate(MAX_VISIBLE * 3);
        self.selected = 0;
    }

    pub fn dismiss(&mut self) {
        self.active = false;
        self.kind = None;
        self.items.clear();
        self.filtered.clear();
        self.selected = 0;
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1).min(self.filtered.len() - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Returns the selected completion item, if any.
    pub fn selected_item(&self) -> Option<&CompletionItem> {
        let idx = *self.filtered.get(self.selected)?;
        self.items.get(idx)
    }

    /// Returns the trigger character length (always 1 byte for @, #, !).
    pub fn trigger_char_len() -> usize {
        1
    }
}

pub fn render(frame: &mut Frame, input_area: Rect, state: &AutocompleteState) {
    if !state.active || state.filtered.is_empty() {
        return;
    }

    let count = state.filtered.len().min(MAX_VISIBLE);
    let height = u16::try_from(count + 2).unwrap_or(u16::MAX); // borders

    let prefix = match &state.kind {
        Some(CompletionKind::User) => "@",
        Some(CompletionKind::Issue) => "#",
        Some(CompletionKind::MergeRequest) => "!",
        None => "",
    };

    // Position dropdown below input area
    let dropdown = Rect {
        x: input_area.x + 1,
        y: input_area.y + input_area.height,
        width: input_area.width.saturating_sub(2).min(50),
        height,
    };

    frame.render_widget(Clear, dropdown);

    let items: Vec<ListItem<'_>> = state
        .filtered
        .iter()
        .take(MAX_VISIBLE)
        .enumerate()
        .map(|(vi, &idx)| {
            let item = &state.items[idx];
            let is_selected = vi == state.selected;
            let style = if is_selected {
                ratatui::style::Style::default()
                    .fg(styles::TEXT_BRIGHT)
                    .bg(styles::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                ratatui::style::Style::default().fg(styles::OVERLAY_TEXT)
            };
            let line = Line::from(vec![
                Span::styled(
                    prefix,
                    ratatui::style::Style::default().fg(styles::OVERLAY_TEXT_DIM),
                ),
                Span::styled(&item.label, style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(styles::BORDER))
        .style(ratatui::style::Style::default().bg(styles::OVERLAY));

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected));

    let list = List::new(items).block(block);
    frame.render_stateful_widget(list, dropdown, &mut list_state);
}
