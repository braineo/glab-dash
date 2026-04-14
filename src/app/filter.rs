//! Filter and sort methods: active filter access, sort/filter UI, chord callbacks.

use super::{App, Overlay, View};
use crate::cmd::Cmd;
use crate::filter::{Field, FilterCondition, Op};
use crate::ui::components::chord_popup;
use crate::ui::views::filter_editor;
use crate::ui::views::list_model::UserFilter;

impl App {
    /// Returns a mutable reference to the `UserFilter` for the current view.
    pub(super) fn active_filter_mut(&mut self) -> &mut UserFilter {
        match self.ui.view {
            View::IssueList | View::IssueDetail => &mut self.ui.views.issue_list.filter,
            View::MrList | View::MrDetail => &mut self.ui.views.mr_list.filter,
            View::Planning => {
                let col = self.ui.views.planning.focused_column;
                &mut self.ui.views.planning.columns[col].filter
            }
            View::Dashboard => &mut self.ui.views.board.filter,
        }
    }

    pub(super) fn active_filter(&self) -> &UserFilter {
        match self.ui.view {
            View::IssueList | View::IssueDetail => &self.ui.views.issue_list.filter,
            View::MrList | View::MrDetail => &self.ui.views.mr_list.filter,
            View::Planning => {
                let col = self.ui.views.planning.focused_column;
                &self.ui.views.planning.columns[col].filter
            }
            View::Dashboard => &self.ui.views.board.filter,
        }
    }

    pub(super) fn action_sort_by_field(&mut self) {
        let kind = match self.ui.view {
            View::IssueList | View::IssueDetail | View::Planning | View::Dashboard => "issue",
            View::MrList | View::MrDetail => "merge_request",
        };

        let mut labels = Vec::new();

        // "Clear sort" when a sort is active
        let has_sort = !self.active_filter().sort_specs.is_empty();
        if has_sort {
            labels.push("⊘ Clear sort".to_string());
        }

        // Sort config presets
        for p in &self.ctx.config.sort_presets {
            if p.kind == kind {
                labels.push(format!("▸ {}", p.name));
            }
        }

        // Built-in field sorts
        let fields: &[crate::sort::SortField] = match kind {
            "merge_request" => crate::sort::SortField::all_mr(),
            _ => crate::sort::SortField::all_issue(),
        };
        for field in fields {
            labels.push(field.name().to_string());
        }

        // Label scope sorts from config
        for order in &self.ctx.config.label_sort_orders {
            labels.push(format!("{}::", order.scope));
        }

        self.ui.chord_state = Some(chord_popup::ChordState::new_for_names("Sort by", labels));
        self.ui.chord_on_complete = Some(Box::new(|value, app| {
            app.handle_sort_field_chosen(&value);
        }));
        self.ui.overlay = Overlay::Chord;
    }

    pub(super) fn handle_sort_field_chosen(&mut self, value: &str) {
        // Clear sort — apply immediately
        if value == "⊘ Clear sort" {
            self.apply_sort_specs(Vec::new());
            return;
        }

        // Config preset — apply immediately
        if let Some(preset_name) = value.strip_prefix("▸ ") {
            self.apply_sort_preset(preset_name);
            return;
        }

        // Field or label scope — show direction chord
        let (field_name, label_scope) = if let Some(scope) = value.strip_suffix("::") {
            ("label".to_string(), Some(scope.to_string()))
        } else {
            (value.to_string(), None)
        };

        let labels = vec!["↓ Descending".to_string(), "↑ Ascending".to_string()];
        self.ui.chord_state = Some(chord_popup::ChordState::new_for_names(
            &format!("Sort {value}"),
            labels,
        ));
        let field_name_clone = field_name.clone();
        let label_scope_clone = label_scope.clone();
        self.ui.chord_on_complete = Some(Box::new(move |value, app| {
            app.handle_sort_direction_chosen(
                &field_name_clone,
                label_scope_clone.as_deref(),
                &value,
            );
        }));
        self.ui.overlay = Overlay::Chord;
    }

