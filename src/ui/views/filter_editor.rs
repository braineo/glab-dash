use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use crate::filter::{Field, FilterCondition, Op};
use crate::ui::{keys, styles};

#[derive(Debug, Clone, PartialEq)]
pub enum EditorStep {
    SelectField,
    SelectOp,
    EnterValue,
}

pub struct FilterEditorState {
    pub step: EditorStep,
    pub field_list: ListState,
    pub op_list: ListState,
    pub value_input: String,
    pub selected_field: Option<Field>,
    pub selected_op: Option<Op>,
    /// Suggestions for the value step, populated by app.rs based on field.
    pub suggestions: Vec<String>,
    filtered_suggestions: Vec<usize>,
    suggestion_state: ListState,
}

impl Default for FilterEditorState {
    fn default() -> Self {
        let mut field_list = ListState::default();
        field_list.select(Some(0));
        let mut op_list = ListState::default();
        op_list.select(Some(0));
        Self {
            step: EditorStep::SelectField,
            field_list,
            op_list,
            value_input: String::new(),
            selected_field: None,
            selected_op: None,
            suggestions: Vec::new(),
            filtered_suggestions: Vec::new(),
            suggestion_state: ListState::default(),
        }
    }
}

impl FilterEditorState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> FilterEditorAction {
        match self.step {
            EditorStep::SelectField => self.handle_field_key(key),
            EditorStep::SelectOp => self.handle_op_key(key),
            EditorStep::EnterValue => self.handle_value_key(key),
        }
    }

    fn handle_field_key(&mut self, key: &KeyEvent) -> FilterEditorAction {
        let fields = Field::all();
        if keys::is_up(key) {
            if let Some(sel) = self.field_list.selected() {
                self.field_list.select(Some(sel.saturating_sub(1)));
            }
            return FilterEditorAction::Continue;
        }
        if keys::is_down(key) {
            if let Some(sel) = self.field_list.selected() {
                self.field_list
                    .select(Some((sel + 1).min(fields.len() - 1)));
            }
            return FilterEditorAction::Continue;
        }
        match key.code {
            KeyCode::Esc => FilterEditorAction::Cancel,
            KeyCode::Enter => {
                if let Some(sel) = self.field_list.selected() {
                    self.selected_field = Some(fields[sel].clone());
                    self.step = EditorStep::SelectOp;
                }
                FilterEditorAction::Continue
            }
            _ => FilterEditorAction::Continue,
        }
    }

    fn handle_op_key(&mut self, key: &KeyEvent) -> FilterEditorAction {
        let ops = Op::all();
        if keys::is_up(key) {
            if let Some(sel) = self.op_list.selected() {
                self.op_list.select(Some(sel.saturating_sub(1)));
            }
            return FilterEditorAction::Continue;
        }
        if keys::is_down(key) {
            if let Some(sel) = self.op_list.selected() {
                self.op_list.select(Some((sel + 1).min(ops.len() - 1)));
            }
            return FilterEditorAction::Continue;
        }
        match key.code {
            KeyCode::Esc => {
                self.step = EditorStep::SelectField;
                FilterEditorAction::Continue
            }
            KeyCode::Enter => {
                if let Some(sel) = self.op_list.selected() {
                    self.selected_op = Some(ops[sel].clone());
                    self.step = EditorStep::EnterValue;
                    self.refilter_suggestions();
                }
                FilterEditorAction::Continue
            }
            _ => FilterEditorAction::Continue,
        }
    }

    fn handle_value_key(&mut self, key: &KeyEvent) -> FilterEditorAction {
        // Ctrl+N/P and arrows for suggestion navigation (not j/k since those type chars)
        let is_nav_up = key.code == KeyCode::Up
            || (key.code == KeyCode::Char('p')
                && key.modifiers == crossterm::event::KeyModifiers::CONTROL);
        let is_nav_down = key.code == KeyCode::Down
            || (key.code == KeyCode::Char('n')
                && key.modifiers == crossterm::event::KeyModifiers::CONTROL);
        if is_nav_up && !self.filtered_suggestions.is_empty() {
            let current = self.suggestion_state.selected().unwrap_or(0);
            self.suggestion_state
                .select(Some(current.saturating_sub(1)));
            return FilterEditorAction::Continue;
        }
        if is_nav_down && !self.filtered_suggestions.is_empty() {
            let current = self.suggestion_state.selected().unwrap_or(0);
            let max = self.filtered_suggestions.len().saturating_sub(1);
            self.suggestion_state
                .select(Some((current + 1).min(max)));
            return FilterEditorAction::Continue;
        }
        match key.code {
            KeyCode::Esc => {
                self.step = EditorStep::SelectOp;
                FilterEditorAction::Continue
            }
            KeyCode::Enter => {
                // If a suggestion is highlighted, use it
                let value = if let Some(sel) = self.suggestion_state.selected() {
                    self.filtered_suggestions
                        .get(sel)
                        .map(|&idx| self.suggestions[idx].clone())
                        .unwrap_or_else(|| self.value_input.clone())
                } else {
                    self.value_input.clone()
                };
                if let (Some(field), Some(op)) = (&self.selected_field, &self.selected_op) {
                    let condition = FilterCondition {
                        field: field.clone(),
                        op: op.clone(),
                        value,
                    };
                    self.reset();
                    return FilterEditorAction::AddCondition(condition);
                }
                FilterEditorAction::Continue
            }
            KeyCode::Tab => {
                // Accept highlighted suggestion into input
                if let Some(sel) = self.suggestion_state.selected() {
                    if let Some(&idx) = self.filtered_suggestions.get(sel) {
                        self.value_input = self.suggestions[idx].clone();
                        self.refilter_suggestions();
                    }
                }
                FilterEditorAction::Continue
            }
            KeyCode::Backspace => {
                self.value_input.pop();
                self.refilter_suggestions();
                FilterEditorAction::Continue
            }
            KeyCode::Char(c) => {
                self.value_input.push(c);
                self.refilter_suggestions();
                FilterEditorAction::Continue
            }
            _ => FilterEditorAction::Continue,
        }
    }

    fn refilter_suggestions(&mut self) {
        if self.suggestions.is_empty() {
            self.filtered_suggestions.clear();
            self.suggestion_state.select(None);
            return;
        }
        let query = self.value_input.to_lowercase();
        self.filtered_suggestions = self
            .suggestions
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                if query.is_empty() {
                    true
                } else {
                    s.to_lowercase().contains(&query)
                }
            })
            .map(|(i, _)| i)
            .collect();
        if self.filtered_suggestions.is_empty() {
            self.suggestion_state.select(None);
        } else {
            self.suggestion_state.select(Some(0));
        }
    }
}

