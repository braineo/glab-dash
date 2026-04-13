use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::Widget;
use tokio::sync::mpsc;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::cmd::{Cmd, Dirty};
use crate::config::Config;
use crate::db::{Db, ViewState};
use crate::filter::{Field, FilterCondition, Op};
use crate::gitlab::client::GitLabClient;
use crate::gitlab::types::{
    Discussion, Issue, Iteration, MergeRequest, ProjectLabel, TrackedIssue, TrackedMergeRequest,
    User, WorkItemStatus,
};
use crate::keybindings::{self, InputMode, KeyAction};
use crate::ui::components::{
    chord_popup, confirm_dialog, error_popup, help, input, label_editor, picker,
};
use crate::ui::keys;
use crate::ui::views::list_model::UserFilter;
use crate::ui::views::{
    dashboard, filter_editor, issue_detail, issue_list, mr_detail, mr_list, planning,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    IssueList,
    IssueDetail,
    MrList,
    MrDetail,
    Planning,
}

#[derive(Debug)]
pub enum Overlay {
    None,
    Help,
    FilterEditor,
    Confirm(ConfirmAction),
    Picker(PickerContext),
    Chord(ChordContext),
    LabelEditor,
    CommentInput,
    Error(String),
}

/// The item currently under the cursor or open in detail view.
/// Single source of truth — rebuilt on every view/selection change via `refresh_focused()`.
/// Key handlers, status bar, and help overlay all read from this.
#[derive(Debug, Clone)]
pub enum FocusedItem {
    Issue { project: String, id: u64, iid: u64 },
    Mr { project: String, iid: u64 },
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    /// (issue_id, iid) — iid used only for confirm dialog text
    CloseIssue(u64, u64),
    /// (issue_id, iid) — iid used only for confirm dialog text
    ReopenIssue(u64, u64),
    CloseMr(String, u64),
    ApproveMr(String, u64),
    MergeMr(String, u64),
    QuitApp,
}

#[derive(Debug)]
pub enum PickerContext {
    Assignee,
    Team,
    /// Reply to a discussion thread; stores thread metadata parallel to picker items.
    ReplyThread(Vec<ThreadPickerInfo>),
}

/// Metadata for a single thread shown in the reply picker.
#[derive(Debug)]
pub struct ThreadPickerInfo {
    pub discussion_id: String,
    pub author: String,
    pub preview: String,
    pub last_author: Option<String>,
    pub last_preview: Option<String>,
    pub reply_count: usize,
}

/// Context for the chord popup overlay (what action to perform on selection).
#[derive(Debug)]
pub enum ChordContext {
    /// Set issue status: (`project_path`, `issue_db_id`, `issue_iid`)
    Status(String, u64, u64),
    Assignee,
    /// Move issue to iteration: (`issue_index` in self.issues)
    Iteration(usize),
    /// Sort: pick a field
    SortField,
    /// Sort: pick direction for chosen field (field_name, optional label_scope)
    SortDirection(String, Option<String>),
    /// Filter menu: presets, existing conditions, add/clear
    FilterMenu,
    /// Filter: pick a field for new condition
    FilterField,
    /// Filter: pick an operator for chosen field
    FilterOp(crate::filter::Field),
}

/// Messages from async operations
pub enum AsyncMsg {
    IssuesLoaded(Result<Vec<TrackedIssue>>, bool),
    MrsLoaded(
        Result<(Vec<TrackedMergeRequest>, Vec<TrackedMergeRequest>)>,
        bool,
    ),
    DiscussionsLoaded(Result<Vec<Discussion>>),
    ActionDone(Result<String>),
    /// An issue was mutated; carry the updated object.
    IssueUpdated(Result<Issue>),
    /// A merge request was mutated; carry the updated object and project path.
    MrUpdated(Result<MergeRequest>, String),
    /// Issue custom status changed: (`project_path`, iid, `new_status_name`).
    IssueStatusUpdated(Result<(String, u64, String)>),
    LabelsLoaded(Result<Vec<ProjectLabel>>),
    /// (statuses, project, `issue_db_id`, iid, `close_only`)
    StatusesLoaded(Result<Vec<WorkItemStatus>>, String, u64, u64, bool),
    IterationsLoaded(Result<Vec<Iteration>>),
    /// Iteration update result: (result, `issue_id`, `new_iteration`)
    IterationUpdated(Result<()>, u64, Option<Iteration>),
    /// Unplanned work: issue_id → added_to_iteration_at timestamp
    UnplannedWorkLoaded(Result<std::collections::HashMap<u64, chrono::DateTime<chrono::Utc>>>),
}

/// Lifecycle of an async health data fetch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FetchState {
    /// Data has not been requested yet.
    #[default]
    Idle,
    /// Async request is in flight.
    InFlight,
    /// Data has been received (success or error).
    Done,
}

pub struct App {
    pub config: Config,
    pub client: GitLabClient,
    pub async_tx: mpsc::UnboundedSender<AsyncMsg>,
    pub db: Db,

    // View state
    pub view: View,
    pub view_stack: Vec<View>,
    pub overlay: Overlay,
    pub active_team: Option<usize>,

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
    pub comment_input: crate::ui::components::input::CommentInput,
    pub autocomplete: crate::ui::components::autocomplete::AutocompleteState,
    /// When set, the next comment submission replies to this discussion thread.
    pub reply_discussion_id: Option<String>,

    // Cache / incremental fetch
    pub last_fetched_at: Option<u64>,
    /// Timestamp (ms) when the current fetch cycle started.
    pub fetch_started_at: Option<u64>,
    /// Duration of the last completed fetch cycle (ms), shown in status bar.
    pub last_fetch_ms: Option<u64>,

    // Work item statuses per project (project_path -> available statuses)
    pub work_item_statuses: std::collections::HashMap<String, Vec<WorkItemStatus>>,

    // Centralized focused item — single source of truth for key handling & UI
    pub focused: Option<FocusedItem>,

    // Chord popup state (vim-style easymotion codes)
    pub chord_state: Option<chord_popup::ChordState>,

    // Label editor state (chord + search dual-mode)
    pub label_editor_state: Option<label_editor::LabelEditorState>,
    pub label_usage: std::collections::HashMap<String, u32>,

    pub label_sort_orders: std::collections::HashMap<String, Vec<String>>,

    // Planning view
    pub planning_state: planning::PlanningViewState,
    pub iterations: Vec<Iteration>,

    // Iteration board on dashboard
    pub iteration_board_state: dashboard::IterationBoardState,

    // Iteration health
    pub iteration_health: Option<dashboard::IterationHealth>,
    pub unplanned_work_cache: std::collections::HashMap<u64, chrono::DateTime<chrono::Utc>>,
    pub shadow_work_cache: Vec<TrackedIssue>,
    pub unplanned_work_state: FetchState,

    // Redraw flag — only render when state has changed
    pub needs_redraw: bool,

    // TEA accumulators — filled during update, drained by event loop
    dirty: Dirty,
    pending_cmds: Vec<Cmd>,
}

impl App {
    pub fn new(
        config: Config,
        client: GitLabClient,
        async_tx: mpsc::UnboundedSender<AsyncMsg>,
        db: Db,
    ) -> Self {
        let label_sort_orders = config
            .label_sort_orders
            .iter()
            .map(|o| (o.scope.clone(), o.values.clone()))
            .collect();
        let active_team = None;
        Self {
            config,
            client,
            async_tx,
            db,
            view: View::Dashboard,
            view_stack: Vec::new(),
            overlay: Overlay::None,
            active_team,
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
            comment_input: crate::ui::components::input::CommentInput::default(),
            autocomplete: crate::ui::components::autocomplete::AutocompleteState::default(),
            reply_discussion_id: None,
            last_fetched_at: None,
            fetch_started_at: None,
            last_fetch_ms: None,
            work_item_statuses: std::collections::HashMap::new(),
            focused: None,
            chord_state: None,
            label_editor_state: None,
            label_usage: std::collections::HashMap::new(),
            label_sort_orders,
            planning_state: planning::PlanningViewState::default(),
            iterations: Vec::new(),
            iteration_board_state: dashboard::IterationBoardState::default(),
            iteration_health: None,
            unplanned_work_cache: std::collections::HashMap::new(),
            shadow_work_cache: Vec::new(),
            unplanned_work_state: FetchState::default(),
            needs_redraw: true,
            dirty: Dirty::default(),
            pending_cmds: Vec::new(),
        }
    }

    /// Load cached data for instant startup display.
    /// Load persisted data from SQLite for instant startup display.
    pub fn load_from_db(&mut self) {
        // Load open issues and MRs for display
        self.issues = self.db.load_issues(Some("opened")).unwrap_or_default();
        self.mrs = self.db.load_mrs(Some("opened")).unwrap_or_default();
        self.labels = self.db.load_labels().unwrap_or_default();
        self.work_item_statuses = self.db.load_work_item_statuses().unwrap_or_default();

        // Load key-value metadata
        if let Ok(Some(usage)) = self.db.get_kv("label_usage") {
            self.label_usage = usage;
        }
        // Restore last_fetched_at so the first fetch is incremental (fast)
        if let Ok(Some(ts)) = self.db.get_kv::<u64>("last_fetched_at") {
            self.last_fetched_at = Some(ts);
        }

        // Restore persisted view state (filters, sorts, fuzzy queries)
        if let Ok(Some(vs)) = self.db.get_kv::<ViewState>("issue_view_state") {
            self.issue_list_state.filter.conditions = vs.conditions;
            self.issue_list_state.filter.sort_specs = vs.sort_specs;
            self.issue_list_state.filter.fuzzy_query = vs.fuzzy_query;
        }
        if let Ok(Some(vs)) = self.db.get_kv::<ViewState>("mr_view_state") {
            self.mr_list_state.filter.conditions = vs.conditions;
            self.mr_list_state.filter.sort_specs = vs.sort_specs;
            self.mr_list_state.filter.fuzzy_query = vs.fuzzy_query;
        }

        // Restore iterations (before health data so classify_iterations sees them)
        self.iterations = self.db.load_iterations().unwrap_or_default();
        if !self.iterations.is_empty() {
            self.classify_iterations();
        }

        // Restore unplanned work dates
        if let Ok(Some(dates)) = self.db.get_kv("unplanned_work_dates") {
            self.unplanned_work_cache = dates;
            self.unplanned_work_state = FetchState::Done;
        }

        self.refresh_shadow_work();
        self.rebuild_label_color_map();
        self.refilter_issues();
        self.refilter_mrs();
        self.rebuild_iteration_board_columns();
        self.refilter_iteration_board();
        self.refilter_planning();
        self.compute_iteration_health();
    }

    /// Rebuild `self.focused` from the current view + selection.
    /// Call after every view change, list selection change, or data load.
    fn refresh_focused(&mut self) {
        self.focused = match self.view {
            View::IssueList | View::IssueDetail => self
                .issue_list_state
                .selected_issue(&self.issues)
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
            View::MrList | View::MrDetail => {
                self.mr_list_state
                    .selected_mr(&self.mrs)
                    .map(|item| FocusedItem::Mr {
                        project: item.project_path.clone(),
                        iid: item.mr.iid,
                    })
            }
            View::Planning => self
                .planning_state
                .selected_issue(&self.issues)
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
            View::Dashboard if self.iteration_board_state.health_focused => self
                .iteration_health
                .as_ref()
                .and_then(|h| h.selected_issue(&self.issues, &self.shadow_work_cache))
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
            View::Dashboard => {
                self.iteration_board_state
                    .selected_issue(&self.issues)
                    .map(|item| FocusedItem::Issue {
                        project: item.project_path.clone(),
                        id: item.issue.id,
                        iid: item.issue.iid,
                    })
            }
        };
    }

