use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::filter::{Field, FilterCondition, Op};
use crate::gitlab::client::GitLabClient;
use crate::gitlab::types::*;
use crate::ui::components::{confirm_dialog, error_popup, help, picker};
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
}

/// Messages from async operations
pub enum AsyncMsg {
    IssuesLoaded(Result<(Vec<TrackedIssue>, Vec<TrackedIssue>)>),
    MrsLoaded(Result<(Vec<TrackedMergeRequest>, Vec<TrackedMergeRequest>)>),
    NotesLoaded(Result<Vec<Note>>),
    ActionDone(Result<String>),
    LabelsLoaded(Result<Vec<ProjectLabel>>),
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
    pub loading: bool,
    pub error: Option<String>,

    // View-specific state
    pub issue_list_state: issue_list::IssueListState,
    pub mr_list_state: mr_list::MrListState,
    pub issue_detail_state: issue_detail::IssueDetailState,
    pub mr_detail_state: mr_detail::MrDetailState,
    pub filter_editor_state: filter_editor::FilterEditorState,
    pub picker_state: Option<picker::PickerState>,
    pub comment_input: crate::ui::components::input::InputState,

    // Filter state
    pub issue_filters: Vec<FilterCondition>,
    pub mr_filters: Vec<FilterCondition>,
    pub filter_bar_focused: bool,
    pub filter_bar_selected: usize,
    // (detail context uses list selection state)
}