    pub(super) fn handle_sort_direction_chosen(
        &mut self,
        field_name: &str,
        label_scope: Option<&str>,
        value: &str,
    ) {
        let direction = if value.starts_with('↑') {
            crate::sort::SortDirection::Asc
        } else {
            crate::sort::SortDirection::Desc
        };

        let Some(field) = crate::sort::SortField::from_str(field_name) else {
            return;
        };

        let specs = vec![crate::sort::SortSpec {
            field,
            direction,
            label_scope: label_scope.map(String::from),
        }];
        self.apply_sort_specs(specs);
    }

    fn apply_sort_specs(&mut self, specs: Vec<crate::sort::SortSpec>) {
        self.active_filter_mut().sort_specs = specs;
        self.ui.dirty.view_state = true;
        self.ui.pending_cmds.push(Cmd::PersistViewState);
    }

    fn apply_sort_preset(&mut self, name: &str) {
        let specs = self
            .ctx
            .config
            .sort_presets
            .iter()
            .find(|p| p.name == name)
            .map(|preset| {
                preset
                    .specs
                    .iter()
                    .filter_map(|s| {
                        let field = crate::sort::SortField::from_str(&s.field)?;
                        let direction = crate::sort::SortDirection::from_str(&s.direction)?;
                        Some(crate::sort::SortSpec {
                            field,
                            direction,
                            label_scope: s.label_scope.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.apply_sort_specs(specs);
    }

    pub(super) fn apply_preset(&mut self, name: &str) {
        if let Some(preset) = self.ctx.config.filters.iter().find(|f| f.name == name) {
            let conditions: Vec<FilterCondition> = preset
                .conditions
                .iter()
                .filter_map(|c| {
                    let field = Field::from_str(&c.field)?;
                    let op = Op::from_str(&c.op)?;
                    Some(FilterCondition {
                        field,
                        op,
                        value: c.value.clone(),
                    })
                })
                .collect();

            self.active_filter_mut().conditions = conditions;
            self.ui.dirty.view_state = true;
            self.ui.pending_cmds.push(Cmd::PersistViewState);
        }
    }

    pub(super) fn action_show_filter_menu(&mut self) {
        let kind = match self.ui.view {
            View::IssueList | View::IssueDetail | View::Planning | View::Dashboard => "issue",
            View::MrList | View::MrDetail => "merge_request",
        };

        let mut labels = Vec::new();

        // ── Builder section ──
        labels.push(format!("{}Builder", chord_popup::HEADER));

        let conditions = &self.active_filter().conditions;
        for cond in conditions {
            labels.push(format!("✕ {}", cond.display()));
        }
        labels.push("+ Add condition".to_string());
        if !conditions.is_empty() {
            labels.push("⊘ Clear all".to_string());
        }

        // ── Presets section ──
        let has_presets = self.ctx.config.filters.iter().any(|f| f.kind == kind);
        if has_presets {
            labels.push(chord_popup::DIVIDER.to_string());
            labels.push(format!("{}Presets", chord_popup::HEADER));
            for f in &self.ctx.config.filters {
                if f.kind == kind {
                    labels.push(format!("▸ {}", f.name));
                }
            }
        }

        self.ui.chord_state = Some(chord_popup::ChordState::new_for_names("Filter", labels));
        self.ui.chord_on_complete = Some(Box::new(|value, app| {
            app.handle_filter_menu_chosen(&value);
        }));
        self.ui.overlay = Overlay::Chord;
    }

    pub(super) fn handle_filter_menu_chosen(&mut self, value: &str) {
        if value == "+ Add condition" {
            self.show_filter_field_chord();
            return;
        }

        if value == "⊘ Clear all" {
            self.action_clear_filters();
            return;
        }

        if let Some(preset_name) = value.strip_prefix("▸ ") {
            self.apply_preset(preset_name);
            return;
        }

        // Remove a condition (strip "✕ " prefix, find and remove matching)
        if let Some(display) = value.strip_prefix("✕ ") {
            let conditions = &mut self.active_filter_mut().conditions;
            if let Some(idx) = conditions.iter().position(|c| c.display() == display) {
                conditions.remove(idx);
            }
            self.ui.dirty.view_state = true;
            self.ui.pending_cmds.push(Cmd::PersistViewState);
            // Reopen the filter menu
            self.action_show_filter_menu();
        }
    }

    fn show_filter_field_chord(&mut self) {
        let labels: Vec<String> = Field::all().iter().map(|f| f.name().to_string()).collect();
        self.ui.chord_state = Some(chord_popup::ChordState::new_for_names(
            "Filter Field",
            labels,
        ));
        self.ui.chord_on_complete = Some(Box::new(|value, app| {
            app.handle_filter_field_chosen(&value);
        }));
        self.ui.overlay = Overlay::Chord;
    }

    pub(super) fn handle_filter_field_chosen(&mut self, value: &str) {
        let Some(field) = Field::from_str(value) else {
            return;
        };
        let labels: Vec<String> = Op::all()
            .iter()
            .map(|o| {
                format!(
                    "{} ({})",
                    match o {
                        Op::Eq => "equals",
                        Op::Neq => "not equals",
                        Op::Contains => "contains",
                        Op::NotContains => "not contains",
                    },
                    o.symbol()
                )
            })
            .collect();
        self.ui.chord_state = Some(chord_popup::ChordState::new_for_names(
            &format!("{value}:"),
            labels,
        ));
        self.ui.chord_on_complete = Some(Box::new(move |value, app| {
            app.handle_filter_op_chosen(field, &value);
        }));
        self.ui.overlay = Overlay::Chord;
    }

    pub(super) fn handle_filter_op_chosen(&mut self, field: Field, value: &str) {
        // Parse op from the display label (e.g., "equals (=)" → Eq)
        let op = if value.starts_with("equals") {
            Op::Eq
        } else if value.starts_with("not equals") {
            Op::Neq
        } else if value.starts_with("not contains") {
            Op::NotContains
        } else if value.starts_with("contains") {
            Op::Contains
        } else {
            return;
        };

        // Set up filter editor at the value step with field and op pre-selected
        self.ui.filter_editor_state.reset();
        self.ui.filter_editor_state.selected_field = Some(field);
        self.ui.filter_editor_state.selected_op = Some(op);
        self.ui.filter_editor_state.step = filter_editor::EditorStep::EnterValue;
        self.ui.filter_editor_state.suggestions = self.get_filter_suggestions();
        self.ui.overlay = Overlay::FilterEditor;
    }

    pub(super) fn action_clear_filters(&mut self) {
        self.active_filter_mut().conditions.clear();
        self.ui.dirty.view_state = true;
        self.ui.pending_cmds.push(Cmd::PersistViewState);
    }

    pub(super) fn get_filter_suggestions(&self) -> Vec<String> {
        match &self.ui.filter_editor_state.selected_field {
            Some(Field::Label) => self.data.labels.iter().map(|l| l.name.clone()).collect(),
            Some(Field::State) => {
                let mut states = vec![
                    "opened".to_string(),
                    "closed".to_string(),
                    "merged".to_string(),
                ];
                // Add any custom status names from cached statuses
                for statuses in self.data.work_item_statuses.values() {
                    for s in statuses {
                        let name = s.name.to_lowercase();
                        if !states
                            .iter()
                            .any(|existing| existing.to_lowercase() == name)
                        {
                            states.push(s.name.clone());
                        }
                    }
                }
                states
            }
            Some(Field::Draft) => vec!["true".to_string(), "false".to_string()],
            Some(Field::Assignee | Field::Author | Field::Reviewer | Field::ApprovedBy) => {
                let mut names = self.picker_members();
                names.insert(0, "$me".to_string());
                names.push("none".to_string());
                names
            }
            _ => Vec::new(),
        }
    }
}