pub enum FilterEditorAction {
    Continue,
    Cancel,
    AddCondition(FilterCondition),
}

fn step_indicator(step: &EditorStep) -> Line<'static> {
    let (s1, s2, s3) = match step {
        EditorStep::SelectField => (styles::BLUE, styles::TEXT_DIM, styles::TEXT_DIM),
        EditorStep::SelectOp => (styles::BLUE, styles::BLUE, styles::TEXT_DIM),
        EditorStep::EnterValue => (styles::BLUE, styles::BLUE, styles::BLUE),
    };
    Line::from(vec![
        Span::raw("  "),
        Span::styled("●", ratatui::style::Style::default().fg(s1)),
        Span::raw(" "),
        Span::styled("●", ratatui::style::Style::default().fg(s2)),
        Span::raw(" "),
        Span::styled("●", ratatui::style::Style::default().fg(s3)),
        Span::styled("  Step ", styles::overlay_desc_style()),
        Span::styled(
            match step {
                EditorStep::SelectField => "1/3",
                EditorStep::SelectOp => "2/3",
                EditorStep::EnterValue => "3/3",
            },
            styles::overlay_desc_style(),
        ),
    ])
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut FilterEditorState) {
    let popup = centered_rect(50, 60, area);
    frame.render_widget(Clear, popup);

    match state.step {
        EditorStep::SelectField => {
            let items: Vec<ListItem> = Field::all()
                .iter()
                .map(|f| ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(f.name(), styles::overlay_text_style()),
                ])))
                .collect();
            let list = List::new(items)
                .highlight_style(styles::selected_style())
                .highlight_symbol(styles::ICON_SELECTOR)
                .block(styles::overlay_block("Select Field"));
            frame.render_stateful_widget(list, popup, &mut state.field_list);

            let indicator_area = Rect {
                x: popup.x + 1,
                y: popup.y + popup.height.saturating_sub(2),
                width: popup.width.saturating_sub(2),
                height: 1,
            };
            frame.render_widget(Paragraph::new(step_indicator(&state.step)), indicator_area);
        }
        EditorStep::SelectOp => {
            let field_name = state
                .selected_field
                .as_ref()
                .map(|f| f.name())
                .unwrap_or("?");
            let items: Vec<ListItem> = Op::all()
                .iter()
                .map(|o| {
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            format!(
                                "{} ({})",
                                match o {
                                    Op::Eq => "equals",
                                    Op::Neq => "not equals",
                                    Op::Contains => "contains",
                                    Op::NotContains => "not contains",
                                },
                                o.symbol()
                            ),
                            styles::overlay_text_style(),
                        ),
                    ]))
                })
                .collect();
            let title = format!("{field_name}: Select Operator");
            let list = List::new(items)
                .highlight_style(styles::selected_style())
                .highlight_symbol(styles::ICON_SELECTOR)
                .block(styles::overlay_block(&title));
            frame.render_stateful_widget(list, popup, &mut state.op_list);

            let indicator_area = Rect {
                x: popup.x + 1,
                y: popup.y + popup.height.saturating_sub(2),
                width: popup.width.saturating_sub(2),
                height: 1,
            };
            frame.render_widget(Paragraph::new(step_indicator(&state.step)), indicator_area);
        }
        EditorStep::EnterValue => {
            let field_name = state
                .selected_field
                .as_ref()
                .map(|f| f.name())
                .unwrap_or("?");
            let op_sym = state
                .selected_op
                .as_ref()
                .map(|o| o.symbol())
                .unwrap_or("?");

            let has_suggestions = !state.suggestions.is_empty();

            if has_suggestions {
                // Split popup: input at top, suggestions list below
                let chunks =
                    Layout::vertical([Constraint::Length(5), Constraint::Min(1)]).split(popup);

                // Input area
                let input_lines = vec![
                    Line::from(""),
                    step_indicator(&state.step),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(field_name, styles::overlay_key_style()),
                        Span::styled(format!(" {op_sym} "), styles::overlay_desc_style()),
                        Span::styled(
                            if state.value_input.is_empty() {
                                "type to filter..."
                            } else {
                                &state.value_input
                            },
                            if state.value_input.is_empty() {
                                styles::overlay_desc_style()
                                    .add_modifier(Modifier::ITALIC)
                            } else {
                                styles::title_style()
                            },
                        ),
                        Span::styled("_", styles::title_style()),
                    ]),
                ];
                let input_block = styles::overlay_block("Enter Value");
                let input_para = Paragraph::new(input_lines).block(input_block);
                frame.render_widget(input_para, chunks[0]);

                // Suggestions list
                let items: Vec<ListItem> = state
                    .filtered_suggestions
                    .iter()
                    .map(|&idx| {
                        let label = &state.suggestions[idx];
                        let mut spans = vec![Span::raw("  ")];
                        spans.extend(styles::label_spans(label));
                        ListItem::new(Line::from(spans))
                    })
                    .collect();

                let count = state.filtered_suggestions.len();
                let suggestion_title = format!("Suggestions ({count})  Tab:accept");
                let list = List::new(items)
                    .highlight_style(styles::selected_style())
                    .highlight_symbol(styles::ICON_SELECTOR)
                    .block(styles::overlay_block(&suggestion_title));
                frame.render_stateful_widget(list, chunks[1], &mut state.suggestion_state);
            } else {
                // No suggestions: simple input
                let lines = vec![
                    Line::from(""),
                    step_indicator(&state.step),
                    Line::from(""),
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(field_name, styles::overlay_key_style()),
                        Span::styled(format!(" {op_sym} "), styles::overlay_desc_style()),
                        Span::styled(
                            if state.value_input.is_empty() {
                                "type value..."
                            } else {
                                &state.value_input
                            },
                            if state.value_input.is_empty() {
                                styles::overlay_desc_style()
                                    .add_modifier(Modifier::ITALIC)
                            } else {
                                styles::title_style()
                            },
                        ),
                        Span::styled("_", styles::title_style()),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  Enter to confirm, Esc to go back",
                        styles::overlay_desc_style(),
                    )),
                    Line::from(Span::styled(
                        "  Hint: $me, none, true/false",
                        styles::overlay_desc_style(),
                    )),
                ];
                let para = Paragraph::new(lines).block(styles::overlay_block("Enter Value"));
                frame.render_widget(para, popup);
            }
        }
    }
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
