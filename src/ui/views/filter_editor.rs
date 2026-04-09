use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, ListState, Paragraph};

use crate::filter::{Field, FilterCondition, Op};
use crate::ui::styles;

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
        match key.code {
            KeyCode::Esc => FilterEditorAction::Cancel,
            KeyCode::Enter => {
                if let Some(sel) = self.field_list.selected() {
                    self.selected_field = Some(fields[sel].clone());
                    self.step = EditorStep::SelectOp;
                }
                FilterEditorAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(sel) = self.field_list.selected() {
                    self.field_list.select(Some(sel.saturating_sub(1)));
                }
                FilterEditorAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(sel) = self.field_list.selected() {
                    self.field_list
                        .select(Some((sel + 1).min(fields.len() - 1)));
                }
                FilterEditorAction::Continue
            }
            _ => FilterEditorAction::Continue,
        }
    }

    fn handle_op_key(&mut self, key: &KeyEvent) -> FilterEditorAction {
        let ops = Op::all();
        match key.code {
            KeyCode::Esc => {
                self.step = EditorStep::SelectField;
                FilterEditorAction::Continue
            }
            KeyCode::Enter => {
                if let Some(sel) = self.op_list.selected() {
                    self.selected_op = Some(ops[sel].clone());
                    self.step = EditorStep::EnterValue;
                }
                FilterEditorAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(sel) = self.op_list.selected() {
                    self.op_list.select(Some(sel.saturating_sub(1)));
                }
                FilterEditorAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(sel) = self.op_list.selected() {
                    self.op_list.select(Some((sel + 1).min(ops.len() - 1)));
                }
                FilterEditorAction::Continue
            }
            _ => FilterEditorAction::Continue,
        }
    }

    fn handle_value_key(&mut self, key: &KeyEvent) -> FilterEditorAction {
        match key.code {
            KeyCode::Esc => {
                self.step = EditorStep::SelectOp;
                FilterEditorAction::Continue
            }
            KeyCode::Enter => {
                if let (Some(field), Some(op)) = (&self.selected_field, &self.selected_op) {
                    let condition = FilterCondition {
                        field: field.clone(),
                        op: op.clone(),
                        value: self.value_input.clone(),
                    };
                    self.reset();
                    return FilterEditorAction::AddCondition(condition);
                }
                FilterEditorAction::Continue
            }
            KeyCode::Backspace => {
                self.value_input.pop();
                FilterEditorAction::Continue
            }
            KeyCode::Char(c) => {
                self.value_input.push(c);
                FilterEditorAction::Continue
            }
            _ => FilterEditorAction::Continue,
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
        Span::styled("  Step ", styles::help_desc_style()),
        Span::styled(
            match step {
                EditorStep::SelectField => "1/3",
                EditorStep::SelectOp => "2/3",
                EditorStep::EnterValue => "3/3",
            },
            styles::help_desc_style(),
        ),
    ])
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut FilterEditorState) {
    let popup = centered_rect(45, 50, area);
    frame.render_widget(Clear, popup);

    match state.step {
        EditorStep::SelectField => {
            let items: Vec<ListItem> = Field::all()
                .iter()
                .map(|f| ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(f.name(), ratatui::style::Style::default().fg(styles::TEXT)),
                ])))
                .collect();
            let list = List::new(items)
                .highlight_style(styles::selected_style())
                .highlight_symbol(styles::ICON_SELECTOR)
                .block(styles::overlay_block("Select Field"));
            frame.render_stateful_widget(list, popup, &mut state.field_list);

            // Render step indicator at bottom
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
                            ratatui::style::Style::default().fg(styles::TEXT),
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
            let lines = vec![
                Line::from(""),
                step_indicator(&state.step),
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(field_name, styles::help_key_style()),
                    Span::styled(format!(" {op_sym} "), styles::help_desc_style()),
                    Span::styled(
                        if state.value_input.is_empty() {
                            "type value..."
                        } else {
                            &state.value_input
                        },
                        if state.value_input.is_empty() {
                            styles::draft_style()
                        } else {
                            styles::title_style()
                        },
                    ),
                    Span::styled("_", styles::title_style()),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  Enter to confirm, Esc to go back",
                    styles::help_desc_style(),
                )),
                Line::from(Span::styled(
                    "  Hint: $me, none, true/false",
                    styles::help_desc_style(),
                )),
            ];
            let para = Paragraph::new(lines).block(styles::overlay_block("Enter Value"));
            frame.render_widget(para, popup);
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