    /// Get members for the active team, or empty vec for "All" view.
    /// Used for implicit team filtering — empty means no filter.
    fn active_team_members(&self) -> Vec<String> {
        match self.active_team {
            Some(idx) => self.config.team_members(idx),
            None => Vec::new(),
        }
    }

    /// Get member list for pickers (assignee, filter suggestions).
    /// Returns all configured members in "All" mode, team members otherwise.
    fn picker_members(&self) -> Vec<String> {
        match self.active_team {
            Some(idx) => self.config.team_members(idx),
            None => self.config.all_members(),
        }
    }

    fn rebuild_label_color_map(&mut self) {
        self.label_color_map = self
            .labels
            .iter()
            .filter_map(|l| Some((l.name.clone(), l.color.clone()?)))
            .collect();
    }

    // ── TEA: reconcile + execute ────────────────────────────────────

    /// Run all downstream updates implied by the dirty flags, then clear
    /// the flags.  This is the **single place** where refilter / refresh /
    /// health calls live — individual handlers never call them directly.
    fn reconcile(&mut self) {
        // Copy flags to avoid borrowing self while calling &mut self methods.
        let d = std::mem::take(&mut self.dirty);
        if !d.any() {
            return;
        }

        if d.issues || d.view_state {
            self.refilter_issues();
        }
        if d.issues || d.iterations || d.view_state {
            self.refilter_planning();
        }
        if d.mrs || d.view_state {
            self.refilter_mrs();
        }
        if d.statuses {
            self.rebuild_iteration_board_columns();
        }
        if d.issues || d.iterations || d.statuses || d.view_state {
            self.refilter_iteration_board();
        }
        if d.issues || d.mrs || d.iterations || d.selection || d.view_state || d.statuses {
            self.refresh_focused();
        }
        if d.issues || d.iterations || d.statuses {
            self.compute_iteration_health();
        }
    }

    /// Drain `pending_cmds` and execute each side-effect.
    fn execute_pending_cmds(&mut self) {
        let cmds = std::mem::take(&mut self.pending_cmds);
        for cmd in cmds {
            self.execute_cmd(cmd);
        }
    }