impl App {
    pub fn new(
        config: Config,
        client: GitLabClient,
        async_tx: mpsc::UnboundedSender<AsyncMsg>,
    ) -> Self {
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
            loading: false,
            error: None,
            issue_list_state: issue_list::IssueListState::default(),
            mr_list_state: mr_list::MrListState::default(),
            issue_detail_state: issue_detail::IssueDetailState::default(),
            mr_detail_state: mr_detail::MrDetailState::default(),
            filter_editor_state: filter_editor::FilterEditorState::default(),
            picker_state: None,
            comment_input: crate::ui::components::input::InputState::default(),
            issue_filters: Vec::new(),
            mr_filters: Vec::new(),
            filter_bar_focused: false,
            filter_bar_selected: 0,
        }
    }

    pub fn fetch_all(&self) {
        self.fetch_issues();
        self.fetch_mrs();
        self.fetch_labels();
    }

    fn fetch_issues(&self) {
        let client = self.client.clone();
        let members = self.config.team_members(self.active_team);
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let tracking = client.fetch_tracking_issues("opened").await;
            let external = client.fetch_external_issues(&members, "opened").await;
            let result = match (tracking, external) {
                (Ok(t), Ok(e)) => Ok((t, e)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::IssuesLoaded(result));
        });
    }

    fn fetch_mrs(&self) {
        let client = self.client.clone();
        let members = self.config.team_members(self.active_team);
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let tracking = client.fetch_tracking_mrs("opened").await;
            let external = client.fetch_external_mrs(&members, "opened").await;
            let result = match (tracking, external) {
                (Ok(t), Ok(e)) => Ok((t, e)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::MrsLoaded(result));
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
            AsyncMsg::IssuesLoaded(result) => match result {
                Ok((tracking, external)) => {
                    self.issues = tracking;
                    self.issues.extend(external);
                    self.loading = false;
                    self.error = None;
                    self.refilter_issues();
                }
                Err(e) => {
                    self.loading = false;
                    self.show_error(format!("Issues: {e}"));
                }
            },
            AsyncMsg::MrsLoaded(result) => match result {
                Ok((tracking, external)) => {
                    self.mrs = tracking;
                    self.mrs.extend(external);
                    self.loading = false;
                    self.error = None;
                    self.refilter_mrs();
                }
                Err(e) => {
                    self.loading = false;
                    self.show_error(format!("MRs: {e}"));
                }
            },
            AsyncMsg::NotesLoaded(result) => match result {
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
            },
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
            AsyncMsg::LabelsLoaded(result) => {
                if let Ok(labels) = result {
                    self.labels = labels;
                }
            }
        }
    }

    fn show_error(&mut self, msg: String) {
        self.error = Some(msg.clone());
        self.overlay = Overlay::Error(msg);
    }

    fn refilter_issues(&mut self) {
        let me = self.config.me.clone();
        let members = self.config.team_members(self.active_team);
        self.issue_list_state
            .apply_filters(&self.issues, &self.issue_filters, &me, &members);
    }

    fn refilter_mrs(&mut self) {
        let me = self.config.me.clone();
        let members = self.config.team_members(self.active_team);
        self.mr_list_state
            .apply_filters(&self.mrs, &self.mr_filters, &me, &members);
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
                self.loading = true;
                self.fetch_all();
                return false;
            }
        }

        // Navigation
        if keys::is_back(&key) {
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
                    let iid = item.issue.iid;
                    let action = if item.issue.state == "opened" {
                        ConfirmAction::CloseIssue(project, iid)
                    } else {
                        ConfirmAction::ReopenIssue(project, iid)
                    };
                    self.overlay = Overlay::Confirm(action);
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
                self.picker_state = Some(picker::PickerState::new("Assignee", members, false));
                self.overlay = Overlay::Picker(PickerContext::Assignee);
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
                self.picker_state = Some(picker::PickerState::new("Assignee", members, false));
                self.overlay = Overlay::Picker(PickerContext::Assignee);
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
        }
    }

    fn handle_issue_detail_key(&mut self, key: KeyEvent) {
        if let Some(item) = self.current_detail_issue().cloned() {
            match key.code {
                KeyCode::Char('j') => self.issue_detail_state.scroll_down(),
                KeyCode::Char('k') => self.issue_detail_state.scroll_up(),
                KeyCode::Char('c') => {
                    self.comment_input = crate::ui::components::input::InputState::default();
                    self.overlay = Overlay::CommentInput;
                }
                KeyCode::Char('x') => {
                    let action = if item.issue.state == "opened" {
                        ConfirmAction::CloseIssue(item.project_path.clone(), item.issue.iid)
                    } else {
                        ConfirmAction::ReopenIssue(item.project_path.clone(), item.issue.iid)
                    };
                    self.overlay = Overlay::Confirm(action);
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
            match key.code {
                KeyCode::Char('j') => self.mr_detail_state.scroll_down(),
                KeyCode::Char('k') => self.mr_detail_state.scroll_up(),
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
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        self.loading = true;
        tokio::spawn(async move {
            let result = match action {
                ConfirmAction::CloseIssue(project, iid) => client
                    .close_issue(&project, iid)
                    .await
                    .map(|_| format!("Closed #{iid}")),
                ConfirmAction::ReopenIssue(project, iid) => client
                    .reopen_issue(&project, iid)
                    .await
                    .map(|_| format!("Reopened #{iid}")),
                ConfirmAction::ApproveMr(project, iid) => client
                    .approve_mr(&project, iid)
                    .await
                    .map(|_| format!("Approved !{iid}")),
                ConfirmAction::MergeMr(project, iid) => client
                    .merge_mr(&project, iid)
                    .await
                    .map(|_| format!("Merged !{iid}")),
                ConfirmAction::QuitApp => unreachable!(),
            };
            let _ = tx.send(AsyncMsg::ActionDone(result));
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
            _ => {}
        }
    }

    fn update_labels(&mut self, labels: Vec<String>) {
        let (project, iid, _is_mr) = match self.view {
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

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        self.loading = true;
        tokio::spawn(async move {
            let result = client
                .update_issue_labels(&project, iid, &labels)
                .await
                .map(|_| "Labels updated".to_string());
            let _ = tx.send(AsyncMsg::ActionDone(result));
        });
    }

    fn update_assignee(&mut self, username: &str) {
        // For simplicity, we search users and use the first match
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let username = username.to_string();
        let (project, iid) = match self.view {
            View::IssueList => {
                if let Some(item) = self.issue_list_state.selected_issue(&self.issues) {
                    (item.project_path.clone(), item.issue.iid)
                } else {
                    return;
                }
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue() {
                    (item.project_path.clone(), item.issue.iid)
                } else {
                    return;
                }
            }
            View::MrList => {
                if let Some(item) = self.mr_list_state.selected_mr(&self.mrs) {
                    (item.project_path.clone(), item.mr.iid)
                } else {
                    return;
                }
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr() {
                    (item.project_path.clone(), item.mr.iid)
                } else {
                    return;
                }
            }
            _ => return,
        };

        self.loading = true;
        tokio::spawn(async move {
            let users = client.search_users(&username).await;
            let result = match users {
                Ok(users) => {
                    if let Some(user) = users.first() {
                        client
                            .update_issue_assignees(&project, iid, &[user.id])
                            .await
                            .map(|_| format!("Assigned to @{username}"))
                    } else {
                        Err(anyhow::anyhow!("User '{username}' not found"))
                    }
                }
                Err(e) => Err(e),
            };
            let _ = tx.send(AsyncMsg::ActionDone(result));
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
            let result = if is_mr {
                client
                    .create_mr_note(&project, iid, &body)
                    .await
                    .map(|_| "Comment added".to_string())
            } else {
                client
                    .create_issue_note(&project, iid, &body)
                    .await
                    .map(|_| "Comment added".to_string())
            };
            let _ = tx.send(AsyncMsg::ActionDone(result));
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
                );
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue().cloned() {
                    issue_detail::render(frame, chunks[0], &item, &self.issue_detail_state);
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
                );
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr().cloned() {
                    mr_detail::render(frame, chunks[0], &item, &self.mr_detail_state);
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
            self.error.as_deref(),
        );

        // Render overlay on top
        match &self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                let ctx = match self.view {
                    View::IssueList | View::MrList => "list",
                    View::IssueDetail | View::MrDetail => "detail",
                    View::Dashboard => "all",
                };
                help::render(frame, area, ctx);
            }
            Overlay::FilterEditor => {
                filter_editor::render(frame, area, &mut self.filter_editor_state);
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
                    picker::render(frame, area, ps);
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
