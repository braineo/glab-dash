mod actions;
mod async_msg;
mod execute;
mod fetch;
mod filter;
mod issue_actions;
mod keys;
mod mr_actions;
mod overlay;
mod render;

use anyhow::Result;
use crossterm::event::KeyEvent;
use tokio::sync::mpsc;

use crate::cmd::{Cmd, Dirty};
use crate::config::Config;
use crate::db::{Db, ViewState};
use crate::gitlab::client::GitLabClient;
use crate::gitlab::types::{
    Issue, Iteration, MergeRequest, ProjectLabel, TrackedIssue, TrackedMergeRequest, WorkItemStatus,
};
use crate::ui::views::Views;
use crate::ui::views::{dashboard, filter_editor};

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
    Confirm,
    Picker,
    Chord,
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

/// Messages from async operations
pub enum AsyncMsg {
    IssuesLoaded(Result<Vec<TrackedIssue>>, bool),
    MrsLoaded(
        Result<(Vec<TrackedMergeRequest>, Vec<TrackedMergeRequest>)>,
        bool,
    ),
    DiscussionsLoaded(Result<Vec<crate::gitlab::types::Discussion>>),
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

/// Infrastructure context — immutable during event handling.
pub struct AppCtx {
    pub config: Config,
    pub client: GitLabClient,
    pub async_tx: mpsc::UnboundedSender<AsyncMsg>,
    pub db: Db,
}

/// Domain data — mutated by handlers.
pub struct AppData {
    pub issues: Vec<TrackedIssue>,
    pub mrs: Vec<TrackedMergeRequest>,
    pub labels: Vec<ProjectLabel>,
    pub label_color_map: crate::ui::styles::LabelColors,
    pub iterations: Vec<Iteration>,
    pub work_item_statuses: std::collections::HashMap<String, Vec<WorkItemStatus>>,
    pub label_usage: std::collections::HashMap<String, u32>,
    pub label_sort_orders: std::collections::HashMap<String, Vec<String>>,
    pub shadow_work_cache: Vec<TrackedIssue>,
    pub unplanned_work_cache: std::collections::HashMap<u64, chrono::DateTime<chrono::Utc>>,
    pub unplanned_work_state: FetchState,
}

/// UI layer — views, overlays, TEA accumulators.
/// Callback invoked when a chord popup selection completes.
pub type ChordCallback = Box<dyn FnOnce(String, &mut App)>;
/// Callback invoked when a picker selection completes.
pub type PickerCallback = Box<dyn FnOnce(Vec<String>, &mut App)>;
/// Callback invoked when a confirm dialog is accepted.
pub type ConfirmCallback = Box<dyn FnOnce(&mut App)>;

pub struct UiState {
    pub view: View,
    pub view_stack: Vec<View>,
    pub views: Views,
    pub overlay: Overlay,
    pub focused: Option<FocusedItem>,
    pub active_team: Option<usize>,
    pub chord_state: Option<crate::ui::components::chord_popup::ChordState>,
    pub chord_on_complete: Option<ChordCallback>,
    pub picker_state: Option<crate::ui::components::picker::PickerState>,
    pub picker_on_complete: Option<PickerCallback>,
    pub confirm_title: String,
    pub confirm_message: String,
    pub confirm_on_accept: Option<ConfirmCallback>,
    pub label_editor_state: Option<crate::ui::components::label_editor::LabelEditorState>,
    pub filter_editor_state: filter_editor::FilterEditorState,
    pub comment_input: crate::ui::components::input::CommentInput,
    pub autocomplete: crate::ui::components::autocomplete::AutocompleteState,
    pub reply_discussion_id: Option<String>,
    pub loading: bool,
    pub loading_msg: &'static str,
    pub error: Option<String>,
    pub last_fetched_at: Option<u64>,
    pub fetch_started_at: Option<u64>,
    pub last_fetch_ms: Option<u64>,
    pub needs_redraw: bool,
    pub dirty: Dirty,
    pub pending_cmds: Vec<Cmd>,
}

pub struct App {
    pub ctx: AppCtx,
    pub data: AppData,
    pub ui: UiState,
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
        Self {
            ctx: AppCtx {
                config,
                client,
                async_tx,
                db,
            },
            data: AppData {
                issues: Vec::new(),
                mrs: Vec::new(),
                labels: Vec::new(),
                label_color_map: std::collections::HashMap::new(),
                iterations: Vec::new(),
                work_item_statuses: std::collections::HashMap::new(),
                label_usage: std::collections::HashMap::new(),
                label_sort_orders,
                shadow_work_cache: Vec::new(),
                unplanned_work_cache: std::collections::HashMap::new(),
                unplanned_work_state: FetchState::default(),
            },
            ui: UiState {
                view: View::Dashboard,
                view_stack: Vec::new(),
                views: Views::default(),
                overlay: Overlay::None,
                focused: None,
                active_team: None,
                chord_state: None,
                chord_on_complete: None,
                picker_state: None,
                picker_on_complete: None,
                confirm_title: String::new(),
                confirm_message: String::new(),
                confirm_on_accept: None,
                label_editor_state: None,
                filter_editor_state: filter_editor::FilterEditorState::default(),
                comment_input: crate::ui::components::input::CommentInput::default(),
                autocomplete: crate::ui::components::autocomplete::AutocompleteState::default(),
                reply_discussion_id: None,
                loading: false,
                loading_msg: "",
                error: None,
                last_fetched_at: None,
                fetch_started_at: None,
                last_fetch_ms: None,
                needs_redraw: true,
                dirty: Dirty::default(),
                pending_cmds: Vec::new(),
            },
        }
    }

