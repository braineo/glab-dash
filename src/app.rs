use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use tokio::sync::mpsc;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::cache;
use crate::config::Config;
use crate::filter::{Field, FilterCondition, Op};
use crate::gitlab::client::GitLabClient;
use crate::gitlab::types::*;
use crate::ui::components::{chord_popup, confirm_dialog, error_popup, help, picker};
use crate::ui::keys;
use crate::ui::views::{dashboard, filter_editor, issue_detail, issue_list, mr_detail, mr_list};

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Dashboard,
    IssueList,
    IssueDetail,
    MrList,
    MrDetail,
}

#[derive(Debug)]
pub enum Overlay {
    None,
    Help,
    FilterEditor,
    Confirm(ConfirmAction),
    Picker(PickerContext),
    Chord(ChordContext),
    CommentInput,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    CloseIssue(String, u64),
    ReopenIssue(String, u64),
    ApproveMr(String, u64),
    MergeMr(String, u64),
    QuitApp,
}

#[derive(Debug)]
pub enum PickerContext {
    Labels,
    Assignee,
    Preset,
    SortPreset,
}

/// Context for the chord popup overlay (what action to perform on selection).
#[derive(Debug)]
pub enum ChordContext {
    /// Set issue status: (project_path, issue_db_id, issue_iid)
    Status(String, u64, u64),
    Assignee,
}

/// Messages from async operations
pub enum AsyncMsg {
    IssuesLoaded(Result<Vec<TrackedIssue>>, bool),
    MrsLoaded(Result<(Vec<TrackedMergeRequest>, Vec<TrackedMergeRequest>)>, bool),
    NotesLoaded(Result<Vec<Note>>),
    ActionDone(Result<String>),
    /// An issue was mutated; carry the updated object and project path.
    IssueUpdated(Result<Issue>, String),
    /// A merge request was mutated; carry the updated object and project path.
    MrUpdated(Result<MergeRequest>, String),
    /// Issue custom status changed: (project_path, iid, new_status_name).
    IssueStatusUpdated(Result<(String, u64, String)>),
    LabelsLoaded(Result<Vec<ProjectLabel>>),
    StatusesLoaded(Result<Vec<WorkItemStatus>>, String, u64, u64),
}

pub struct App {
    pub config: Config,
    pub client: GitLabClient,
    pub async_tx: mpsc::UnboundedSender<AsyncMsg>,

    // View state
    pub view: View,
    pub view_stack: Vec<View>,
    pub overlay: Overlay,
    pub active_team: usize,

    // Data
    pub issues: Vec<TrackedIssue>,
    pub mrs: Vec<TrackedMergeRequest>,
    pub labels: Vec<ProjectLabel>,
    pub label_color_map: crate::ui::styles::LabelColors,
    pub loading: bool,
    pub loading_msg: &'static str,
    pub error: Option<String>,

    // View-specific state
    pub issue_list_state: issue_list::IssueListState,
    pub mr_list_state: mr_list::MrListState,
    pub issue_detail_state: issue_detail::IssueDetailState,
    pub mr_detail_state: mr_detail::MrDetailState,
    pub filter_editor_state: filter_editor::FilterEditorState,
    pub picker_state: Option<picker::PickerState>,
    pub comment_input: crate::ui::components::input::InputState,

    // Cache / incremental fetch
    pub last_fetched_at: Option<u64>,

    // Work item statuses per project (project_path -> available statuses)
    pub work_item_statuses: std::collections::HashMap<String, Vec<WorkItemStatus>>,

    // Chord popup state (vim-style easymotion codes)
    pub chord_state: Option<chord_popup::ChordState>,

    // Filter state
    pub issue_filters: Vec<FilterCondition>,
    pub mr_filters: Vec<FilterCondition>,
    pub filter_bar_focused: bool,
    pub filter_bar_selected: usize,

    // Sort state
    pub issue_sort: Vec<crate::sort::SortSpec>,
    pub mr_sort: Vec<crate::sort::SortSpec>,
    pub label_sort_orders: std::collections::HashMap<String, Vec<String>>,
}

impl App {
    pub fn new(
        config: Config,
        client: GitLabClient,
        async_tx: mpsc::UnboundedSender<AsyncMsg>,
    ) -> Self {
        let label_sort_orders = config
            .label_sort_orders
            .iter()
            .map(|o| (o.scope.clone(), o.values.clone()))
            .collect();
        Self {
            config,
            client,
            async_tx,
            view: View::Dashboard,
            view_stack: Vec::new(),
            overlay: Overlay::None,
            active_team: 0,
            issues: Vec::new(),
            mrs: Vec::new(),
            labels: Vec::new(),
            label_color_map: std::collections::HashMap::new(),
            loading: false,
            loading_msg: "",
            error: None,
            issue_list_state: issue_list::IssueListState::default(),
            mr_list_state: mr_list::MrListState::default(),
            issue_detail_state: issue_detail::IssueDetailState::default(),
            mr_detail_state: mr_detail::MrDetailState::default(),
            filter_editor_state: filter_editor::FilterEditorState::default(),
            picker_state: None,
            comment_input: crate::ui::components::input::InputState::default(),
            last_fetched_at: None,
            work_item_statuses: std::collections::HashMap::new(),
            chord_state: None,
            issue_filters: Vec::new(),
            mr_filters: Vec::new(),
            filter_bar_focused: false,
            filter_bar_selected: 0,
            issue_sort: Vec::new(),
            mr_sort: Vec::new(),
            label_sort_orders,
        }
    }

    /// Load cached data for instant startup display.
    pub fn load_cache(&mut self) {
        if let Some(cached) = cache::load() {
            if cached.team_index == self.active_team {
                self.last_fetched_at = Some(cached.saved_at);
                self.issues = cached.issues;
                self.mrs = cached.mrs;
                self.labels = cached.labels;
                self.rebuild_label_color_map();
                self.refilter_issues();
                self.refilter_mrs();
            }
        }
    }

    fn rebuild_label_color_map(&mut self) {
        self.label_color_map = self
            .labels
            .iter()
            .filter_map(|l| Some((l.name.clone(), l.color.clone()?)))
            .collect();
    }

    fn save_cache(&self) {
        cache::save(self.active_team, &self.issues, &self.mrs, &self.labels);
    }

    pub fn fetch_all(&self) {
        self.fetch_issues();
        self.fetch_mrs();
        self.fetch_labels();
    }