    /// Execute a single side-effect command.
    fn execute_cmd(&mut self, cmd: Cmd) {
        match cmd {
            // ── Persistence (targeted SQLite writes) ─────────────────
            Cmd::PersistIssues => {
                let _ = self.db.upsert_issues(&self.issues);
            }
            Cmd::PersistMrs => {
                let _ = self.db.upsert_mrs(&self.mrs);
            }
            Cmd::PersistLabels => {
                let _ = self.db.upsert_labels(&self.labels);
            }
            Cmd::PersistIterations => {
                let _ = self.db.upsert_iterations(&self.iterations);
            }
            Cmd::PersistStatuses { ref project } => {
                if let Some(statuses) = self.work_item_statuses.get(project) {
                    let _ = self.db.set_work_item_statuses(project, statuses);
                }
            }
            Cmd::PersistViewState => {
                let ivs = ViewState {
                    conditions: self.issue_list_state.filter.conditions.clone(),
                    sort_specs: self.issue_list_state.filter.sort_specs.clone(),
                    fuzzy_query: self.issue_list_state.filter.fuzzy_query.clone(),
                };
                let mvs = ViewState {
                    conditions: self.mr_list_state.filter.conditions.clone(),
                    sort_specs: self.mr_list_state.filter.sort_specs.clone(),
                    fuzzy_query: self.mr_list_state.filter.fuzzy_query.clone(),
                };
                let _ = self.db.set_kv("issue_view_state", &ivs);
                let _ = self.db.set_kv("mr_view_state", &mvs);
            }
            Cmd::PersistUnplannedWork => {
                let _ = self
                    .db
                    .set_kv("unplanned_work_dates", &self.unplanned_work_cache);
            }
            Cmd::PersistLabelUsage => {
                let _ = self.db.set_kv("label_usage", &self.label_usage);
            }

            // ── API fetches ──────────────────────────────────────────
            Cmd::FetchAll => {
                self.fetch_started_at = Some(Self::now_millis());
                self.fetch_all();
            }
            Cmd::FetchAllFull => {
                self.last_fetched_at = None;
                self.unplanned_work_state = FetchState::Idle;
                self.fetch_started_at = Some(Self::now_millis());
                self.fetch_all();
            }
            Cmd::FetchHealthData => self.maybe_fetch_health_data(),

            // ── API mutations ────────────────────────────────────────
            Cmd::SpawnCloseIssue { issue_id } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue(issue_id, serde_json::json!({"stateEvent": "CLOSE"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result));
                });
            }
            Cmd::SpawnReopenIssue { issue_id } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue(issue_id, serde_json::json!({"stateEvent": "REOPEN"}))
                        .await;
                    let _ = tx.send(AsyncMsg::IssueUpdated(result));
                });
            }
            Cmd::SpawnCloseMr { project, iid } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_mr(&project, iid, serde_json::json!({"state_event": "close"}))
                        .await;
                    let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                });
            }
            Cmd::SpawnApproveMr { project, iid } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .approve_mr(&project, iid)
                        .await
                        .map(|()| format!("Approved !{iid}"));
                    let _ = tx.send(AsyncMsg::ActionDone(result));
                });
            }
            Cmd::SpawnMergeMr { project, iid } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client.merge_mr(&project, iid).await;
                    let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                });
            }
            Cmd::SpawnMoveIteration {
                issue_id,
                target_gid,
                old_iteration,
            } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue_iteration(issue_id, target_gid.as_deref())
                        .await;
                    let _ = tx.send(AsyncMsg::IterationUpdated(result, issue_id, old_iteration));
                });
            }
            Cmd::SpawnSetStatus {
                project,
                issue_id,
                iid,
                status_id,
                status_display,
            } => {
                let client = self.client.clone();
                let tx = self.async_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_issue_status(issue_id, &status_id)
                        .await
                        .map(|()| (project, iid, status_display));
                    let _ = tx.send(AsyncMsg::IssueStatusUpdated(result));
                });
            }
        }
    }

    /// Process an async message: update state, reconcile, execute side-effects.
    pub fn process_async_msg(&mut self, msg: AsyncMsg) {
        self.dirty = Dirty::default();
        self.pending_cmds.clear();
        self.handle_async_msg(msg);
        self.reconcile();
        self.execute_pending_cmds();
    }

    /// Process a key event: update state, reconcile, execute side-effects.
    /// Returns `true` if the app should quit.
    pub fn process_key(&mut self, key: KeyEvent) -> bool {
        self.dirty = Dirty::default();
        self.pending_cmds.clear();
        self.needs_redraw = true;
        let quit = self.handle_key(key);
        self.reconcile();
        self.execute_pending_cmds();
        quit
    }

    pub fn fetch_all(&self) {
        self.fetch_issues();
        self.fetch_mrs();
        self.fetch_labels();
        self.fetch_iterations();
        self.fetch_statuses_for_board();
    }

    /// Fetch work item statuses for each tracking project (for the iteration board).
    fn fetch_statuses_for_board(&self) {
        for project in &self.config.tracking_projects {
            if self.work_item_statuses.contains_key(project) {
                continue; // already cached
            }
            let client = self.client.clone();
            let tx = self.async_tx.clone();
            let project = project.clone();
            tokio::spawn(async move {
                let result = client.fetch_work_item_statuses(&project).await;
                // Reuse StatusesLoaded with sentinel values (issue_id=0, iid=0)
                // to indicate this is a background fetch, not a chord popup trigger.
                let _ = tx.send(AsyncMsg::StatusesLoaded(result, project, 0, 0, false));
            });
        }
    }

    /// Convert a unix timestamp to ISO 8601 for the GitLab API, with 60s safety buffer.
    fn updated_after_param(ts: u64) -> String {
        let buffered = ts.saturating_sub(60);
        chrono::DateTime::from_timestamp(i64::try_from(buffered).unwrap_or(i64::MAX), 0)
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

    pub fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    /// Record fetch duration. Called by each data handler; the last one to arrive
    /// captures the total wall-clock time from `fetch_all()`.
    fn record_fetch_done(&mut self) {
        self.loading = false;
        if let Some(started) = self.fetch_started_at {
            self.last_fetch_ms = Some(Self::now_millis().saturating_sub(started));
        }
    }

    fn fetch_issues(&self) {
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let updated_after = self.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        let members = self.config.all_members();
        tokio::spawn(async move {
            let ua = updated_after.as_deref();
            let (tracking, assigned) = tokio::join!(
                client.fetch_tracking_issues(None, ua),
                client.fetch_assigned_issues(&members, None, ua),
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
        let members = self.config.all_members();
        let tx = self.async_tx.clone();
        let updated_after = self.last_fetched_at.map(Self::updated_after_param);
        let incremental = updated_after.is_some();
        tokio::spawn(async move {
            let ua = updated_after.as_deref();
            let tracking = client.fetch_tracking_mrs("all", ua).await;
            let external = client.fetch_external_mrs(&members, "all", ua).await;
            let result = match (tracking, external) {
                (Ok(t), Ok(e)) => Ok((t, e)),
                (Err(e), _) | (_, Err(e)) => Err(e),
            };
            let _ = tx.send(AsyncMsg::MrsLoaded(result, incremental));
        });
    }

    fn fetch_labels(&self) {
        let client = self.client.clone();
        let projects = self.config.tracking_projects.clone();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let mut all_labels = Vec::new();
            let mut seen_ids = std::collections::HashSet::new();
            for project in &projects {
                if let Ok(labels) = client.list_project_labels(project).await {
                    for label in labels {
                        if seen_ids.insert(label.id) {
                            all_labels.push(label);
                        }
                    }
                }
            }
            let _ = tx.send(AsyncMsg::LabelsLoaded(Ok(all_labels)));
        });
    }

    fn fetch_notes_for_issue(&self, project: &str, iid: u64) {
        let client = self.client.clone();
        let project = project.to_string();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_issue_discussions(&project, iid).await;
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(result));
        });
    }

    /// Fetch work-item statuses and show a chord popup.
    /// `close_only`: when true, filter to close-category statuses (for `x` key).
    fn fetch_statuses_and_show_chord(
        &mut self,
        project: &str,
        issue_id: u64,
        iid: u64,
        close_only: bool,
    ) {
        // If we already have cached statuses for this project, show chord immediately
        if let Some(statuses) = self.work_item_statuses.get(project) {
            if statuses.is_empty() {
                // No custom statuses — fall back to open/close toggle
                let item_state = self
                    .issue_list_state
                    .selected_issue(&self.issues)
                    .or_else(|| self.current_detail_issue())
                    .map_or("opened", |i| i.issue.state.as_str());
                let action = if item_state == "opened" {
                    ConfirmAction::CloseIssue(issue_id, iid)
                } else {
                    ConfirmAction::ReopenIssue(issue_id, iid)
                };
                self.overlay = Overlay::Confirm(action);
            } else {
                self.show_status_chord(project, issue_id, iid, close_only);
            }
            return;
        }

        // Fetch statuses from GitLab
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let project = project.to_string();
        self.loading = true;
        tokio::spawn(async move {
            let result = client.fetch_work_item_statuses(&project).await;
            let _ = tx.send(AsyncMsg::StatusesLoaded(
                result, project, issue_id, iid, close_only,
            ));
        });
    }

    /// Build and display the status chord popup from cached statuses.
    fn show_status_chord(&mut self, project: &str, issue_id: u64, iid: u64, close_only: bool) {
        let Some(statuses) = self.work_item_statuses.get(project) else {
            return;
        };

        // Exclude "Duplicate" — requires linking to another issue,
        // which can't be done from a simple status change.
        let is_duplicate =
            |s: &crate::gitlab::types::WorkItemStatus| s.name.to_lowercase().contains("duplicate");

        // Filter then sort by category priority so "done" statuses get shorter codes.
        let mut sorted_indices: Vec<usize> = (0..statuses.len())
            .filter(|&i| !is_duplicate(&statuses[i]))
            .collect();
        sorted_indices.sort_by_key(|&i| match statuses[i].category.as_deref() {
            Some("done") => 0,
            Some("active" | "opened") => 1,
            Some("canceled") => 2,
            _ => 3,
        });
        let sorted_names: Vec<String> = sorted_indices
            .iter()
            .map(|&i| statuses[i].name.clone())
            .collect();
        let sorted_codes = chord_popup::generate_priority_codes(&sorted_names);

        // Map codes back to original indices
        let mut all_codes = vec![String::new(); statuses.len()];
        for (sorted_pos, &orig_idx) in sorted_indices.iter().enumerate() {
            all_codes[orig_idx].clone_from(&sorted_codes[sorted_pos]);
        }
        let all_names: Vec<String> = statuses.iter().map(|s| s.name.clone()).collect();

        if close_only {
            let is_close_category = |s: &crate::gitlab::types::WorkItemStatus| {
                s.category
                    .as_deref()
                    .is_some_and(|c| matches!(c, "done" | "canceled" | "closed"))
            };

            // Collect close statuses with their pre-computed codes
            let mut close_items: Vec<(usize, &str)> = statuses
                .iter()
                .enumerate()
                .filter(|(_, s)| is_close_category(s))
                .map(|(i, s)| (i, s.category.as_deref().unwrap_or("")))
                .collect();

            if close_items.is_empty() {
                // No close-category statuses — fall back to simple close/reopen
                let item_state = self
                    .issue_list_state
                    .selected_issue(&self.issues)
                    .or_else(|| self.current_detail_issue())
                    .map_or("opened", |i| i.issue.state.as_str());
                let action = if item_state == "opened" {
                    ConfirmAction::CloseIssue(issue_id, iid)
                } else {
                    ConfirmAction::ReopenIssue(issue_id, iid)
                };
                self.overlay = Overlay::Confirm(action);
                return;
            }

            // Sort: "done" first so it gets priority in display
            close_items.sort_by_key(|(_, cat)| match *cat {
                "done" => 0,
                "canceled" => 1,
                _ => 2,
            });

            let options: Vec<(String, String)> = close_items
                .iter()
                .map(|&(i, _)| (all_codes[i].clone(), all_names[i].clone()))
                .collect();
            let max_code_len = options.iter().map(|(c, _)| c.len()).max().unwrap_or(1);

            self.chord_state = Some(
                chord_popup::ChordState::from_options("Close As", options, max_code_len)
                    .with_kind(chord_popup::ChordKind::Status),
            );
        } else {
            let options: Vec<(String, String)> = all_codes
                .into_iter()
                .zip(all_names)
                .filter(|(code, _)| !code.is_empty())
                .collect();
            let max_code_len = options.iter().map(|(c, _)| c.len()).max().unwrap_or(1);

            self.chord_state = Some(
                chord_popup::ChordState::from_options("Set Status", options, max_code_len)
                    .with_kind(chord_popup::ChordKind::Status),
            );
        }
        self.overlay = Overlay::Chord(ChordContext::Status(project.to_string(), issue_id, iid));
    }

    /// `s` key — open full status picker/chord for the focused issue.
    fn do_set_status(&mut self) {
        if let Some(FocusedItem::Issue {
            project, id, iid, ..
        }) = self.focused.clone()
        {
            self.fetch_statuses_and_show_chord(&project, id, iid, false);
        }
    }

    /// `x` key — close/reopen the focused item.
    /// Issues: chord picker filtered to close-category statuses (e.g. Done, Won't Do).
    /// Falls back to simple confirm if no custom statuses exist.
    /// MRs: simple close confirm.
    fn do_toggle_state(&mut self) {
        match self.focused.clone() {
            Some(FocusedItem::Issue {
                project, id, iid, ..
            }) => {
                self.fetch_statuses_and_show_chord(&project, id, iid, true);
            }
            Some(FocusedItem::Mr { project, iid, .. }) => {
                self.overlay = Overlay::Confirm(ConfirmAction::CloseMr(project, iid));
            }
            None => {}
        }
    }

    fn fetch_notes_for_mr(&self, project: &str, iid: u64) {
        let client = self.client.clone();
        let project = project.to_string();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.list_mr_discussions(&project, iid).await;
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(result));
        });
    }

    pub fn handle_async_msg(&mut self, msg: AsyncMsg) {
        match msg {
            AsyncMsg::IssuesLoaded(result, incremental) => match result {
                Ok(issues) => {
                    self.merge_issues(issues, incremental);
                    // Persist ALL issues (open + closed) to DB, then filter
                    // in-memory to open-only for display.
                    let _ = self.db.upsert_issues(&self.issues);
                    self.issues.retain(|i| i.issue.state == "opened");
                    self.refresh_shadow_work();
                    let now = Self::now_secs();
                    self.last_fetched_at = Some(now);
                    let _ = self.db.set_kv("last_fetched_at", &now);
                    self.error = None;
                    self.record_fetch_done();
                    self.dirty.issues = true;
                    self.pending_cmds.push(Cmd::FetchHealthData);
                }
                Err(e) => {
                    self.record_fetch_done();
                    self.show_error(format!("Issues: {e:#}"));
                }
            },
            AsyncMsg::MrsLoaded(result, incremental) => match result {
                Ok((tracking, external)) => {
                    let mrs: Vec<_> = tracking.into_iter().chain(external).collect();
                    self.merge_mrs(mrs, incremental);
                    // Persist ALL MRs to DB, then filter in-memory to open-only
                    let _ = self.db.upsert_mrs(&self.mrs);
                    self.mrs.retain(|m| m.mr.state == "opened");
                    let now = Self::now_secs();
                    self.last_fetched_at = Some(now);
                    let _ = self.db.set_kv("last_fetched_at", &now);
                    self.record_fetch_done();
                    self.error = None;
                    self.dirty.mrs = true;
                }
                Err(e) => {
                    self.record_fetch_done();
                    self.show_error(format!("MRs: {e:#}"));
                }
            },
            AsyncMsg::DiscussionsLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(discussions) => {
                        if self.view == View::IssueDetail {
                            self.issue_detail_state.discussions = discussions;
                            self.issue_detail_state.loading_notes = false;
                        } else if self.view == View::MrDetail {
                            self.mr_detail_state.discussions = discussions;
                            self.mr_detail_state.loading_notes = false;
                        }
                    }
                    Err(e) => {
                        self.show_error(format!("Notes: {e:#}"));
                    }
                }
            }
            AsyncMsg::ActionDone(result) => {
                self.loading = false;
                match result {
                    Ok(_msg) => {
                        self.error = None;
                        self.pending_cmds.push(Cmd::FetchAll);
                    }
                    Err(e) => {
                        self.show_error(format!("{e:#}"));
                    }
                }
            }
            AsyncMsg::IssueUpdated(result) => {
                self.loading = false;
                match result {
                    Ok(issue) => {
                        if let Some(pos) = self.issues.iter().position(|e| e.issue.id == issue.id) {
                            self.issues[pos].issue = issue;
                        }
                        self.error = None;
                        self.dirty.issues = true;
                        self.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::MrUpdated(result, project_path) => {
                self.loading = false;
                match result {
                    Ok(mr) => {
                        if let Some(pos) = self
                            .mrs
                            .iter()
                            .position(|e| e.mr.iid == mr.iid && e.project_path == project_path)
                        {
                            self.mrs[pos].mr = mr;
                        }
                        self.error = None;
                        self.dirty.mrs = true;
                        self.pending_cmds.push(Cmd::PersistMrs);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::IssueStatusUpdated(result) => {
                self.loading = false;
                match result {
                    Ok((project_path, iid, status_name)) => {
                        if let Some(pos) = self
                            .issues
                            .iter()
                            .position(|e| e.issue.iid == iid && e.project_path == project_path)
                        {
                            self.issues[pos].issue.custom_status = Some(status_name);
                        }
                        self.error = None;
                        self.dirty.issues = true;
                        self.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => self.show_error(format!("{e:#}")),
                }
            }
            AsyncMsg::LabelsLoaded(result) => {
                if let Ok(labels) = result {
                    self.labels = labels;
                    self.rebuild_label_color_map();
                    self.dirty.labels = true;
                    self.pending_cmds.push(Cmd::PersistLabels);
                }
            }
            AsyncMsg::StatusesLoaded(result, project, issue_id, iid, close_only) => {
                self.loading = false;
                let is_background = issue_id == 0 && iid == 0;
                match result {
                    Ok(statuses) => {
                        if statuses.is_empty() && !is_background {
                            // No custom statuses — fall back to open/close toggle
                            let item_state = self
                                .issue_list_state
                                .selected_issue(&self.issues)
                                .or_else(|| self.current_detail_issue())
                                .map_or("opened", |i| i.issue.state.as_str());
                            let action = if item_state == "opened" {
                                ConfirmAction::CloseIssue(issue_id, iid)
                            } else {
                                ConfirmAction::ReopenIssue(issue_id, iid)
                            };
                            self.overlay = Overlay::Confirm(action);
                        } else if !statuses.is_empty() {
                            self.work_item_statuses.insert(project.clone(), statuses);
                            self.dirty.statuses = true;
                            self.pending_cmds.push(Cmd::PersistStatuses {
                                project: project.clone(),
                            });
                            if !is_background {
                                self.show_status_chord(&project, issue_id, iid, close_only);
                            }
                        }
                    }
                    Err(e) => {
                        if !is_background {
                            self.show_error(format!("Statuses: {e:#}"));
                        }
                    }
                }
            }
            AsyncMsg::IterationsLoaded(result) => match result {
                Ok(iters) => {
                    self.iterations = iters;
                    self.classify_iterations();
                    self.dirty.iterations = true;
                    self.pending_cmds.push(Cmd::PersistIterations);
                    self.pending_cmds.push(Cmd::FetchHealthData);
                }
                Err(e) => {
                    self.show_error(format!("Iterations: {e:#}"));
                }
            },
            AsyncMsg::IterationUpdated(result, issue_id, old_iteration) => {
                self.loading = false;
                match result {
                    Ok(()) => {
                        self.error = None;
                        self.pending_cmds.push(Cmd::PersistIssues);
                    }
                    Err(e) => {
                        // Revert the optimistic update
                        if let Some(pos) = self.issues.iter().position(|i| i.issue.id == issue_id) {
                            self.issues[pos].issue.iteration = old_iteration;
                            self.dirty.issues = true;
                        }
                        self.show_error(format!("Move iteration: {e:#}"));
                    }
                }
            }
            AsyncMsg::UnplannedWorkLoaded(result) => {
                if let Ok(dates) = result {
                    self.unplanned_work_cache.extend(dates);
                }
                self.unplanned_work_state = FetchState::Done;
                // Unplanned work affects health computation, use issues dirty flag
                self.dirty.issues = true;
                self.pending_cmds.push(Cmd::PersistUnplannedWork);
            }
        }
    }

    /// Merge incoming issues into `self.issues`, preserving newer cached entries.
    fn merge_issues(&mut self, issues: Vec<TrackedIssue>, incremental: bool) {
        if incremental {
            for item in issues {
                if let Some(pos) = self.issues.iter().position(|i| i.issue.id == item.issue.id) {
                    if self.issues[pos].issue.updated_at <= item.issue.updated_at {
                        self.issues[pos] = item;
                    }
                } else {
                    self.issues.push(item);
                }
            }
        } else {
            let mut new_issues = issues;
            for new_iss in &mut new_issues {
                if let Some(pos) = self
                    .issues
                    .iter()
                    .position(|i| i.issue.id == new_iss.issue.id)
                {
                    let old_iss = &self.issues[pos];
                    if old_iss.issue.updated_at > new_iss.issue.updated_at {
                        *new_iss = old_iss.clone();
                    }
                }
            }
            self.issues = new_issues;
        }
    }

    /// Merge incoming MRs into `self.mrs`, preserving newer cached entries.
    /// Uses second precision: GraphQL truncates sub-second timestamps.
    fn merge_mrs(&mut self, mrs: Vec<TrackedMergeRequest>, incremental: bool) {
        if incremental {
            for item in mrs {
                if let Some(pos) = self.mrs.iter().position(|m| m.mr.id == item.mr.id) {
                    let old_secs = self.mrs[pos].mr.updated_at.timestamp();
                    let new_secs = item.mr.updated_at.timestamp();
                    if old_secs <= new_secs {
                        self.mrs[pos] = item;
                    }
                } else {
                    self.mrs.push(item);
                }
            }
        } else {
            let mut new_mrs = mrs;
            for new_mr in &mut new_mrs {
                if let Some(pos) = self.mrs.iter().position(|m| m.mr.id == new_mr.mr.id) {
                    let old_mr = &self.mrs[pos];
                    let old_secs = old_mr.mr.updated_at.timestamp();
                    let new_secs = new_mr.mr.updated_at.timestamp();
                    if old_secs > new_secs {
                        *new_mr = old_mr.clone();
                    }
                }
            }
            self.mrs = new_mrs;
        }
    }

    fn show_error(&mut self, msg: String) {
        self.error = Some(msg.clone());
        self.overlay = Overlay::Error(msg);
    }

    pub fn refilter_issues(&mut self) {
        let me = self.config.me.clone();
        let members = self.active_team_members();
        self.issue_list_state
            .apply_filters(&self.issues, &me, &members, &self.label_sort_orders);
    }

    pub fn refilter_planning(&mut self) {
        self.planning_state
            .partition_issues(&self.issues, &self.label_sort_orders);
    }

    pub fn refilter_iteration_board(&mut self) {
        let current_iter = self.planning_state.current_iteration.as_ref();
        let me = self.config.me.clone();
        let members = self.active_team_members();
        self.iteration_board_state.partition_issues(
            &self.issues,
            current_iter,
            &self.label_sort_orders,
            &me,
            &members,
        );
    }

    fn classify_iterations(&mut self) {
        // Iterations come sorted by CADENCE_AND_DUE_DATE_ASC.
        // States: "closed", "current", "upcoming".
        // Find current, then adjacent entries are previous/next.
        let current_pos = self.iterations.iter().position(|i| i.state == "current");

        let new_current = current_pos.map(|pos| self.iterations[pos].clone());

        // Reset health caches if the current iteration changed
        let iter_changed = match (&self.planning_state.current_iteration, &new_current) {
            (Some(old), Some(new)) => old.id != new.id,
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        };
        if iter_changed {
            self.unplanned_work_cache.clear();
            self.shadow_work_cache.clear();
            self.unplanned_work_state = FetchState::Idle;
            self.iteration_health = None;
        }

        self.planning_state.current_iteration = new_current;

        self.planning_state.prev_iteration = current_pos
            .and_then(|pos| pos.checked_sub(1))
            .map(|pos| self.iterations[pos].clone());

        self.planning_state.next_iteration = current_pos
            .and_then(|pos| self.iterations.get(pos + 1))
            .cloned();

        // Build iteration board columns from available statuses
        self.rebuild_iteration_board_columns();
    }

    fn rebuild_iteration_board_columns(&mut self) {
        // Collect all statuses from all tracked projects
        let mut all_statuses: Vec<WorkItemStatus> = Vec::new();
        for project in &self.config.tracking_projects {
            if let Some(statuses) = self.work_item_statuses.get(project) {
                for s in statuses {
                    if !all_statuses.iter().any(|existing| existing.name == s.name) {
                        all_statuses.push(s.clone());
                    }
                }
            }
        }
        if !all_statuses.is_empty() {
            self.iteration_board_state
                .build_columns(&all_statuses, &self.config.kanban_columns);
        }
    }

    fn fetch_iterations(&self) {
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.fetch_iterations().await;
            let _ = tx.send(AsyncMsg::IterationsLoaded(result));
        });
    }

    /// Fetch "added to iteration" dates for unplanned work detection.
    fn fetch_unplanned_work_data(&mut self) {
        let Some(current_iter) = self.planning_state.current_iteration.as_ref() else {
            return;
        };
        let current_id = current_iter.id.clone();

        // Collect issues in the current iteration that we haven't cached yet
        let items: Vec<(String, String, u64)> = self
            .issues
            .iter()
            .filter(|i| {
                i.issue
                    .iteration
                    .as_ref()
                    .is_some_and(|it| it.id == current_id)
                    && !self.unplanned_work_cache.contains_key(&i.issue.id)
            })
            .map(|i| {
                // Derive namespace from project_path (same as the tracking project ancestor)
                let namespace = self
                    .config
                    .tracking_projects
                    .first()
                    .cloned()
                    .unwrap_or_else(|| i.project_path.clone());
                (namespace, i.issue.iid.to_string(), i.issue.id)
            })
            .collect();

        if items.is_empty() {
            self.unplanned_work_state = FetchState::Done;
            self.compute_iteration_health();
            return;
        }

        self.unplanned_work_state = FetchState::InFlight;

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        tokio::spawn(async move {
            let result = client.fetch_iteration_added_dates_batch(items).await;
            let _ = tx.send(AsyncMsg::UnplannedWorkLoaded(result));
        });
    }

    /// Refresh shadow work cache from DB (closed issues in current iteration range).
    fn refresh_shadow_work(&mut self) {
        let Some(iter) = self.planning_state.current_iteration.as_ref() else {
            self.shadow_work_cache.clear();
            return;
        };
        let (Some(start), Some(end)) = (iter.start_date.as_ref(), iter.due_date.as_ref()) else {
            self.shadow_work_cache.clear();
            return;
        };
        let closed_after = format!("{start}T00:00:00+00:00");
        let closed_before = format!("{end}T23:59:59+00:00");
        if let Ok(shadow) = self
            .db
            .query_shadow_work(&closed_after, &closed_before, Some(&iter.id))
        {
            self.shadow_work_cache = shadow;
        }
    }

    /// Trigger unplanned work fetch if conditions are met.
    fn maybe_fetch_health_data(&mut self) {
        if self.planning_state.current_iteration.is_none() {
            return;
        }
        if self.unplanned_work_state != FetchState::InFlight {
            self.fetch_unplanned_work_data();
        }
    }

    /// Recompute iteration health metrics from current data.
    fn compute_iteration_health(&mut self) {
        let Some(current_iter) = self.planning_state.current_iteration.as_ref() else {
            self.iteration_health = None;
            return;
        };

        self.iteration_health = Some(dashboard::compute_health(
            &self.issues,
            current_iter,
            &self.unplanned_work_cache,
            self.unplanned_work_state != FetchState::Done,
            &self.shadow_work_cache,
            self.iteration_health.as_ref(),
        ));
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

    fn refilter_mrs(&mut self) {
        let me = self.config.me.clone();
        let members = self.active_team_members();
        self.mr_list_state
            .apply_filters(&self.mrs, &me, &members, &self.label_sort_orders);
    }

    /// Returns a mutable reference to the `UserFilter` for the current view.
    fn active_filter_mut(&mut self) -> &mut UserFilter {
        match self.view {
            View::IssueList | View::IssueDetail => &mut self.issue_list_state.filter,
            View::MrList | View::MrDetail => &mut self.mr_list_state.filter,
            View::Planning => {
                let col = self.planning_state.focused_column;
                &mut self.planning_state.columns[col].filter
            }
            View::Dashboard => &mut self.iteration_board_state.filter,
        }
    }

    fn active_filter(&self) -> &UserFilter {
        match self.view {
            View::IssueList | View::IssueDetail => &self.issue_list_state.filter,
            View::MrList | View::MrDetail => &self.mr_list_state.filter,
            View::Planning => {
                let col = self.planning_state.focused_column;
                &self.planning_state.columns[col].filter
            }
            View::Dashboard => &self.iteration_board_state.filter,
        }
    }

    fn action_sort_by_field(&mut self) {
        let kind = match self.view {
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
        for p in &self.config.sort_presets {
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
        for order in &self.config.label_sort_orders {
            labels.push(format!("{}::", order.scope));
        }

        self.chord_state = Some(chord_popup::ChordState::new_for_names("Sort by", labels));
        self.overlay = Overlay::Chord(ChordContext::SortField);
    }

    fn handle_sort_field_chosen(&mut self, value: &str) {
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
        self.chord_state = Some(chord_popup::ChordState::new_for_names(
            &format!("Sort {value}"),
            labels,
        ));
        self.overlay = Overlay::Chord(ChordContext::SortDirection(field_name, label_scope));
    }

    fn handle_sort_direction_chosen(
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
        self.dirty.view_state = true;
        self.pending_cmds.push(Cmd::PersistViewState);
    }

    fn apply_sort_preset(&mut self, name: &str) {
        let specs = self
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

    // ── InputMode ──────────────────────────────────────────────────────

    /// Compute the current input mode from overlay and view state.
    fn input_mode(&self) -> InputMode {
        // Overlay takes highest priority
        match &self.overlay {
            Overlay::CommentInput | Overlay::Picker(_) | Overlay::LabelEditor => {
                return InputMode::TextInput;
            }
            Overlay::Chord(_) => return InputMode::Chord,
            Overlay::Help | Overlay::Confirm(_) | Overlay::Error(_) => {
                return InputMode::Modal;
            }
            Overlay::FilterEditor => {
                return if self.filter_editor_state.step == filter_editor::EditorStep::EnterValue {
                    InputMode::TextInput
                } else {
                    InputMode::Normal
                };
            }
            Overlay::None => {}
        }

        // Inline fuzzy search
        if self.active_filter().is_searching() {
            return InputMode::TextInput;
        }

        InputMode::Normal
    }

    // ── Key dispatch ─────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Assume any key press changes visible state.  Navigation methods
        // that hit a boundary (e.g. up at top of list) clear this flag so
        // the event loop can skip the redundant render.
        self.needs_redraw = true;
        match self.input_mode() {
            InputMode::TextInput => self.handle_text_input(key),
            InputMode::Chord => self.handle_chord_input(key),
            InputMode::Modal => self.handle_modal_input(key),
            InputMode::Normal => self.handle_normal_input(key),
        }
    }

    /// All chars go to the active text widget.
    fn handle_text_input(&mut self, key: KeyEvent) -> bool {
        match &self.overlay {
            Overlay::CommentInput
            | Overlay::Picker(_)
            | Overlay::FilterEditor
            | Overlay::LabelEditor => self.handle_overlay_key(key),
            Overlay::None => {
                // Inline fuzzy search
                self.handle_fuzzy_text(key);
                false
            }
            _ => false,
        }
    }

    /// Home-row keys select a chord option; Esc or anything else cancels.
    fn handle_chord_input(&mut self, key: KeyEvent) -> bool {
        self.handle_overlay_key(key)
    }

    /// Modal overlay dispatch (Help, Confirm, Error).
    fn handle_modal_input(&mut self, key: KeyEvent) -> bool {
        self.handle_overlay_key(key)
    }

    /// Normal mode: filter bar check, then binding registry dispatch.
    fn handle_normal_input(&mut self, key: KeyEvent) -> bool {
        // Filter bar captures all keys when focused
        if self.active_filter().bar_focused {
            self.handle_filter_bar_key(key);
            return false;
        }

        // FilterEditor in field/op selection step (Normal mode)
        if matches!(self.overlay, Overlay::FilterEditor) {
            return self.handle_overlay_key(key);
        }

        // Binding registry dispatch
        if let Some(action) = keybindings::match_binding(self.view, &key) {
            return self.execute_action(action);
        }
        false
    }

    /// Centralized fuzzy search handler.
    fn handle_fuzzy_text(&mut self, key: KeyEvent) {
        let is_issue_or_mr = matches!(self.view, View::IssueList | View::MrList);
        let is_exit = matches!(key.code, KeyCode::Enter | KeyCode::Esc);

        let needs_refilter = self.active_filter_mut().handle_fuzzy_input(&key);
        if needs_refilter == Some(true) {
            self.dirty.view_state = true;
        }
        if is_issue_or_mr && is_exit {
            self.pending_cmds.push(Cmd::PersistViewState);
        }
        self.dirty.selection = true;
    }

    // ── Action execution ─────────────────────────────────────────────

    #[allow(clippy::too_many_lines)]
    fn execute_action(&mut self, action: KeyAction) -> bool {
        match action {
            // === Global ===
            KeyAction::Back => {
                if let Some(prev) = self.view_stack.pop() {
                    self.view = prev;
                    self.dirty.selection = true;
                } else {
                    self.overlay = Overlay::Confirm(ConfirmAction::QuitApp);
                }
            }
            KeyAction::ToggleHelp => {
                self.overlay = Overlay::Help;
            }
            KeyAction::ShowLastError => {
                if let Some(err) = &self.error {
                    self.overlay = Overlay::Error(err.clone());
                }
            }
            KeyAction::SwitchTeam => {
                if !self.config.teams.is_empty() {
                    let mut names: Vec<String> = vec!["All".to_string()];
                    names.extend(self.config.teams.iter().map(|t| t.name.clone()));
                    self.picker_state = Some(picker::PickerState::new("Switch Team", names, false));
                    self.overlay = Overlay::Picker(PickerContext::Team);
                }
            }
            KeyAction::NavigateTo(target) => {
                if self.view != target {
                    self.navigate_to_view(target);
                }
            }

            // === List / detail navigation ===
            KeyAction::MoveDown => self.nav_down(),
            KeyAction::MoveUp => self.nav_up(),
            KeyAction::Top => self.nav_top(),
            KeyAction::Bottom => self.nav_bottom(),
            KeyAction::PageDown => self.nav_page_down(),
            KeyAction::PageUp => self.nav_page_up(),
            KeyAction::OpenDetail => self.action_open_detail(),

            // === Search & Filter ===
            KeyAction::StartSearch => self.action_start_search(),
            KeyAction::FocusFilterBar => self.action_focus_filter_bar(),
            KeyAction::FilterMenu => self.action_show_filter_menu(),
            KeyAction::ClearFilters => self.action_clear_filters(),
            KeyAction::SortByField => self.action_sort_by_field(),

            // === Item actions (resolved via view/FocusedItem) ===
            KeyAction::Refresh => {
                self.loading = true;
                self.pending_cmds.push(Cmd::FetchAll);
            }
            KeyAction::FullRefresh => {
                self.loading = true;
                self.pending_cmds.push(Cmd::FetchAllFull);
            }
            KeyAction::OpenBrowser => self.action_open_browser(),
            KeyAction::SetStatus => self.do_set_status(),
            KeyAction::ToggleState => self.do_toggle_state(),
            KeyAction::EditLabels => self.action_edit_labels(),
            KeyAction::EditAssignee => self.action_edit_assignee(),
            KeyAction::Comment => self.action_open_comment(),

            // === MR-specific ===
            KeyAction::Approve => {
                if let Some(FocusedItem::Mr {
                    ref project, iid, ..
                }) = self.focused
                {
                    self.overlay = Overlay::Confirm(ConfirmAction::ApproveMr(project.clone(), iid));
                }
            }
            KeyAction::Merge => {
                if let Some(FocusedItem::Mr {
                    ref project, iid, ..
                }) = self.focused
                {
                    self.overlay = Overlay::Confirm(ConfirmAction::MergeMr(project.clone(), iid));
                }
            }

            // === Detail-specific ===
            KeyAction::ReplyThread => self.action_reply_thread(),

            // === Board / column navigation ===
            KeyAction::ToggleDashboardFocus => {
                if self.view == View::Dashboard {
                    self.iteration_board_state.health_focused =
                        !self.iteration_board_state.health_focused;
                    self.dirty.selection = true;
                }
            }
            KeyAction::ColumnLeft => self.action_column_left(),
            KeyAction::ColumnRight => self.action_column_right(),

            // === Planning-specific ===
            KeyAction::ToggleColumnPrev => {
                self.planning_state.column_visible[0] = !self.planning_state.column_visible[0];
                self.planning_state.clamp_focus();
            }
            KeyAction::ToggleColumnNext => {
                self.planning_state.column_visible[2] = !self.planning_state.column_visible[2];
                self.planning_state.clamp_focus();
            }
            KeyAction::ToggleLayout => {
                self.planning_state.toggle_layout();
                self.dirty.issues = true;
                self.dirty.selection = true;
            }
            KeyAction::MoveIteration => {
                self.show_iteration_chord();
            }
        }
        false
    }

    // ── Action helpers ───────────────────────────────────────────────

    fn navigate_to_view(&mut self, target: View) {
        self.view_stack.clear();
        if target != View::Dashboard {
            self.view_stack.push(View::Dashboard);
        }
        self.view = target;
        // Ensure the target view has up-to-date indices
        self.dirty.view_state = true;
        self.dirty.selection = true;
    }

    fn nav_down(&mut self) {
        match self.view {
            View::IssueDetail => self.issue_detail_state.scroll_down(),
            View::MrDetail => self.mr_detail_state.scroll_down(),
            View::IssueList => {
                if !self.issue_list_state.list.select_next() {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::MrList => {
                if !self.mr_list_state.list.select_next() {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::Planning => {
                let col = self.planning_state.focused_column;
                if !self.planning_state.columns[col].list.select_next() {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::Dashboard if self.iteration_board_state.health_focused => {
                if let Some(ref mut health) = self.iteration_health
                    && !health.active_list_mut().select_next()
                {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                let moved = self
                    .iteration_board_state
                    .columns
                    .get_mut(col)
                    .is_some_and(|c| c.list.select_next());
                if !moved {
                    self.needs_redraw = false;
                    return;
                }
            }
        }
        self.dirty.selection = true;
    }

    fn nav_up(&mut self) {
        match self.view {
            View::IssueDetail => self.issue_detail_state.scroll_up(),
            View::MrDetail => self.mr_detail_state.scroll_up(),
            View::IssueList => {
                if !self.issue_list_state.list.select_prev() {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::MrList => {
                if !self.mr_list_state.list.select_prev() {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::Planning => {
                let col = self.planning_state.focused_column;
                if !self.planning_state.columns[col].list.select_prev() {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::Dashboard if self.iteration_board_state.health_focused => {
                if let Some(ref mut health) = self.iteration_health
                    && !health.active_list_mut().select_prev()
                {
                    self.needs_redraw = false;
                    return;
                }
            }
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                let moved = self
                    .iteration_board_state
                    .columns
                    .get_mut(col)
                    .is_some_and(|c| c.list.select_prev());
                if !moved {
                    self.needs_redraw = false;
                    return;
                }
            }
        }
        self.dirty.selection = true;
    }

    fn nav_top(&mut self) {
        let moved = match self.view {
            View::IssueList => self.issue_list_state.list.select_first(),
            View::MrList => self.mr_list_state.list.select_first(),
            View::Planning => {
                let col = self.planning_state.focused_column;
                self.planning_state.columns[col].list.select_first()
            }
            View::Dashboard if self.iteration_board_state.health_focused => self
                .iteration_health
                .as_mut()
                .is_some_and(|h| h.active_list_mut().select_first()),
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                self.iteration_board_state
                    .columns
                    .get_mut(col)
                    .is_some_and(|c| c.list.select_first())
            }
            _ => return,
        };
        if moved {
            self.dirty.selection = true;
        } else {
            self.needs_redraw = false;
        }
    }

    fn nav_bottom(&mut self) {
        let moved = match self.view {
            View::IssueList => self.issue_list_state.list.select_last(),
            View::MrList => self.mr_list_state.list.select_last(),
            View::Planning => {
                let col = self.planning_state.focused_column;
                self.planning_state.columns[col].list.select_last()
            }
            View::Dashboard if self.iteration_board_state.health_focused => self
                .iteration_health
                .as_mut()
                .is_some_and(|h| h.active_list_mut().select_last()),
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                self.iteration_board_state
                    .columns
                    .get_mut(col)
                    .is_some_and(|c| c.list.select_last())
            }
            _ => return,
        };
        if moved {
            self.dirty.selection = true;
        } else {
            self.needs_redraw = false;
        }
    }

    fn nav_page_down(&mut self) {
        let moved = match self.view {
            View::IssueList => self.issue_list_state.list.page_down(),
            View::MrList => self.mr_list_state.list.page_down(),
            View::Planning => {
                let col = self.planning_state.focused_column;
                self.planning_state.columns[col].list.page_down()
            }
            View::Dashboard if self.iteration_board_state.health_focused => self
                .iteration_health
                .as_mut()
                .is_some_and(|h| h.active_list_mut().page_down()),
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                self.iteration_board_state
                    .columns
                    .get_mut(col)
                    .is_some_and(|c| c.list.page_down())
            }
            _ => return,
        };
        if moved {
            self.dirty.selection = true;
        } else {
            self.needs_redraw = false;
        }
    }

    fn nav_page_up(&mut self) {
        let moved = match self.view {
            View::IssueList => self.issue_list_state.list.page_up(),
            View::MrList => self.mr_list_state.list.page_up(),
            View::Planning => {
                let col = self.planning_state.focused_column;
                self.planning_state.columns[col].list.page_up()
            }
            View::Dashboard if self.iteration_board_state.health_focused => self
                .iteration_health
                .as_mut()
                .is_some_and(|h| h.active_list_mut().page_up()),
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                self.iteration_board_state
                    .columns
                    .get_mut(col)
                    .is_some_and(|c| c.list.page_up())
            }
            _ => return,
        };
        if moved {
            self.dirty.selection = true;
        } else {
            self.needs_redraw = false;
        }
    }

    /// Ensure `issue_list_state` points at the issue identified by (project, iid)
    /// so the detail view can display it via `current_detail_issue()`.
    /// If the issue isn't in `self.issues` (e.g. shadow work from a separate cache),
    /// it is appended so the detail view can render it.
    fn sync_issue_list_for_detail(&mut self, project: &str, iid: u64) {
        let pos = self
            .issues
            .iter()
            .position(|i| i.issue.iid == iid && i.project_path == project)
            .or_else(|| {
                // Shadow work issues live in a separate cache — copy into issues
                let sw = self
                    .shadow_work_cache
                    .iter()
                    .find(|i| i.issue.iid == iid && i.project_path == project)?
                    .clone();
                self.issues.push(sw);
                Some(self.issues.len() - 1)
            });

        if let Some(pos) = pos {
            if let Some(list_pos) = self
                .issue_list_state
                .list
                .indices
                .iter()
                .position(|&i| i == pos)
            {
                self.issue_list_state
                    .list
                    .table_state
                    .select(Some(list_pos));
            } else {
                self.issue_list_state.list.indices.push(pos);
                self.issue_list_state
                    .list
                    .table_state
                    .select(Some(self.issue_list_state.list.indices.len() - 1));
            }
        }
    }

    fn action_open_detail(&mut self) {
        match self.view {
            View::IssueList => {
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
            View::MrList => {
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
            View::Dashboard if self.iteration_board_state.health_focused => {
                if let Some(FocusedItem::Issue { project, iid, .. }) = self.focused.clone() {
                    self.sync_issue_list_for_detail(&project, iid);
                    self.issue_detail_state.reset();
                    self.issue_detail_state.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::Dashboard);
                    self.view = View::IssueDetail;
                }
            }
            View::Dashboard => {
                if let Some(item) = self
                    .iteration_board_state
                    .selected_issue(&self.issues)
                    .cloned()
                {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    // Sync issue_list_state for detail view
                    let col = self.iteration_board_state.focused_column;
                    if let Some(idx) = self
                        .iteration_board_state
                        .columns
                        .get(col)
                        .and_then(|c| c.list.selected_index())
                    {
                        if let Some(pos) = self
                            .issue_list_state
                            .list
                            .indices
                            .iter()
                            .position(|&i| i == idx)
                        {
                            self.issue_list_state.list.table_state.select(Some(pos));
                        } else {
                            self.issue_list_state.list.indices.push(idx);
                            self.issue_list_state
                                .list
                                .table_state
                                .select(Some(self.issue_list_state.list.indices.len() - 1));
                        }
                    }
                    self.issue_detail_state.reset();
                    self.issue_detail_state.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::Dashboard);
                    self.view = View::IssueDetail;
                }
            }
            View::Planning => {
                if let Some(item) = self.planning_state.selected_issue(&self.issues).cloned() {
                    let project = item.project_path.clone();
                    let iid = item.issue.iid;
                    let col = self.planning_state.focused_column;
                    if let Some(sel) = self.planning_state.columns[col].list.table_state.selected()
                        && let Some(&idx) = self.planning_state.columns[col].list.indices.get(sel)
                    {
                        if let Some(pos) = self
                            .issue_list_state
                            .list
                            .indices
                            .iter()
                            .position(|&i| i == idx)
                        {
                            self.issue_list_state.list.table_state.select(Some(pos));
                        } else {
                            self.issue_list_state.list.indices.push(idx);
                            self.issue_list_state
                                .list
                                .table_state
                                .select(Some(self.issue_list_state.list.indices.len() - 1));
                        }
                    }
                    self.issue_detail_state.reset();
                    self.issue_detail_state.loading_notes = true;
                    self.fetch_notes_for_issue(&project, iid);
                    self.view_stack.push(View::Planning);
                    self.view = View::IssueDetail;
                }
            }
            _ => {}
        }
        self.dirty.selection = true;
    }

    fn action_start_search(&mut self) {
        self.active_filter_mut().start_search();
    }

    fn action_focus_filter_bar(&mut self) {
        let f = self.active_filter_mut();
        if !f.conditions.is_empty() {
            f.bar_focused = true;
            f.bar_selected = 0;
        }
    }

    fn action_clear_filters(&mut self) {
        self.active_filter_mut().conditions.clear();
        self.dirty.view_state = true;
        self.pending_cmds.push(Cmd::PersistViewState);
    }

    fn action_show_filter_menu(&mut self) {
        let kind = match self.view {
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
        let has_presets = self.config.filters.iter().any(|f| f.kind == kind);
        if has_presets {
            labels.push(chord_popup::DIVIDER.to_string());
            labels.push(format!("{}Presets", chord_popup::HEADER));
            for f in &self.config.filters {
                if f.kind == kind {
                    labels.push(format!("▸ {}", f.name));
                }
            }
        }

        self.chord_state = Some(chord_popup::ChordState::new_for_names("Filter", labels));
        self.overlay = Overlay::Chord(ChordContext::FilterMenu);
    }

    fn handle_filter_menu_chosen(&mut self, value: &str) {
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
            self.dirty.view_state = true;
            self.pending_cmds.push(Cmd::PersistViewState);
            // Reopen the filter menu
            self.action_show_filter_menu();
        }
    }

    fn show_filter_field_chord(&mut self) {
        let labels: Vec<String> = Field::all().iter().map(|f| f.name().to_string()).collect();
        self.chord_state = Some(chord_popup::ChordState::new_for_names(
            "Filter Field",
            labels,
        ));
        self.overlay = Overlay::Chord(ChordContext::FilterField);
    }

    fn handle_filter_field_chosen(&mut self, value: &str) {
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
        self.chord_state = Some(chord_popup::ChordState::new_for_names(
            &format!("{value}:"),
            labels,
        ));
        self.overlay = Overlay::Chord(ChordContext::FilterOp(field));
    }

    fn handle_filter_op_chosen(&mut self, field: Field, value: &str) {
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
        self.filter_editor_state.reset();
        self.filter_editor_state.selected_field = Some(field);
        self.filter_editor_state.selected_op = Some(op);
        self.filter_editor_state.step = filter_editor::EditorStep::EnterValue;
        self.filter_editor_state.suggestions = self.get_filter_suggestions();
        self.overlay = Overlay::FilterEditor;
    }

    fn action_open_browser(&mut self) {
        match self.view {
            View::IssueList | View::IssueDetail => {
                if let Some(item) = self
                    .current_detail_issue()
                    .or_else(|| self.issue_list_state.selected_issue(&self.issues))
                {
                    let _ = open::that_detached(&item.issue.web_url);
                }
            }
            View::MrList | View::MrDetail => {
                if let Some(item) = self
                    .current_detail_mr()
                    .or_else(|| self.mr_list_state.selected_mr(&self.mrs))
                {
                    let _ = open::that_detached(&item.mr.web_url);
                }
            }
            View::Dashboard if self.iteration_board_state.health_focused => {
                if let Some(item) = self
                    .iteration_health
                    .as_ref()
                    .and_then(|h| h.selected_issue(&self.issues, &self.shadow_work_cache))
                {
                    let _ = open::that_detached(&item.issue.web_url);
                }
            }
            View::Dashboard => {
                if let Some(item) = self.iteration_board_state.selected_issue(&self.issues) {
                    let _ = open::that_detached(&item.issue.web_url);
                }
            }
            View::Planning => {
                if let Some(item) = self.planning_state.selected_issue(&self.issues) {
                    let _ = open::that(&item.issue.web_url);
                }
            }
        }
    }

    fn action_edit_labels(&mut self) {
        let label_names: Vec<String> = self.labels.iter().map(|l| l.name.clone()).collect();
        let current = match self.view {
            View::IssueList => self
                .issue_list_state
                .selected_issue(&self.issues)
                .map(|i| i.issue.labels.clone()),
            View::IssueDetail => self.current_detail_issue().map(|i| i.issue.labels.clone()),
            View::MrList => self
                .mr_list_state
                .selected_mr(&self.mrs)
                .map(|m| m.mr.labels.clone()),
            View::MrDetail => self.current_detail_mr().map(|m| m.mr.labels.clone()),
            View::Dashboard if self.iteration_board_state.health_focused => self
                .iteration_health
                .as_ref()
                .and_then(|h| h.selected_issue(&self.issues, &self.shadow_work_cache))
                .map(|i| i.issue.labels.clone()),
            View::Dashboard => self
                .iteration_board_state
                .selected_issue(&self.issues)
                .map(|i| i.issue.labels.clone()),
            View::Planning => self
                .planning_state
                .selected_issue(&self.issues)
                .map(|i| i.issue.labels.clone()),
        };
        if let Some(current) = current {
            let issue_labels: Vec<Vec<String>> =
                self.issues.iter().map(|i| i.issue.labels.clone()).collect();
            self.label_editor_state = Some(label_editor::LabelEditorState::new(
                label_names,
                &current,
                &self.label_usage,
                &issue_labels,
                20,
            ));
            self.overlay = Overlay::LabelEditor;
        }
    }

    fn action_edit_assignee(&mut self) {
        match self.view {
            View::IssueDetail | View::MrDetail => {
                let members = self.picker_members();
                self.picker_state = Some(picker::PickerState::new("Assignee", members, false));
                self.overlay = Overlay::Picker(PickerContext::Assignee);
            }
            _ => {
                let members = self.picker_members();
                self.chord_state = Some(chord_popup::ChordState::new_for_names(
                    "Set Assignee",
                    members,
                ));
                self.overlay = Overlay::Chord(ChordContext::Assignee);
            }
        }
    }

    fn action_open_comment(&mut self) {
        if self.focused.is_some() {
            self.comment_input = crate::ui::components::input::CommentInput::default();
            self.reply_discussion_id = None;
            self.overlay = Overlay::CommentInput;
        }
    }

    fn action_reply_thread(&mut self) {
        let infos = match self.view {
            View::IssueDetail => self.issue_detail_state.thread_picker_items(),
            View::MrDetail => self.mr_detail_state.thread_picker_items(),
            _ => return,
        };
        if !infos.is_empty() {
            let (labels, subtitles) = build_thread_picker_display(&infos);
            self.picker_state = Some(
                picker::PickerState::new("Reply to thread", labels, false)
                    .with_subtitles(subtitles),
            );
            self.overlay = Overlay::Picker(PickerContext::ReplyThread(infos));
        }
    }

    fn action_column_left(&mut self) {
        match self.view {
            View::Dashboard if self.iteration_board_state.health_focused => {
                if let Some(ref mut health) = self.iteration_health {
                    health.active_tab = health.active_tab.prev();
                    health.active_list_mut().table_state.select(Some(0));
                }
            }
            View::Dashboard if !self.iteration_board_state.columns.is_empty() => {
                self.iteration_board_state.focused_column =
                    self.iteration_board_state.focused_column.saturating_sub(1);
            }
            View::Planning => {
                self.planning_state.move_focus_left();
            }
            _ => {}
        }
        self.dirty.selection = true;
    }

    fn action_column_right(&mut self) {
        match self.view {
            View::Dashboard if self.iteration_board_state.health_focused => {
                if let Some(ref mut health) = self.iteration_health {
                    health.active_tab = health.active_tab.next();
                    health.active_list_mut().table_state.select(Some(0));
                }
            }
            View::Dashboard
                if !self.iteration_board_state.columns.is_empty()
                    && self.iteration_board_state.focused_column + 1
                        < self.iteration_board_state.columns.len() =>
            {
                self.iteration_board_state.focused_column += 1;
            }
            View::Planning => {
                self.planning_state.move_focus_right();
            }
            _ => {}
        }
        self.dirty.selection = true;
    }

    fn handle_chord_result(&mut self, value: &str) {
        let context = std::mem::replace(&mut self.overlay, Overlay::None);
        match context {
            Overlay::Chord(ChordContext::Status(project, issue_id, iid)) => {
                self.set_issue_status(&project, issue_id, iid, value);
            }
            Overlay::Chord(ChordContext::Assignee) => {
                self.update_assignee(value);
            }
            Overlay::Chord(ChordContext::Iteration(issue_idx)) => {
                self.apply_iteration_move(issue_idx, value);
            }
            Overlay::Chord(ChordContext::SortField) => {
                self.handle_sort_field_chosen(value);
            }
            Overlay::Chord(ChordContext::SortDirection(field, scope)) => {
                self.handle_sort_direction_chosen(&field, scope.as_deref(), value);
            }
            Overlay::Chord(ChordContext::FilterMenu) => {
                self.handle_filter_menu_chosen(value);
            }
            Overlay::Chord(ChordContext::FilterField) => {
                self.handle_filter_field_chosen(value);
            }
            Overlay::Chord(ChordContext::FilterOp(field)) => {
                self.handle_filter_op_chosen(field, value);
            }
            _ => {}
        }
    }

    fn show_iteration_chord(&mut self) {
        let Some(FocusedItem::Issue { id, .. }) = &self.focused else {
            return;
        };
        let Some(issue_idx) = self.issues.iter().position(|i| i.issue.id == *id) else {
            return;
        };

        // Build choices: prev / current / next / remove
        let mut labels = Vec::new();
        if let Some(iter) = &self.planning_state.prev_iteration {
            labels.push(format!("◁ {}", planning::iteration_label(iter)));
        }
        if let Some(iter) = &self.planning_state.current_iteration {
            labels.push(format!("● {}", planning::iteration_label(iter)));
        }
        if let Some(iter) = &self.planning_state.next_iteration {
            labels.push(format!("▷ {}", planning::iteration_label(iter)));
        }
        labels.push("⊘ Remove iteration".to_string());

        self.chord_state = Some(chord_popup::ChordState::new("Move to iteration", labels));
        self.overlay = Overlay::Chord(ChordContext::Iteration(issue_idx));
    }

    fn apply_iteration_move(&mut self, issue_idx: usize, choice: &str) {
        let target = if choice.starts_with('◁') {
            self.planning_state.prev_iteration.clone()
        } else if choice.starts_with('●') {
            self.planning_state.current_iteration.clone()
        } else if choice.starts_with('▷') {
            self.planning_state.next_iteration.clone()
        } else {
            // Remove iteration
            None
        };

        let issue_id = self.issues[issue_idx].issue.id;
        let old_iteration = self.issues[issue_idx].issue.iteration.clone();

        // Optimistic update
        self.issues[issue_idx].issue.iteration.clone_from(&target);
        self.issues[issue_idx].issue.updated_at = chrono::Utc::now();
        self.dirty.issues = true;

        let target_gid = target.as_ref().map(|i| i.id.clone());
        self.pending_cmds.push(Cmd::SpawnMoveIteration {
            issue_id,
            target_gid,
            old_iteration,
        });
        self.pending_cmds.push(Cmd::FetchHealthData);
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
                    self.filter_editor_state.suggestions = self.get_filter_suggestions();
                }
                match action {
                    filter_editor::FilterEditorAction::Continue => {}
                    filter_editor::FilterEditorAction::Cancel => {
                        self.overlay = Overlay::None;
                        self.action_show_filter_menu();
                    }
                    filter_editor::FilterEditorAction::AddCondition(cond) => {
                        self.active_filter_mut().conditions.push(cond);
                        self.dirty.view_state = true;
                        self.pending_cmds.push(Cmd::PersistViewState);
                        // Reopen filter menu for adding more conditions
                        self.overlay = Overlay::None;
                        self.action_show_filter_menu();
                    }
                }
            }
            Overlay::Confirm(action) => {
                let action = action.clone();
                match key.code {
                    KeyCode::Char('y' | 'Y') => {
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
                            self.handle_picker_result(&values);
                            // ReplyThread transitions to CommentInput; don't overwrite
                            if !matches!(self.overlay, Overlay::CommentInput) {
                                self.overlay = Overlay::None;
                            }
                            self.picker_state = None;
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
                            self.handle_chord_result(&value);
                        }
                    }
                }
            }
            Overlay::LabelEditor => {
                if let Some(ref mut les) = self.label_editor_state {
                    match les.handle_key(&key) {
                        label_editor::LabelEditorAction::Continue => {}
                        label_editor::LabelEditorAction::Cancel => {
                            self.label_editor_state = None;
                            self.overlay = Overlay::None;
                        }
                        label_editor::LabelEditorAction::Confirmed(labels) => {
                            self.handle_label_editor_result(&labels);
                            self.label_editor_state = None;
                            self.overlay = Overlay::None;
                        }
                    }
                }
            }
            Overlay::CommentInput => {
                // Autocomplete takes priority when active
                if self.autocomplete.active {
                    if key.code == KeyCode::Tab {
                        self.accept_completion();
                        return false;
                    }
                    if key.code == KeyCode::Esc {
                        self.autocomplete.dismiss();
                        return false;
                    }
                    if keys::is_nav_up(&key) {
                        self.autocomplete.move_up();
                        return false;
                    }
                    if keys::is_nav_down(&key) {
                        self.autocomplete.move_down();
                        return false;
                    }
                }
                match self.comment_input.handle_key(&key) {
                    input::InputAction::Cancel => {
                        self.autocomplete.dismiss();
                        self.overlay = Overlay::None;
                    }
                    input::InputAction::Submit => {
                        let body = self.comment_input.text();
                        let body = body.trim().to_string();
                        if !body.is_empty() {
                            self.submit_comment(&body);
                        }
                        self.autocomplete.dismiss();
                        self.overlay = Overlay::None;
                    }
                    input::InputAction::Continue => {
                        let text = self.comment_input.text();
                        let cursor = self.comment_input.cursor_byte_pos();
                        let members = self.config.all_members();
                        self.autocomplete
                            .update(&text, cursor, &members, &self.issues, &self.mrs);
                    }
                }
            }
            Overlay::Error(_) => {
                // Any key dismisses the error popup
                self.overlay = Overlay::None;
            }
            Overlay::None => {}
        }
        false
    }

    fn handle_filter_bar_key(&mut self, key: KeyEvent) {
        if keys::is_back(&key) || keys::is_tab(&key) {
            self.active_filter_mut().bar_focused = false;
            return;
        }
        if keys::is_left(&key) {
            let f = self.active_filter_mut();
            f.bar_selected = f.bar_selected.saturating_sub(1);
        } else if keys::is_right(&key) {
            let f = self.active_filter_mut();
            if f.bar_selected + 1 < f.conditions.len() {
                f.bar_selected += 1;
            }
        } else if key.code == KeyCode::Char('x') || key.code == KeyCode::Char('d') {
            let f = self.active_filter_mut();
            if f.bar_selected < f.conditions.len() {
                f.conditions.remove(f.bar_selected);
                if f.bar_selected > 0 && f.bar_selected >= f.conditions.len() {
                    f.bar_selected = f.conditions.len().saturating_sub(1);
                }
                if f.conditions.is_empty() {
                    f.bar_focused = false;
                }
                self.dirty.view_state = true;
                self.pending_cmds.push(Cmd::PersistViewState);
            }
        }
    }

    fn execute_confirm(&mut self, action: ConfirmAction) {
        // Optimistic updates — set dirty flags, reconcile will refilter
        match &action {
            ConfirmAction::CloseIssue(issue_id, _) => {
                if let Some(pos) = self.issues.iter().position(|i| i.issue.id == *issue_id) {
                    self.issues[pos].issue.state = "closed".to_string();
                    self.issues[pos].issue.updated_at = chrono::Utc::now();
                    self.dirty.issues = true;
                    self.pending_cmds.push(Cmd::PersistIssues);
                }
            }
            ConfirmAction::ReopenIssue(issue_id, _) => {
                if let Some(pos) = self.issues.iter().position(|i| i.issue.id == *issue_id) {
                    self.issues[pos].issue.state = "opened".to_string();
                    self.issues[pos].issue.updated_at = chrono::Utc::now();
                    self.dirty.issues = true;
                    self.pending_cmds.push(Cmd::PersistIssues);
                }
            }
            ConfirmAction::CloseMr(project, iid) => {
                if let Some(pos) = self
                    .mrs
                    .iter()
                    .position(|m| m.project_path == *project && m.mr.iid == *iid)
                {
                    self.mrs[pos].mr.state = "closed".to_string();
                    self.mrs[pos].mr.updated_at = chrono::Utc::now();
                    self.dirty.mrs = true;
                    self.pending_cmds.push(Cmd::PersistMrs);
                }
            }
            ConfirmAction::MergeMr(project, iid) => {
                if let Some(pos) = self
                    .mrs
                    .iter()
                    .position(|m| m.project_path == *project && m.mr.iid == *iid)
                {
                    self.mrs[pos].mr.state = "merged".to_string();
                    self.mrs[pos].mr.updated_at = chrono::Utc::now();
                    self.dirty.mrs = true;
                    self.pending_cmds.push(Cmd::PersistMrs);
                }
            }
            _ => {}
        }

        // Spawn API call via Cmd
        let spawn_cmd = match action {
            ConfirmAction::CloseIssue(issue_id, _) => Cmd::SpawnCloseIssue { issue_id },
            ConfirmAction::ReopenIssue(issue_id, _) => Cmd::SpawnReopenIssue { issue_id },
            ConfirmAction::CloseMr(project, iid) => Cmd::SpawnCloseMr { project, iid },
            ConfirmAction::ApproveMr(project, iid) => Cmd::SpawnApproveMr { project, iid },
            ConfirmAction::MergeMr(project, iid) => Cmd::SpawnMergeMr { project, iid },
            ConfirmAction::QuitApp => unreachable!(),
        };
        self.pending_cmds.push(spawn_cmd);
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
            self.dirty.issues = true;
        }
        self.pending_cmds.push(Cmd::PersistIssues);
        self.pending_cmds.push(Cmd::SpawnSetStatus {
            project: project.to_string(),
            issue_id,
            iid,
            status_id,
            status_display: status_name.to_string(),
        });
    }

    fn handle_picker_result(&mut self, values: &[String]) {
        // Determine what we picked for based on overlay context
        let context = std::mem::replace(&mut self.overlay, Overlay::None);
        match context {
            Overlay::Picker(PickerContext::Assignee) => {
                if let Some(username) = values.first() {
                    self.update_assignee(username);
                }
            }
            Overlay::Picker(PickerContext::Team) => {
                if let Some(name) = values.first() {
                    if name == "All" {
                        self.active_team = None;
                    } else {
                        self.active_team = self.config.teams.iter().position(|t| t.name == *name);
                    }
                    self.dirty.issues = true;
                    self.dirty.mrs = true;
                    self.dirty.selection = true;
                }
            }
            Overlay::Picker(PickerContext::ReplyThread(infos)) => {
                if let (Some(ps), Some(picked_label)) = (&self.picker_state, values.first())
                    && let Some(idx) = ps.items.iter().position(|item| item == picked_label)
                    && let Some(info) = infos.get(idx)
                {
                    self.reply_discussion_id = Some(info.discussion_id.clone());
                    self.comment_input = crate::ui::components::input::CommentInput::default();
                    self.overlay = Overlay::CommentInput;
                }
            }
            _ => {}
        }
    }

    /// Return the index into self.issues / self.mrs for the currently selected item,
    /// plus (`project_path`, iid, `is_mr`).
    fn selected_item_idx(&self) -> Option<(usize, String, u64, bool)> {
        match self.view {
            View::IssueList | View::IssueDetail => {
                let idx = self.issue_list_state.list.selected_index()?;
                let item = self.issues.get(idx)?;
                Some((idx, item.project_path.clone(), item.issue.iid, false))
            }
            View::Planning => {
                let col = self.planning_state.focused_column;
                let idx = self.planning_state.columns[col].list.selected_index()?;
                let item = self.issues.get(idx)?;
                Some((idx, item.project_path.clone(), item.issue.iid, false))
            }
            View::MrList | View::MrDetail => {
                let idx = self.mr_list_state.list.selected_index()?;
                let item = self.mrs.get(idx)?;
                Some((idx, item.project_path.clone(), item.mr.iid, true))
            }
            View::Dashboard => {
                let col = self.iteration_board_state.focused_column;
                let idx = self
                    .iteration_board_state
                    .columns
                    .get(col)
                    .and_then(|c| c.list.selected_index())?;
                let item = self.issues.get(idx)?;
                Some((idx, item.project_path.clone(), item.issue.iid, false))
            }
        }
    }

    fn handle_label_editor_result(&mut self, labels: &[String]) {
        for label in labels {
            *self.label_usage.entry(label.clone()).or_insert(0) += 1;
        }
        self.update_labels(labels);
        self.pending_cmds.push(Cmd::PersistLabelUsage);
    }

    fn update_labels(&mut self, labels: &[String]) {
        let Some((idx, project, iid, is_mr)) = self.selected_item_idx() else {
            return;
        };

        let client = self.client.clone();
        let tx = self.async_tx.clone();

        if is_mr {
            self.mrs[idx].mr.labels = labels.to_vec();
            let payload = serde_json::json!({"labels": labels.join(",")});
            tokio::spawn(async move {
                let result = client.update_mr(&project, iid, payload).await;
                let _ = tx.send(AsyncMsg::MrUpdated(result, project));
            });
        } else {
            let old_labels = &self.issues[idx].issue.labels;
            let new_set: std::collections::HashSet<&str> =
                labels.iter().map(String::as_str).collect();
            let old_set: std::collections::HashSet<&str> =
                old_labels.iter().map(String::as_str).collect();

            let add_gids: Vec<String> = new_set
                .difference(&old_set)
                .filter_map(|name| self.label_id_by_name(name))
                .map(|id| format!("gid://gitlab/Label/{id}"))
                .collect();
            let remove_gids: Vec<String> = old_set
                .difference(&new_set)
                .filter_map(|name| self.label_id_by_name(name))
                .map(|id| format!("gid://gitlab/Label/{id}"))
                .collect();

            self.issues[idx].issue.labels = labels.to_vec();
            let issue_id = self.issues[idx].issue.id;
            let input = serde_json::json!({
                "labelsWidget": {
                    "addLabelIds": add_gids,
                    "removeLabelIds": remove_gids,
                }
            });
            tokio::spawn(async move {
                let result = client.update_issue(issue_id, input).await;
                let _ = tx.send(AsyncMsg::IssueUpdated(result));
            });
        }
    }

    fn label_id_by_name(&self, name: &str) -> Option<u64> {
        self.labels.iter().find(|l| l.name == name).map(|l| l.id)
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
            self.mrs[idx].mr.assignees = vec![placeholder.clone()];
        } else {
            self.issues[idx].issue.assignees = vec![placeholder];
        }

        let issue_id = if is_mr {
            0 // not used for MRs
        } else {
            self.issues[idx].issue.id
        };

        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let username = username.to_string();
        tokio::spawn(async move {
            let users = client.search_users(&username).await;
            match users {
                Ok(users) => {
                    if let Some(user) = users.first() {
                        if is_mr {
                            let payload = serde_json::json!({"assignee_ids": [user.id]});
                            let result = client.update_mr(&project, iid, payload).await;
                            let _ = tx.send(AsyncMsg::MrUpdated(result, project));
                        } else {
                            let input = serde_json::json!({
                                "assigneesWidget": {
                                    "assigneeIds": [format!("gid://gitlab/User/{}", user.id)]
                                }
                            });
                            let result = client.update_issue(issue_id, input).await;
                            let _ = tx.send(AsyncMsg::IssueUpdated(result));
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

    fn accept_completion(&mut self) {
        let Some(item) = self.autocomplete.selected_item().cloned() else {
            return;
        };
        let trigger_pos = self.autocomplete.trigger_pos;
        let trigger_len =
            crate::ui::components::autocomplete::AutocompleteState::trigger_char_len();
        let text = self.comment_input.text();
        let cursor = self.comment_input.cursor_byte_pos();

        let mut new_value = String::with_capacity(text.len() + item.insert.len());
        new_value.push_str(&text[..trigger_pos + trigger_len]);
        new_value.push_str(&item.insert);
        new_value.push(' ');
        new_value.push_str(&text[cursor..]);

        let new_cursor = trigger_pos + trigger_len + item.insert.len() + 1;
        self.comment_input
            .set_text_and_cursor(&new_value, new_cursor);
        self.autocomplete.dismiss();
    }

    fn submit_comment(&mut self, body: &str) {
        let client = self.client.clone();
        let tx = self.async_tx.clone();
        let body = body.to_string();

        let (project, iid, is_mr) = match self.view {
            View::IssueList | View::Planning => {
                let selected = if self.view == View::Planning {
                    self.planning_state.selected_issue(&self.issues).cloned()
                } else {
                    self.issue_list_state.selected_issue(&self.issues).cloned()
                };
                if let Some(item) = selected {
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
            View::Dashboard => return,
        };

        let reply_id = self.reply_discussion_id.take();
        self.loading = true;
        tokio::spawn(async move {
            let create_result = match &reply_id {
                Some(disc_id) => {
                    if is_mr {
                        client
                            .reply_to_mr_discussion(&project, iid, disc_id, &body)
                            .await
                    } else {
                        client
                            .reply_to_issue_discussion(&project, iid, disc_id, &body)
                            .await
                    }
                }
                None => {
                    if is_mr {
                        client.create_mr_note(&project, iid, &body).await
                    } else {
                        client.create_issue_note(&project, iid, &body).await
                    }
                }
            };
            if let Err(e) = create_result {
                let _ = tx.send(AsyncMsg::ActionDone(Err(e)));
                return;
            }
            // Re-fetch discussions so the UI shows the new comment
            let discussions = if is_mr {
                client.list_mr_discussions(&project, iid).await
            } else {
                client.list_issue_discussions(&project, iid).await
            };
            let _ = tx.send(AsyncMsg::DiscussionsLoaded(discussions));
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

            self.active_filter_mut().conditions = conditions;
            self.dirty.view_state = true;
            self.pending_cmds.push(Cmd::PersistViewState);
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
            Constraint::Length(1), // Tab bar
            Constraint::Min(1),    // Main content
            Constraint::Length(1), // Status bar
        ])
        .split(area);

        // Tab bar
        crate::ui::components::tab_bar::render(frame, chunks[0], self.view);

        let ctx = crate::ui::RenderCtx {
            label_colors: &self.label_color_map,
        };

        // Render main view
        match self.view {
            View::Dashboard => {
                let current_iter = self.planning_state.current_iteration.as_ref();
                dashboard::render(
                    frame,
                    chunks[1],
                    &self.config,
                    self.active_team,
                    &self.issues,
                    &self.mrs,
                    self.loading,
                    &mut self.iteration_board_state,
                    current_iter,
                    self.iteration_health.as_mut(),
                    &self.shadow_work_cache,
                    &self.unplanned_work_cache,
                );
            }
            View::IssueList => {
                issue_list::render(
                    frame,
                    chunks[1],
                    &mut self.issue_list_state,
                    &self.issues,
                    &ctx,
                );
            }
            View::IssueDetail => {
                if let Some(item) = self.current_detail_issue().cloned() {
                    issue_detail::render(frame, chunks[1], &item, &self.issue_detail_state, &ctx);
                }
            }
            View::MrList => {
                mr_list::render(frame, chunks[1], &mut self.mr_list_state, &self.mrs, &ctx);
            }
            View::MrDetail => {
                if let Some(item) = self.current_detail_mr().cloned() {
                    mr_detail::render(frame, chunks[1], &item, &self.mr_detail_state, &ctx);
                }
            }
            View::Planning => {
                planning::render(
                    frame,
                    chunks[1],
                    &mut self.planning_state,
                    &self.issues,
                    &self.config,
                    self.active_team,
                    &ctx,
                );
            }
        }

        // Status bar
        let team_name = self
            .active_team
            .and_then(|idx| self.config.teams.get(idx))
            .map_or("all", |t| t.name.as_str());
        let view_name = match self.view {
            View::Dashboard => "Dashboard",
            View::IssueList => "Issues",
            View::IssueDetail => "Issue Detail",
            View::MrList => "Merge Requests",
            View::MrDetail => "MR Detail",
            View::Planning => "Planning",
        };
        let item_count = match self.view {
            View::IssueList => self.issue_list_state.list.len(),
            View::MrList => self.mr_list_state.list.len(),
            View::Planning => self
                .planning_state
                .columns
                .iter()
                .map(|c| c.list.len())
                .sum(),
            _ => self.issues.len() + self.mrs.len(),
        };
        // Skip Global and Navigation groups — tabs handle those
        let binding_hints: Vec<(&str, &str)> = keybindings::binding_groups_for_view(self.view)
            .iter()
            .filter(|g| g.title != "Global" && g.title != "Navigation")
            .flat_map(|g| g.bindings.iter())
            .filter(|b| b.visible_in_help())
            .take(8)
            .map(|b| (b.label, b.description))
            .collect();
        let hints = binding_hints.as_slice();
        crate::ui::components::status_bar::render(
            frame,
            chunks[2],
            &crate::ui::components::status_bar::StatusBarProps {
                view_name,
                team_name,
                item_count,
                loading: self.loading,
                loading_msg: self.loading_msg,
                error: self.error.as_deref(),
                last_fetched_at: self.last_fetched_at,
                last_fetch_ms: self.last_fetch_ms,
                hints,
            },
        );

        // Render overlay on top
        match &self.overlay {
            Overlay::None => {}
            Overlay::Help => {
                help::render(frame, area, self.view);
            }
            Overlay::FilterEditor => {
                filter_editor::render(frame, area, &mut self.filter_editor_state, &ctx);
            }
            Overlay::Confirm(action) => {
                let (title, msg) = match action {
                    ConfirmAction::CloseIssue(_, iid) => {
                        ("Close Issue", format!("Close issue #{iid}?"))
                    }
                    ConfirmAction::ReopenIssue(_, iid) => {
                        ("Reopen Issue", format!("Reopen issue #{iid}?"))
                    }
                    ConfirmAction::CloseMr(_, iid) => ("Close MR", format!("Close MR !{iid}?")),
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
                let popup = centered_rect(60, 40, area);
                ratatui::widgets::Clear.render(popup, frame.buffer_mut());
                let title = if self.reply_discussion_id.is_some() {
                    "Reply (Enter submit, C-j newline)"
                } else {
                    "Comment (Enter submit, C-j newline)"
                };
                crate::ui::components::input::render(frame, popup, &mut self.comment_input, title);
                crate::ui::components::autocomplete::render(frame, popup, &self.autocomplete);
            }
            Overlay::Chord(_) => {
                if let Some(ref cs) = self.chord_state {
                    chord_popup::render(frame, area, cs);
                }
            }
            Overlay::LabelEditor => {
                if let Some(ref les) = self.label_editor_state {
                    label_editor::render(frame, area, les, &self.label_color_map);
                }
            }
            Overlay::Error(msg) => {
                error_popup::render(frame, area, msg);
            }
        }
    }
}

/// Build display labels and subtitles for the thread reply picker.
fn build_thread_picker_display(infos: &[ThreadPickerInfo]) -> (Vec<String>, Vec<String>) {
    let labels: Vec<String> = infos
        .iter()
        .map(|t| format!("@{}: {}", t.author, t.preview))
        .collect();
    let subtitles: Vec<String> = infos
        .iter()
        .map(|t| {
            if t.reply_count > 0 {
                let last_author = t.last_author.as_deref().unwrap_or("?");
                let last_msg = t.last_preview.as_deref().unwrap_or("");
                format!(
                    "\u{21B3} @{}: {}  ({} {})",
                    last_author,
                    last_msg,
                    t.reply_count,
                    if t.reply_count == 1 {
                        "reply"
                    } else {
                        "replies"
                    }
                )
            } else {
                String::new()
            }
        })
        .collect();
    (labels, subtitles)
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