    /// Load cached data for instant startup display.
    /// Load persisted data from SQLite for instant startup display.
    pub fn load_from_db(&mut self) {
        // Load open issues and MRs for display
        self.data.issues = self.ctx.db.load_issues(Some("opened")).unwrap_or_default();
        self.data.mrs = self.ctx.db.load_mrs(Some("opened")).unwrap_or_default();
        self.data.labels = self.ctx.db.load_labels().unwrap_or_default();
        self.data.work_item_statuses = self.ctx.db.load_work_item_statuses().unwrap_or_default();

        // Load key-value metadata
        if let Ok(Some(usage)) = self.ctx.db.get_kv("label_usage") {
            self.data.label_usage = usage;
        }
        // Restore last_fetched_at so the first fetch is incremental (fast)
        if let Ok(Some(ts)) = self.ctx.db.get_kv::<u64>("last_fetched_at") {
            self.ui.last_fetched_at = Some(ts);
        }

        // Restore persisted view state (filters, sorts, fuzzy queries)
        if let Ok(Some(vs)) = self.ctx.db.get_kv::<ViewState>("issue_view_state") {
            self.ui.views.issue_list.filter.conditions = vs.conditions;
            self.ui.views.issue_list.filter.sort_specs = vs.sort_specs;
            self.ui.views.issue_list.filter.fuzzy_query = vs.fuzzy_query;
        }
        if let Ok(Some(vs)) = self.ctx.db.get_kv::<ViewState>("mr_view_state") {
            self.ui.views.mr_list.filter.conditions = vs.conditions;
            self.ui.views.mr_list.filter.sort_specs = vs.sort_specs;
            self.ui.views.mr_list.filter.fuzzy_query = vs.fuzzy_query;
        }

        // Restore iterations (before health data so classify_iterations sees them)
        self.data.iterations = self.ctx.db.load_iterations().unwrap_or_default();
        if !self.data.iterations.is_empty() {
            self.classify_iterations();
        }

        // Restore unplanned work dates
        if let Ok(Some(dates)) = self.ctx.db.get_kv("unplanned_work_dates") {
            self.data.unplanned_work_cache = dates;
            self.data.unplanned_work_state = FetchState::Done;
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
        self.ui.focused = match self.ui.view {
            View::IssueList | View::IssueDetail => self
                .ui
                .views
                .issue_list
                .selected_issue(&self.data.issues)
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
            View::MrList | View::MrDetail => {
                self.ui
                    .views
                    .mr_list
                    .selected_mr(&self.data.mrs)
                    .map(|item| FocusedItem::Mr {
                        project: item.project_path.clone(),
                        iid: item.mr.iid,
                    })
            }
            View::Planning => self
                .ui
                .views
                .planning
                .selected_issue(&self.data.issues)
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
            View::Dashboard if self.ui.views.board.health_focused => self
                .ui
                .views
                .health
                .as_ref()
                .and_then(|h| h.selected_issue(&self.data.issues, &self.data.shadow_work_cache))
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
            View::Dashboard => self
                .ui
                .views
                .board
                .selected_issue(&self.data.issues)
                .map(|item| FocusedItem::Issue {
                    project: item.project_path.clone(),
                    id: item.issue.id,
                    iid: item.issue.iid,
                }),
        };
    }

    /// Get members for the active team, or empty vec for "All" view.
    /// Used for implicit team filtering — empty means no filter.
    fn active_team_members(&self) -> Vec<String> {
        match self.ui.active_team {
            Some(idx) => self.ctx.config.team_members(idx),
            None => Vec::new(),
        }
    }

    /// Get member list for pickers (assignee, filter suggestions).
    /// Returns all configured members in "All" mode, team members otherwise.
    fn picker_members(&self) -> Vec<String> {
        match self.ui.active_team {
            Some(idx) => self.ctx.config.team_members(idx),
            None => self.ctx.config.all_members(),
        }
    }

    fn rebuild_label_color_map(&mut self) {
        self.data.label_color_map = self
            .data
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
        let d = std::mem::take(&mut self.ui.dirty);
        if !d.any() {
            return;
        }

        if d.labels {
            self.rebuild_label_color_map();
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
        if d.issues {
            self.refresh_shadow_work();
        }
        if d.issues || d.iterations || d.statuses {
            self.compute_iteration_health();
        }
    }

    /// Process an async message: update state, reconcile, execute side-effects.
    pub fn process_async_msg(&mut self, msg: AsyncMsg) {
        self.ui.dirty = Dirty::default();
        self.ui.pending_cmds.clear();
        self.handle_async_msg(msg);
        self.reconcile();
        self.execute_pending_cmds();
    }

    /// Process a key event: update state, reconcile, execute side-effects.
    /// Returns `true` if the app should quit.
    pub fn process_key(&mut self, key: KeyEvent) -> bool {
        self.ui.dirty = Dirty::default();
        self.ui.pending_cmds.clear();
        self.ui.needs_redraw = true;
        let quit = self.handle_key(key);
        self.reconcile();
        self.execute_pending_cmds();
        quit
    }

    pub fn refilter_issues(&mut self) {
        let me = self.ctx.config.me.clone();
        let members = self.active_team_members();
        self.ui.views.issue_list.apply_filters(
            &self.data.issues,
            &me,
            &members,
            &self.data.label_sort_orders,
        );
    }

    pub fn refilter_planning(&mut self) {
        self.ui
            .views
            .planning
            .partition_issues(&self.data.issues, &self.data.label_sort_orders);
    }

    pub fn refilter_iteration_board(&mut self) {
        let current_iter = self.ui.views.planning.current_iteration.as_ref();
        let me = self.ctx.config.me.clone();
        let members = self.active_team_members();
        self.ui.views.board.partition_issues(
            &self.data.issues,
            current_iter,
            &self.data.label_sort_orders,
            &me,
            &members,
        );
    }

    fn classify_iterations(&mut self) {
        // Iterations come sorted by CADENCE_AND_DUE_DATE_ASC.
        // States: "closed", "current", "upcoming".
        // Find current, then adjacent entries are previous/next.
        let current_pos = self
            .data
            .iterations
            .iter()
            .position(|i| i.state == "current");

        let new_current = current_pos.map(|pos| self.data.iterations[pos].clone());

        // Reset health caches if the current iteration changed
        let iter_changed = match (&self.ui.views.planning.current_iteration, &new_current) {
            (Some(old), Some(new)) => old.id != new.id,
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        };
        if iter_changed {
            self.data.unplanned_work_cache.clear();
            self.data.shadow_work_cache.clear();
            self.data.unplanned_work_state = FetchState::Idle;
            self.ui.views.health = None;
        }

        self.ui.views.planning.current_iteration = new_current;

        self.ui.views.planning.prev_iteration = current_pos
            .and_then(|pos| pos.checked_sub(1))
            .map(|pos| self.data.iterations[pos].clone());

        self.ui.views.planning.next_iteration = current_pos
            .and_then(|pos| self.data.iterations.get(pos + 1))
            .cloned();

        // Build iteration board columns from available statuses
        self.rebuild_iteration_board_columns();
    }

    fn rebuild_iteration_board_columns(&mut self) {
        // Collect all statuses from all tracked projects
        let mut all_statuses: Vec<WorkItemStatus> = Vec::new();
        for project in &self.ctx.config.tracking_projects {
            if let Some(statuses) = self.data.work_item_statuses.get(project) {
                for s in statuses {
                    if !all_statuses.iter().any(|existing| existing.name == s.name) {
                        all_statuses.push(s.clone());
                    }
                }
            }
        }
        if !all_statuses.is_empty() {
            self.ui
                .views
                .board
                .build_columns(&all_statuses, &self.ctx.config.kanban_columns);
        }
    }

    /// Refresh shadow work cache from DB (closed issues in current iteration range).
    fn refresh_shadow_work(&mut self) {
        let Some(iter) = self.ui.views.planning.current_iteration.as_ref() else {
            self.data.shadow_work_cache.clear();
            return;
        };
        let (Some(start), Some(end)) = (iter.start_date.as_ref(), iter.due_date.as_ref()) else {
            self.data.shadow_work_cache.clear();
            return;
        };
        let closed_after = format!("{start}T00:00:00+00:00");
        let closed_before = format!("{end}T23:59:59+00:00");
        if let Ok(shadow) =
            self.ctx
                .db
                .query_shadow_work(&closed_after, &closed_before, Some(&iter.id))
        {
            self.data.shadow_work_cache = shadow;
        }
    }

    /// Recompute iteration health metrics from current data.
    fn compute_iteration_health(&mut self) {
        let Some(current_iter) = self.ui.views.planning.current_iteration.as_ref() else {
            self.ui.views.health = None;
            return;
        };

        self.ui.views.health = Some(dashboard::compute_health(
            &self.data.issues,
            current_iter,
            &self.data.unplanned_work_cache,
            self.data.unplanned_work_state != FetchState::Done,
            &self.data.shadow_work_cache,
            self.ui.views.health.as_ref(),
        ));
    }

    fn refilter_mrs(&mut self) {
        let me = self.ctx.config.me.clone();
        let members = self.active_team_members();
        self.ui.views.mr_list.apply_filters(
            &self.data.mrs,
            &me,
            &members,
            &self.data.label_sort_orders,
        );
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