    /// Convert a unix timestamp to ISO 8601 for the GitLab API, with 60s safety buffer.
    fn updated_after_param(ts: u64) -> String {
        let buffered = ts.saturating_sub(60);
        chrono::DateTime::from_timestamp(buffered as i64, 0)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn fetch_issues(&self) {
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let updated_after = self.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        let members = self.config.team_members(self.active_team);
        tokio::spawn(async move {
            let (state, ua) = if incremental {
                (None, updated_after.as_deref())
            } else {
                (Some("opened"), None)
            };
            let (tracking, assigned) = tokio::join!(
                client.fetch_tracking_issues(state, ua),
                client.fetch_assigned_issues(&members, state, ua),
            );
            let result = match (tracking, assigned) {
                (Ok(mut t), Ok(a)) => {
                    let existing: std::collections::HashSet<u64> =
                        t.iter().map(|i| i.issue.id).collect();
                    t.extend(a.into_iter().filter(|i| !existing.contains(&i.issue.id)));
                    Ok(t)
                }
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::IssuesLoaded(result, incremental));
        });
    }

    fn fetch_mrs(&self) {
        let client = self.client.clone();
        let members = self.config.team_members(self.active_team);
        let tx = self.async_tx.clone();
        let updated_after = self.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        tokio::spawn(async move {
            let (state, ua) = if incremental {
                ("all", updated_after.as_deref())
            } else {
                ("opened", None)
            };
            let tracking = client.fetch_tracking_mrs(state, ua).await;
            let external = client.fetch_external_mrs(&members, state, ua).await;
            let result = match (tracking, external) {
                (Ok(t), Ok(e)) => Ok((t, e)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::MrsLoaded(result, incremental));
        });
    }

    fn fetch_labels(&self) {
        let client = self.client.clone();
        let project = self.config.tracking_project.clone();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_project_labels(&project).await;
            let _ = tx.send(AsyncMsg::LabelsLoaded(result));
        });
    }

    fn fetch_notes_for_issue(&self, project: &str, iid: u64) {
        let client = self.client.clone();
        let project = project.to_string();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_issue_notes(&project, iid).await;
            let _ = tx.send(AsyncMsg::NotesLoaded(result));
        });
    }

    fn fetch_statuses_and_show_picker(&mut self, project: String, issue_id: u64, iid: u64) {
        // If we already have cached statuses for this project, show chord immediately
        if let Some(statuses) = self.work_item_statuses.get(&project) {
            if statuses.is_empty() {
                // No custom statuses — fall back to open/close toggle
                let item_state = self
                    .issue_list_state
                    .selected_issue(&self.issues)
                    .or_else(|| self.current_detail_issue())
                    .map(|i| i.issue.state.as_str())
                    .unwrap_or("opened");
                let action = if item_state == "opened" {
                    ConfirmAction::CloseIssue(project, iid)
                } else {
                    ConfirmAction::ReopenIssue(project, iid)
                };
                self.overlay = Overlay::Confirm(action);
            } else {
                let names: Vec<String> = statuses.iter().map(|s| s.name.clone()).collect();
                self.chord_state = Some(chord_popup::ChordState::new("Set Status", names).with_kind(chord_popup::ChordKind::Status));
                self.overlay = Overlay::Chord(ChordContext::Status(project, issue_id, iid));
            }
            return;
        }

        // Fetch statuses from GitLab
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let project_clone = project.clone();
        self.loading = true;
        tokio::spawn(async move {
            let result = client.fetch_work_item_statuses(&project_clone).await;
            let _ = tx.send(AsyncMsg::StatusesLoaded(result, project_clone, issue_id, iid));
        });
    }

    fn fetch_notes_for_mr(&self, project: &str, iid: u64) {
        let client = self.client.clone();
        let project = project.to_string();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_mr_notes(&project, iid).await;
            let _ = tx.send(AsyncMsg::NotesLoaded(result));
        });
    }

    pub fn handle_async_msg(&mut self, msg: AsyncMsg) {
        match msg {
            AsyncMsg::IssuesLoaded(result, incremental) => match result {
                Ok(items) => {
                    if incremental {
                        for item in items {
                            if let Some(pos) =
                                self.issues.iter().position(|e| e.issue.id == item.issue.id)
                            {
                                self.issues[pos] = item;
                            } else {
                                self.issues.push(item);
                            }
                        }
                        self.issues.retain(|i| i.issue.state == "opened");
                    } else {
                        self.issues = items;
                    }
                    self.last_fetched_at = Some(Self::now_secs());
                    self.error = None;
                    self.loading = false;
                    self.refilter_issues();
                    self.save_cache();
                }
                Err(e) => {
                    self.loading = false;
                    self.show_error(format!("Issues: {e}"));
                }
            },
            AsyncMsg::MrsLoaded(result, incremental) => match result {
                Ok((tracking, external)) => {
                    if incremental {
                        for item in tracking.into_iter().chain(external) {
                            if let Some(pos) =
                                self.mrs.iter().position(|e| e.mr.id == item.mr.id)
                            {
                                self.mrs[pos] = item;
                            } else {
                                self.mrs.push(item);
                            }
                        }
                        self.mrs.retain(|m| m.mr.state == "opened");
                    } else {
                        self.mrs = tracking;
                        self.mrs.extend(external);
                    }
                    self.last_fetched_at = Some(Self::now_secs());
                    self.loading = false;
                    self.error = None;
                    self.refilter_mrs();
                    self.save_cache();
                }
                Err(e) => {
                    self.loading = false;
                    self.show_error(format!("MRs: {e}"));
                }
            },
            AsyncMsg::NotesLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(notes) => {
                        if self.view == View::IssueDetail {
                            self.issue_detail_state.notes = notes;
                            self.issue_detail_state.loading_notes = false;
                        } else if self.view == View::MrDetail {
                            self.mr_detail_state.notes = notes;
                            self.mr_detail_state.loading_notes = false;
                        }
                    }
                    Err(e) => {
                        self.show_error(format!("Notes: {e}"));
                    }
                }
            }
            AsyncMsg::ActionDone(result) => {
                self.loading = false;
                match result {
                    Ok(_msg) => {
                        self.error = None;
                        // Refresh data after action
                        self.fetch_all();
                    }
                    Err(e) => {
                        self.show_error(e.to_string());
                    }
                }
            }
            AsyncMsg::IssueUpdated(result, project_path) => {
                self.loading = false;
                match result {
                    Ok(issue) => {
                        if let Some(pos) = self.issues.iter().position(|e| {
                            e.issue.iid == issue.iid && e.project_path == project_path
                        }) {
                            let custom_status = self.issues[pos].issue.custom_status.clone();
                            self.issues[pos].issue = issue;
                            self.issues[pos].issue.custom_status = custom_status;
                        }
                        self.error = None;
                        self.refilter_issues();
                        self.save_cache();
                    }
                    Err(e) => self.show_error(e.to_string()),
                }
            }
            AsyncMsg::MrUpdated(result, project_path) => {
                self.loading = false;
                match result {
                    Ok(mr) => {
                        if let Some(pos) = self.mrs.iter().position(|e| {
                            e.mr.iid == mr.iid && e.project_path == project_path
                        }) {
                            self.mrs[pos].mr = mr;
                        }
                        self.error = None;
                        self.refilter_mrs();
                        self.save_cache();
                    }
                    Err(e) => self.show_error(e.to_string()),
                }
            }
            AsyncMsg::IssueStatusUpdated(result) => {
                self.loading = false;
                match result {
                    Ok((project_path, iid, status_name)) => {
                        if let Some(pos) = self.issues.iter().position(|e| {
                            e.issue.iid == iid && e.project_path == project_path
                        }) {
                            self.issues[pos].issue.custom_status = Some(status_name);
                        }
                        self.error = None;
                        self.refilter_issues();
                        self.save_cache();
                    }
                    Err(e) => self.show_error(e.to_string()),
                }
            }
            AsyncMsg::LabelsLoaded(result) => {
                if let Ok(labels) = result {
                    self.labels = labels;
                    self.rebuild_label_color_map();
                    self.save_cache();
                }
            }
            AsyncMsg::StatusesLoaded(result, project, issue_id, iid) => {
                self.loading = false;
                match result {
                    Ok(statuses) => {
                        if statuses.is_empty() {
                            // No custom statuses available — fall back to open/close toggle
                            let item_state = self
                                .issue_list_state
                                .selected_issue(&self.issues)
                                .or_else(|| self.current_detail_issue())
                                .map(|i| i.issue.state.as_str())
                                .unwrap_or("opened");
                            let action = if item_state == "opened" {
                                ConfirmAction::CloseIssue(project, iid)
                            } else {
                                ConfirmAction::ReopenIssue(project, iid)
                            };
                            self.overlay = Overlay::Confirm(action);
                        } else {
                            // Cache and show status chord
                            let names: Vec<String> =
                                statuses.iter().map(|s| s.name.clone()).collect();
                            self.work_item_statuses
                                .insert(project.clone(), statuses);
                            self.chord_state =
                                Some(chord_popup::ChordState::new("Set Status", names).with_kind(chord_popup::ChordKind::Status));
                            self.overlay =
                                Overlay::Chord(ChordContext::Status(project, issue_id, iid));
                        }
                    }
                    Err(e) => {
                        self.show_error(format!("Statuses: {e}"));
                    }
                }
            }
        }
    }

    fn show_error(&mut self, msg: String) {
        self.error = Some(msg.clone());
        self.overlay = Overlay::Error(msg);
    }

    pub fn refilter_issues(&mut self) {
        let me = self.config.me.clone();
        let members = self.config.team_members(self.active_team);
        self.issue_list_state.active_sort = self.issue_sort.clone();
        self.issue_list_state.apply_filters(
            &self.issues,
            &self.issue_filters,
            &me,
            &members,
            &self.label_sort_orders,
        );
    }

    fn get_filter_suggestions(&self) -> Vec<String> {
        use crate::filter::Field;
        match &self.filter_editor_state.selected_field {
            Some(Field::Label) => self.labels.iter().map(|l| l.name.clone()).collect(),
            Some(Field::State) => {
                let mut states = vec![
                    "opened".to_string(),
                    "closed".to_string(),
                    "merged".to_string(),
                ];
                // Add any custom status names from cached statuses
                for statuses in self.work_item_statuses.values() {
                    for s in statuses {
                        let name = s.name.to_lowercase();
                        if !states.iter().any(|existing| existing.to_lowercase() == name) {
                            states.push(s.name.clone());
                        }
                    }
                }
                states
            }
            Some(Field::Draft) => vec!["true".to_string(), "false".to_string()],
            Some(Field::Assignee | Field::Author | Field::Reviewer | Field::ApprovedBy) => {
                let mut names = self.config.team_members(self.active_team);
                names.insert(0, "$me".to_string());
                names.push("none".to_string());
                names
            }
            _ => Vec::new(),
        }
    }

    fn refilter_mrs(&mut self) {
        let me = self.config.me.clone();
        let members = self.config.team_members(self.active_team);
        self.mr_list_state.active_sort = self.mr_sort.clone();
        self.mr_list_state.apply_filters(
            &self.mrs,
            &self.mr_filters,
            &me,
            &members,
            &self.label_sort_orders,
        );
    }

    fn show_sort_preset_picker(&mut self, kind: &str) {
        let mut names: Vec<String> = Vec::new();

        // "Clear sort" when a sort is active
        let has_sort = match self.view {
            View::IssueList => !self.issue_sort.is_empty(),
            View::MrList => !self.mr_sort.is_empty(),
            _ => false,
        };
        if has_sort {
            names.push("⊘ Clear sort".to_string());
        }

        // Config presets
        for p in &self.config.sort_presets {
            if p.kind == kind {
                names.push(format!("▸ {}", p.name));
            }
        }

        // Built-in field sorts (always available)
        let fields: &[crate::sort::SortField] = match kind {
            "merge_request" => crate::sort::SortField::all_mr(),
            _ => crate::sort::SortField::all_issue(),
        };
        for field in fields {
            names.push(format!("↓ {} (newest first)", field.name()));
            names.push(format!("↑ {} (oldest first)", field.name()));
        }

        // Label scope sorts from config
        for order in &self.config.label_sort_orders {
            names.push(format!("↓ {}:: (high priority first)", order.scope));
            names.push(format!("↑ {}:: (low priority first)", order.scope));
        }

        self.picker_state = Some(picker::PickerState::new("Sort", names, false));
        self.overlay = Overlay::Picker(PickerContext::SortPreset);
    }

    fn apply_sort_preset(&mut self, name: &str) {
        let specs = if name == "⊘ Clear sort" {
            Vec::new()
        } else if let Some(preset_name) = name.strip_prefix("▸ ") {
            // Config preset
            self.config
                .sort_presets
                .iter()
                .find(|p| p.name == preset_name)
                .map(|preset| {
                    preset
                        .specs
                        .iter()
                        .filter_map(|s| {
                            let field = crate::sort::SortField::from_str(&s.field)?;
                            let direction =
                                crate::sort::SortDirection::from_str(&s.direction)?;
                            Some(crate::sort::SortSpec {
                                field,
                                direction,
                                label_scope: s.label_scope.clone(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else if let Some(rest) = name.strip_prefix("↓ ") {
            self.parse_inline_sort(rest, crate::sort::SortDirection::Desc)
        } else if let Some(rest) = name.strip_prefix("↑ ") {
            self.parse_inline_sort(rest, crate::sort::SortDirection::Asc)
        } else {
            return;
        };

        match self.view {
            View::IssueList => {
                self.issue_sort = specs;
                self.refilter_issues();
            }
            View::MrList => {
                self.mr_sort = specs;
                self.refilter_mrs();
            }
            _ => {}
        }
    }

    fn parse_inline_sort(
        &self,
        text: &str,
        direction: crate::sort::SortDirection,
    ) -> Vec<crate::sort::SortSpec> {
        // "field_name (description)" or "scope:: (description)"
        let field_part = text.split(" (").next().unwrap_or(text);

        // Label scope sort: "workflow::"
        if let Some(scope) = field_part.strip_suffix("::") {
            return vec![crate::sort::SortSpec {
                field: crate::sort::SortField::Label,
                direction,
                label_scope: Some(scope.to_string()),
            }];
        }

        // Regular field sort
        if let Some(field) = crate::sort::SortField::from_str(field_part) {
            return vec![crate::sort::SortSpec {
                field,
                direction,
                label_scope: None,
            }];
        }

        Vec::new()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Handle overlay first
        if !matches!(self.overlay, Overlay::None) {
            return self.handle_overlay_key(key);
        }

        // Handle filter bar focus
        if self.filter_bar_focused {
            self.handle_filter_bar_key(key);
            return false;
        }

        // Global keys — q navigates back, confirms quit on dashboard
        if keys::is_quit(&key) {
            if let Some(prev) = self.view_stack.pop() {
                self.view = prev;
            } else {
                self.overlay = Overlay::Confirm(ConfirmAction::QuitApp);
            }
            return false;
        }

        if key.code == KeyCode::Char('?') {
            self.overlay = Overlay::Help;
            return false;
        }

        // Team switching with number keys (only when not in text input)
        if let KeyCode::Char(c) = key.code
            && c.is_ascii_digit()
            && key.modifiers == KeyModifiers::NONE
        {
            let num = c.to_digit(10).unwrap_or(0) as usize;
            if num >= 1 && num <= self.config.teams.len() {
                self.active_team = num - 1;
                self.last_fetched_at = None; // force full fetch for new team
                self.loading = true;
                self.fetch_all();
                return false;
            }
        }

        // Navigation (skip if a view is in search input mode)
        let in_search = match self.view {
            View::IssueList => self.issue_list_state.searching,
            View::MrList => self.mr_list_state.searching,
            _ => false,
        };
        if keys::is_back(&key) && !in_search {
            if let Some(prev) = self.view_stack.pop() {
                self.view = prev;
            }
            return false;
        }

        // View-specific handling
        match self.view {
            View::Dashboard => self.handle_dashboard_key(key),
            View::IssueList => self.handle_issue_list_key(key),
            View::IssueDetail => self.handle_issue_detail_key(key),
            View::MrList => self.handle_mr_list_key(key),
            View::MrDetail => self.handle_mr_detail_key(key),
        }

        false
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('i') => {
                self.view_stack.push(View::Dashboard);
                self.view = View::IssueList;
                self.refilter_issues();
            }
            KeyCode::Char('m') => {
                self.view_stack.push(View::Dashboard);
                self.view = View::MrList;
                self.refilter_mrs();
            }
            _ => {}
        }
    }

    fn handle_chord_result(&mut self, value: String) {
        let context = std::mem::replace(&mut self.overlay, Overlay::None);
        match context {
            Overlay::Chord(ChordContext::Status(project, issue_id, iid)) => {
                self.set_issue_status(&project, issue_id, iid, &value);
            }
            Overlay::Chord(ChordContext::Assignee) => {
                self.update_assignee(&value);
            }
            _ => {}
        }
    }

    fn handle_issue_list_key(&mut self, key: KeyEvent) {
        // Tab to focus filter bar
        if keys::is_tab(&key) && !self.issue_filters.is_empty() {
            self.filter_bar_focused = true;
            self.filter_bar_selected = 0;
            return;
        }

        let action = self.issue_list_state.handle_key(&key, self.issues.len());

        match action {
            issue_list::IssueListAction::None => {}
            issue_list::IssueListAction::Refilter => self.refilter_issues(),
            issue_list::IssueListAction::Refresh => {
                self.loading = true;
                self.fetch_all();
            }
            issue_list::IssueListAction::OpenDetail => {
                if let Some(item) = self.issue_list_state.selected_issue(&self.issues) {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    self.issue_detail_state.reset();
                    self.issue_detail_state.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::IssueList);
                    self.view = View::IssueDetail;
                }
            }
            issue_list::IssueListAction::ToggleState => {
                if let Some(item) = self.issue_list_state.selected_issue(&self.issues) {
                    let project = item.project_path.clone();
                    let issue_id = item.issue.id;
                    let iid = item.issue.iid;
                    // Chord popup if statuses cached
                    if let Some(statuses) = self.work_item_statuses.get(&project)
                        && !statuses.is_empty()
                    {
                        let names: Vec<String> =
                            statuses.iter().map(|s| s.name.clone()).collect();
                        self.chord_state =
                            Some(chord_popup::ChordState::new("Set Status", names).with_kind(chord_popup::ChordKind::Status));
                        self.overlay =
                            Overlay::Chord(ChordContext::Status(project, issue_id, iid));
                        return;
                    }
                    // Fall back to fetch + picker/confirm
                    self.fetch_statuses_and_show_picker(project, issue_id, iid);
                }
            }
            issue_list::IssueListAction::EditLabels => {
                let label_names: Vec<String> = self.labels.iter().map(|l| l.name.clone()).collect();
                let current = self
                    .issue_list_state
                    .selected_issue(&self.issues)
                    .map(|i| i.issue.labels.clone())
                    .unwrap_or_default();
                self.picker_state = Some(
                    picker::PickerState::new("Labels", label_names, true)
                        .with_pre_selected(&current),
                );
                self.overlay = Overlay::Picker(PickerContext::Labels);
            }
            issue_list::IssueListAction::EditAssignee => {
                let members = self.config.team_members(self.active_team);
                self.chord_state =
                    Some(chord_popup::ChordState::new("Set Assignee", members));
                self.overlay = Overlay::Chord(ChordContext::Assignee);
            }
            issue_list::IssueListAction::Comment => {
                self.comment_input = crate::ui::components::input::InputState::default();
                self.overlay = Overlay::CommentInput;
            }
            issue_list::IssueListAction::OpenBrowser => {
                if let Some(item) = self.issue_list_state.selected_issue(&self.issues) {
                    let _ = open::that(&item.issue.web_url);
                }
            }
            issue_list::IssueListAction::AddFilter => {
                self.filter_editor_state.reset();
                self.overlay = Overlay::FilterEditor;
            }
            issue_list::IssueListAction::ClearFilters => {
                self.issue_filters.clear();
                self.refilter_issues();
            }
            issue_list::IssueListAction::PickPreset => {
                let presets: Vec<String> = self
                    .config
                    .filters
                    .iter()
                    .filter(|f| f.kind == "issue")
                    .map(|f| f.name.clone())
                    .collect();
                if !presets.is_empty() {
                    self.picker_state =
                        Some(picker::PickerState::new("Filter Preset", presets, false));
                    self.overlay = Overlay::Picker(PickerContext::Preset);
                }
            }
            issue_list::IssueListAction::PickSortPreset => {
                self.show_sort_preset_picker("issue");
            }
        }
    }

    fn handle_mr_list_key(&mut self, key: KeyEvent) {
        if keys::is_tab(&key) && !self.mr_filters.is_empty() {
            self.filter_bar_focused = true;
            self.filter_bar_selected = 0;
            return;
        }

        let action = self.mr_list_state.handle_key(&key, self.mrs.len());

        match action {
            mr_list::MrListAction::None => {}
            mr_list::MrListAction::Refilter => self.refilter_mrs(),
            mr_list::MrListAction::Refresh => {
                self.loading = true;
                self.fetch_all();
            }
            mr_list::MrListAction::OpenDetail => {
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    let project = item.project_path.clone();
                    let iid = item.mr.iid;
                    self.mr_detail_state.reset();
                    self.mr_detail_state.loading_notes = true;
                    self.fetch_notes_for_mr(&project, iid);
                    self.view_stack.push(View::MrList);
                    self.view = View::MrDetail;
                }
            }
            mr_list::MrListAction::Approve => {
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    let project = item.project_path.clone();
                    let iid = item.mr.iid;
                    self.overlay = Overlay::Confirm(ConfirmAction::ApproveMr(project, iid));
                }
            }
            mr_list::MrListAction::Merge => {
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    let project = item.project_path.clone();
                    let iid = item.mr.iid;
                    self.overlay = Overlay::Confirm(ConfirmAction::MergeMr(project, iid));
                }
            }
            mr_list::MrListAction::ToggleState => {
                // MRs can only be closed
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    let project = item.project_path.clone();
                    let iid = item.mr.iid;
                    self.overlay = Overlay::Confirm(ConfirmAction::CloseIssue(project, iid));
                }
            }
            mr_list::MrListAction::EditLabels => {
                let label_names: Vec<String> = self.labels.iter().map(|l| l.name.clone()).collect();
                let current = self
                    .mr_list_state
                    .selected_mr(&self.mrs)
                    .map(|m| m.mr.labels.clone())
                    .unwrap_or_default();
                self.picker_state = Some(
                    picker::PickerState::new("Labels", label_names, true)
                        .with_pre_selected(&current),
                );
                self.overlay = Overlay::Picker(PickerContext::Labels);
            }
            mr_list::MrListAction::EditAssignee => {
                let members = self.config.team_members(self.active_team);
                self.chord_state =
                    Some(chord_popup::ChordState::new("Set Assignee", members));
                self.overlay = Overlay::Chord(ChordContext::Assignee);
            }
            mr_list::MrListAction::Comment => {
                self.comment_input = crate::ui::components::input::InputState::default();
                self.overlay = Overlay::CommentInput;
            }
            mr_list::MrListAction::OpenBrowser => {
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    let _ = open::that(&item.mr.web_url);
                }
            }
            mr_list::MrListAction::AddFilter => {
                self.filter_editor_state.reset();
                self.overlay = Overlay::FilterEditor;
            }
            mr_list::MrListAction::ClearFilters => {
                self.mr_filters.clear();
                self.refilter_mrs();
            }
            mr_list::MrListAction::PickPreset => {
                let presets: Vec<String> = self
                    .config
                    .filters
                    .iter()
                    .filter(|f| f.kind == "merge_request")
                    .map(|f| f.name.clone())
                    .collect();
                if !presets.is_empty() {
                    self.picker_state =
                        Some(picker::PickerState::new("Filter Preset", presets, false));
                    self.overlay = Overlay::Picker(PickerContext::Preset);
                }
            }
            mr_list::MrListAction::PickSortPreset => {
                self.show_sort_preset_picker("merge_request");
            }
        }
    }

    fn handle_issue_detail_key(&mut self, key: KeyEvent) {
        if let Some(item) = self.current_detail_issue().cloned() {
            if keys::is_down(&key) {
                self.issue_detail_state.scroll_down();
                return;
            }
            if keys::is_up(&key) {
                self.issue_detail_state.scroll_up();
                return;
            }
            match key.code {
                KeyCode::Char('c') => {
                    self.comment_input = crate::ui::components::input::InputState::default();
                    self.overlay = Overlay::CommentInput;
                }
                KeyCode::Char('s') | KeyCode::Char('x') => {
                    let project = item.project_path.clone();
                    let issue_id = item.issue.id;
                    let iid = item.issue.iid;
                    self.fetch_statuses_and_show_picker(project, issue_id, iid);
                }
                KeyCode::Char('o') => {
                    let _ = open::that(&item.issue.web_url);
                }
                KeyCode::Char('l') => {
                    let label_names: Vec<String> =
                        self.labels.iter().map(|l| l.name.clone()).collect();
                    self.picker_state = Some(
                        picker::PickerState::new("Labels", label_names, true)
                            .with_pre_selected(&item.issue.labels),
                    );
                    self.overlay = Overlay::Picker(PickerContext::Labels);
                }
                KeyCode::Char('a') => {
                    let members = self.config.team_members(self.active_team);
                    self.picker_state = Some(picker::PickerState::new("Assignee", members, false));
                    self.overlay = Overlay::Picker(PickerContext::Assignee);
                }
                _ => {}
            }
        }
    }

    fn handle_mr_detail_key(&mut self, key: KeyEvent) {
        if let Some(item) = self.current_detail_mr().cloned() {
            if keys::is_down(&key) {
                self.mr_detail_state.scroll_down();
                return;
            }
            if keys::is_up(&key) {
                self.mr_detail_state.scroll_up();
                return;
            }
            match key.code {
                KeyCode::Char('c') => {
                    self.comment_input = crate::ui::components::input::InputState::default();
                    self.overlay = Overlay::CommentInput;
                }
                KeyCode::Char('A') => {
                    self.overlay = Overlay::Confirm(ConfirmAction::ApproveMr(
                        item.project_path.clone(),
                        item.mr.iid,
                    ));
                }
                KeyCode::Char('M') => {
                    self.overlay = Overlay::Confirm(ConfirmAction::MergeMr(
                        item.project_path.clone(),
                        item.mr.iid,
                    ));
                }
                KeyCode::Char('o') => {
                    let _ = open::that(&item.mr.web_url);
                }
                KeyCode::Char('l') => {
                    let label_names: Vec<String> =
                        self.labels.iter().map(|l| l.name.clone()).collect();
                    self.picker_state = Some(
                        picker::PickerState::new("Labels", label_names, true)
                            .with_pre_selected(&item.mr.labels),
                    );
                    self.overlay = Overlay::Picker(PickerContext::Labels);
                }
                KeyCode::Char('a') => {
                    let members = self.config.team_members(self.active_team);
                    self.picker_state = Some(picker::PickerState::new("Assignee", members, false));
                    self.overlay = Overlay::Picker(PickerContext::Assignee);
                }
                _ => {}
            }
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) -> bool {
        match &self.overlay {
            Overlay::Help => {
                if key.code == KeyCode::Char('?') || keys::is_back(&key) {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::FilterEditor => {
                let action = self.filter_editor_state.handle_key(&key);
                // Populate suggestions when entering value step
                if self.filter_editor_state.step == filter_editor::EditorStep::EnterValue
                    && self.filter_editor_state.suggestions.is_empty()
                {
                    self.filter_editor_state.suggestions =
                        self.get_filter_suggestions();
                }
                match action {
                    filter_editor::FilterEditorAction::Continue => {}
                    filter_editor::FilterEditorAction::Cancel => {
                        self.overlay = Overlay::None;
                    }
                    filter_editor::FilterEditorAction::AddCondition(cond) => {
                        match self.view {
                            View::IssueList | View::IssueDetail => {
                                self.issue_filters.push(cond);
                                self.refilter_issues();
                            }
                            View::MrList | View::MrDetail => {
                                self.mr_filters.push(cond);
                                self.refilter_mrs();
                            }
                            _ => {}
                        }
                        self.overlay = Overlay::None;
                    }
                }
            }
            Overlay::Confirm(action) => {
                let action = action.clone();
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        if matches!(action, ConfirmAction::QuitApp) {
                            return true;
                        }
                        self.execute_confirm(action);
                        self.overlay = Overlay::None;
                    }
                    KeyCode::Char('n') | KeyCode::Esc => {
                        self.overlay = Overlay::None;
                    }
                    _ => {}
                }
            }
            Overlay::Picker(_) => {
                if let Some(ref mut ps) = self.picker_state {
                    let action = ps.handle_key(&key);
                    match action {
                        picker::PickerAction::Continue => {}
                        picker::PickerAction::Cancel => {
                            self.picker_state = None;
                            self.overlay = Overlay::None;
                        }
                        picker::PickerAction::Picked(values) => {
                            self.handle_picker_result(values);
                            self.picker_state = None;
                            self.overlay = Overlay::None;
                        }
                    }
                }
            }
            Overlay::Chord(_) => {
                if let Some(ref mut cs) = self.chord_state {
                    let action = cs.handle_key(&key);
                    match action {
                        chord_popup::ChordAction::Continue => {}
                        chord_popup::ChordAction::Cancel => {
                            self.chord_state = None;
                            self.overlay = Overlay::None;
                        }
                        chord_popup::ChordAction::Selected(value) => {
                            self.chord_state = None;
                            self.handle_chord_result(value);
                        }
                    }
                }
            }
            Overlay::CommentInput => match key.code {
                KeyCode::Esc => {
                    self.overlay = Overlay::None;
                }
                KeyCode::Enter => {
                    let body = self.comment_input.value.clone();
                    if !body.is_empty() {
                        self.submit_comment(&body);
                    }
                    self.overlay = Overlay::None;
                }
                _ => {
                    self.comment_input.handle_key(&key);
                }
            },
            Overlay::Error(_) => {
                // Any key dismisses the error popup
                self.overlay = Overlay::None;
            }
            Overlay::None => {}
        }
        false
    }

    fn handle_filter_bar_key(&mut self, key: KeyEvent) {
        let filters = match self.view {
            View::IssueList => &mut self.issue_filters,
            View::MrList => &mut self.mr_filters,
            _ => {
                self.filter_bar_focused = false;
                return;
            }
        };

        if keys::is_back(&key) || keys::is_tab(&key) {
            self.filter_bar_focused = false;
            return;
        }

        if keys::is_left(&key) {
            self.filter_bar_selected = self.filter_bar_selected.saturating_sub(1);
        } else if keys::is_right(&key) {
            if self.filter_bar_selected + 1 < filters.len() {
                self.filter_bar_selected += 1;
            }
        } else if (key.code == KeyCode::Char('x') || key.code == KeyCode::Char('d'))
            && self.filter_bar_selected < filters.len()
        {
            filters.remove(self.filter_bar_selected);
            if self.filter_bar_selected > 0 && self.filter_bar_selected >= filters.len() {
                self.filter_bar_selected = filters.len().saturating_sub(1);
            }
            if filters.is_empty() {
                self.filter_bar_focused = false;
            }
            match self.view {
                View::IssueList => self.refilter_issues(),
                View::MrList => self.refilter_mrs(),
                _ => {}
            }
        }
    }

    fn execute_confirm(&mut self, action: ConfirmAction) {
        // Optimistic updates
        match &action {
            ConfirmAction::CloseIssue(project, iid) => {
                if let Some(pos) = self
                    .issues
                    .iter()
                    .position(|e| e.issue.iid == *iid && e.project_path == *project)
                {
                    self.issues[pos].issue.state = "closed".to_string();
                    self.refilter_issues();
                    self.save_cache();
                }
            }
            ConfirmAction::ReopenIssue(project, iid) => {
                if let Some(pos) = self
                    .issues
                    .iter()
                    .position(|e| e.issue.iid == *iid && e.project_path == *project)
                {
                    self.issues[pos].issue.state = "opened".to_string();
                    self.refilter_issues();
                    self.save_cache();
                }
            }
            ConfirmAction::MergeMr(project, iid) => {
                if let Some(pos) = self
                    .mrs
                    .iter()
                    .position(|e| e.mr.iid == *iid && e.project_path == *project)
                {
                    self.mrs[pos].mr.state = "merged".to_string();
                    self.refilter_mrs();
                    self.save_cache();
                }
            }
            _ => {}
        }

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            match action {
                ConfirmAction::CloseIssue(project, iid) => {
                    let result = client
                        .update_issue(&project, iid, serde_json::json!({"state_event": "close"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result, project));
                }
                ConfirmAction::ReopenIssue(project, iid) => {
                    let result = client
                        .update_issue(
                            &project,
                            iid,
                            serde_json::json!({"state_event": "reopen"}),
                        )
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result, project));
                }
                ConfirmAction::ApproveMr(project, iid) => {
                    let result = client
                        .approve_mr(&project, iid)
                        .await
                        .map(|_| format!("Approved !{iid}"));
                    let _ = tx.send(AsyncMsg::ActionDone(result));
                }
                ConfirmAction::MergeMr(project, iid) => {
                    let result = client.merge_mr(&project, iid).await;
                    let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                }
                ConfirmAction::QuitApp => unreachable!(),
            }
        });
    }

    fn set_issue_status(&mut self, project: &str, issue_id: u64, iid: u64, status_name: &str) {
        // Find the status ID from cached statuses
        let status_id = self
            .work_item_statuses
            .get(project)
            .and_then(|statuses| statuses.iter().find(|s| s.name == status_name))
            .map(|s| s.id.clone());

        let Some(status_id) = status_id else {
            self.show_error(format!("Status '{status_name}' not found"));
            return;
        };

        // Optimistic update
        if let Some(pos) = self
            .issues
            .iter()
            .position(|e| e.issue.iid == iid && e.project_path == project)
        {
            self.issues[pos].issue.custom_status = Some(status_name.to_string());
            self.refilter_issues();
            self.save_cache();
        }

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let project = project.to_string();
        let status_display = status_name.to_string();
        tokio::spawn(async move {
            let result = client
                .update_issue_status(issue_id, &status_id)
                .await
                .map(|_| (project, iid, status_display));
            let _ = tx.send(AsyncMsg::IssueStatusUpdated(result));
        });
    }

    fn handle_picker_result(&mut self, values: Vec<String>) {
        // Determine what we picked for based on overlay context
        let context = std::mem::replace(&mut self.overlay, Overlay::None);
        match context {
            Overlay::Picker(PickerContext::Labels) => {
                self.update_labels(values);
            }
            Overlay::Picker(PickerContext::Assignee) => {
                if let Some(username) = values.first() {
                    self.update_assignee(username);
                }
            }
            Overlay::Picker(PickerContext::Preset) => {
                if let Some(preset_name) = values.first() {
                    self.apply_preset(preset_name);
                }
            }
            Overlay::Picker(PickerContext::SortPreset) => {
                if let Some(name) = values.first() {
                    self.apply_sort_preset(name);
                }
            }
            _ => {}
        }
    }

    /// Return the index into self.issues / self.mrs for the currently selected item,
    /// plus (project_path, iid, is_mr).
    fn selected_item_idx(&self) -> Option<(usize, String, u64, bool)> {
        match self.view {
            View::IssueList | View::IssueDetail => {
                let idx = self
                    .issue_list_state
                    .table_state
                    .selected()
                    .and_then(|sel| self.issue_list_state.filtered_indices.get(sel).copied())?;
                let item = self.issues.get(idx)?;
                Some((idx, item.project_path.clone(), item.issue.iid, false))
            }
            View::MrList | View::MrDetail => {
                let idx = self
                    .mr_list_state
                    .table_state
                    .selected()
                    .and_then(|sel| self.mr_list_state.filtered_indices.get(sel).copied())?;
                let item = self.mrs.get(idx)?;
                Some((idx, item.project_path.clone(), item.mr.iid, true))
            }
            _ => None,
        }
    }

    fn update_labels(&mut self, labels: Vec<String>) {
        let Some((idx, project, iid, is_mr)) = self.selected_item_idx() else {
            return;
        };

        // Optimistic update
        if is_mr {
            self.mrs[idx].mr.labels = labels.clone();
        } else {
            self.issues[idx].issue.labels = labels.clone();
        }

        let payload = serde_json::json!({"labels": labels.join(",")});
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            if is_mr {
                let result = client.update_mr(&project, iid, payload).await;
                let _ = tx.send(AsyncMsg::MrUpdated(result, project));
            } else {
                let result = client.update_issue(&project, iid, payload).await;
                let _ = tx.send(AsyncMsg::IssueUpdated(result, project));
            }
        });
    }

    fn update_assignee(&mut self, username: &str) {
        let Some((idx, project, iid, is_mr)) = self.selected_item_idx() else {
            return;
        };

        // Optimistic update with a placeholder User
        let placeholder = User {
            id: 0,
            username: username.to_string(),
            name: username.to_string(),
            avatar_url: None,
            web_url: String::new(),
        };
        if is_mr {
            self.mrs[idx].mr.assignees = vec![placeholder];
        } else {
            self.issues[idx].issue.assignees = vec![placeholder];
        }

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let username = username.to_string();
        tokio::spawn(async move {
            let users = client.search_users(&username).await;
            match users {
                Ok(users) => {
                    if let Some(user) = users.first() {
                        let payload = serde_json::json!({"assignee_ids": [user.id]});
                        if is_mr {
                            let result = client.update_mr(&project, iid, payload).await;
                            let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                        } else {
                            let result = client.update_issue(&project, iid, payload).await;
                            let _ = tx.send(AsyncMsg::IssueUpdated(result, project));
                        }
                    } else {
                        let _ = tx.send(AsyncMsg::ActionDone(Err(anyhow::anyhow!(
                            "User '{username}' not found"
                        ))));
                    }
                }
                Err(e) => {
                    let _ = tx.send(AsyncMsg::ActionDone(Err(e)));
                }
            }
        });
    }

    fn submit_comment(&mut self, body: &str) {
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let body = body.to_string();

        let (project, iid, is_mr) = match self.view {
            View::IssueList => {
                if let Some(item) = self.issue_list_state.selected_issue(&self.issues) {
                    (item.project_path.clone(), item.issue.iid, false)
                } else {
                    return;
                }
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue() {
                    (item.project_path.clone(), item.issue.iid, false)
                } else {
                    return;
                }
            }
            View::MrList => {
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    (item.project_path.clone(), item.mr.iid, true)
                } else {
                    return;
                }
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr() {
                    (item.project_path.clone(), item.mr.iid, true)
                } else {
                    return;
                }
            }
            _ => return,
        };

        self.loading = true;
        tokio::spawn(async move {
            let create_result = if is_mr {
                client.create_mr_note(&project, iid, &body).await
            } else {
                client.create_issue_note(&project, iid, &body).await
            };
            if let Err(e) = create_result {
                let _ = tx.send(AsyncMsg::ActionDone(Err(e)));
                return;
            }
            // Re-fetch notes so the UI shows the new comment
            let notes = if is_mr {
                client.list_mr_notes(&project, iid).await
            } else {
                client.list_issue_notes(&project, iid).await
            };
            let _ = tx.send(AsyncMsg::NotesLoaded(notes));
        });
    }

    fn apply_preset(&mut self, name: &str) {
        if let Some(preset) = self.config.filters.iter().find(|f| f.name == name) {
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

            match self.view {
                View::IssueList => {
                    self.issue_filters = conditions;
                    self.refilter_issues();
                }
                View::MrList => {
                    self.mr_filters = conditions;
                    self.refilter_mrs();
                }
                _ => {}
            }
        }
    }

    fn current_detail_issue(&self) -> Option<&TrackedIssue> {
        // The detail view shows the issue that was selected when we opened it
        self.issue_list_state.selected_issue(&self.issues)
    }

    fn current_detail_mr(&self) -> Option<&TrackedMergeRequest> {
        self.mr_list_state.selected_mr(&self.mrs)
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Min(1),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(area);

        let ctx = crate::ui::RenderCtx {
            label_colors: &self.label_color_map,
        };

        // Render main view
        match self.view {
            View::Dashboard => {
                dashboard::render(
                    frame,
                    chunks[0],
                    &self.config,
                    self.active_team,
                    &self.issues,
                    &self.mrs,
                    self.loading,
                );
            }
            View::IssueList => {
                issue_list::render(
                    frame,
                    chunks[0],
                    &mut self.issue_list_state,
                    &self.issues,
                    &self.issue_filters,
                    self.filter_bar_focused,
                    self.filter_bar_selected,
                    &ctx,
                );
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue().cloned() {
                    issue_detail::render(
                        frame,
                        chunks[0],
                        &item,
                        &self.issue_detail_state,
                        &ctx,
                    );
                }
            }
            View::MrList => {
                mr_list::render(
                    frame,
                    chunks[0],
                    &mut self.mr_list_state,
                    &self.mrs,
                    &self.mr_filters,
                    self.filter_bar_focused,
                    self.filter_bar_selected,
                    &ctx,
                );
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr().cloned() {
                    mr_detail::render(
                        frame,
                        chunks[0],
                        &item,
                        &self.mr_detail_state,
                        &ctx,
                    );
                }
            }
        }

        // Status bar
        let team_name = self
            .config
            .teams
            .get(self.active_team)
            .map(|t| t.name.as_str())
            .unwrap_or("all");
        let view_name = match self.view {
            View::Dashboard => "Dashboard",
            View::IssueList => "Issues",
            View::IssueDetail => "Issue Detail",
            View::MrList => "Merge Requests",
            View::MrDetail => "MR Detail",
        };
        let item_count = match self.view {
            View::IssueList => self.issue_list_state.filtered_indices.len(),
            View::MrList => self.mr_list_state.filtered_indices.len(),
            _ => self.issues.len() + self.mrs.len(),
        };
        crate::ui::components::status_bar::render(
            frame,
            chunks[1],
            view_name,
            team_name,
            item_count,
            self.loading,
            self.loading_msg,
            self.error.as_deref(),
            self.last_fetched_at,
        );

        // Render overlay on top
        match &self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                let help_ctx = match self.view {
                    View::IssueList | View::MrList => "list",
                    View::IssueDetail | View::MrDetail => "detail",
                    View::Dashboard => "all",
                };
                help::render(frame, area, help_ctx);
            }
            Overlay::FilterEditor => {
                filter_editor::render(
                    frame,
                    area,
                    &mut self.filter_editor_state,
                    &ctx,
                );
            }
            Overlay::Confirm(action) => {
                let (title, msg) = match action {
                    ConfirmAction::CloseIssue(_, iid) => {
                        ("Close Issue", format!("Close issue #{iid}?"))
                    }
                    ConfirmAction::ReopenIssue(_, iid) => {
                        ("Reopen Issue", format!("Reopen issue #{iid}?"))
                    }
                    ConfirmAction::ApproveMr(_, iid) => {
                        ("Approve MR", format!("Approve MR !{iid}?"))
                    }
                    ConfirmAction::MergeMr(_, iid) => ("Merge MR", format!("Merge MR !{iid}?")),
                    ConfirmAction::QuitApp => ("Quit", "Quit glab-dash?".to_string()),
                };
                confirm_dialog::render(frame, area, title, &msg);
            }
            Overlay::Picker(_) => {
                if let Some(ref mut ps) = self.picker_state {
                    picker::render(frame, area, ps, &ctx);
                }
            }
            Overlay::CommentInput => {
                let popup = centered_rect(60, 15, area);
                ratatui::widgets::Clear.render(popup, frame.buffer_mut());
                crate::ui::components::input::render(
                    frame,
                    popup,
                    &self.comment_input,
                    "Comment (Enter to submit, Esc to cancel)",
                );
            }
            Overlay::Chord(_) => {
                if let Some(ref cs) = self.chord_state {
                    chord_popup::render(frame, area, cs);
                }
            }
            Overlay::Error(msg) => {
                error_popup::render(frame, area, msg);
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
