use crate::gitlab::types::{Iteration, TrackedIssue, TrackedMergeRequest};

/// Side-effect descriptors returned from update logic.
///
/// Update handlers mutate model state in place and push `Cmd` values to
/// `self.pending_cmds`.  After the handler returns the event loop drains
/// the queue via `execute_pending_cmds()` — this is the *only* place that
/// performs I/O (disk, network, browser).
///
/// Simple API mutations whose spawn logic is just "call one client method
/// and send the result" are modelled as `Spawn*` variants.  Complex flows
/// that need state access for GID lookups, user searches, etc. keep their
/// `tokio::spawn` in the originating method and only use dirty flags +
/// `Cmd::Persist*` for the persistence side.
#[derive(Debug)]
pub enum Cmd {
    // ── Persistence (targeted SQLite writes) ─────────────────────────
    PersistIssues,
    PersistMrs,
    /// Persist a snapshot of all issues (open + closed) taken before the
    /// in-memory open-only filter.  Used by `IssuesLoaded` so closed issues
    /// accumulate in the DB for shadow-work queries.
    PersistIssuesFull(Vec<TrackedIssue>),
    /// Same as `PersistIssuesFull` but for merge requests.
    PersistMrsFull(Vec<TrackedMergeRequest>),
    PersistLabels,
    PersistIterations,
    PersistStatuses {
        project: String,
    },
    PersistViewState,
    PersistUnplannedWork,
    PersistLabelUsage,
    PersistLastFetchedAt(u64),

    // ── API fetches ──────────────────────────────────────────────────
    FetchAll,
    FetchAllFull,
    FetchHealthData,

    // ── API mutations (simple spawn-and-forget) ──────────────────────
    SpawnCloseIssue {
        issue_id: u64,
    },
    SpawnReopenIssue {
        issue_id: u64,
    },
    SpawnCloseMr {
        project: String,
        iid: u64,
    },
    SpawnApproveMr {
        project: String,
        iid: u64,
    },
    SpawnMergeMr {
        project: String,
        iid: u64,
    },
    SpawnMoveIteration {
        issue_id: u64,
        target_gid: Option<String>,
        old_iteration: Option<Iteration>,
    },
    SpawnSetStatus {
        project: String,
        issue_id: u64,
        iid: u64,
        status_id: String,
        status_display: String,
    },
}

/// Tracks which data domains changed during an update cycle.
///
/// After every `handle_key` / `handle_async_msg`, `reconcile()` reads these
/// flags and runs exactly the downstream refilter/refresh/health calls that
/// are needed — nothing more, nothing less.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Dirty {
    /// `self.issues` Vec was mutated (items added, removed, or modified).
    pub issues: bool,
    /// `self.mrs` Vec was mutated.
    pub mrs: bool,
    /// `self.labels` changed.
    pub labels: bool,
    /// `self.iterations` changed.
    pub iterations: bool,
    /// `self.work_item_statuses` changed.
    pub statuses: bool,
    /// Filter conditions, sort specs, or fuzzy query changed.
    pub view_state: bool,
    /// View or selection changed (needs `refresh_focused`).
    pub selection: bool,
}

impl Dirty {
    pub fn any(&self) -> bool {
        self.issues
            || self.mrs
            || self.labels
            || self.iterations
            || self.statuses
            || self.view_state
            || self.selection
    }
}

/// Result of a focus node handling a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    /// Event was consumed. Stop bubbling.
    Consumed,
    /// Event was not handled. Parent should try.
    Bubble,
    /// Application should quit.
    Quit,
}

impl EventResult {
    pub fn handled(self) -> bool {
        !matches!(self, Self::Bubble)
    }
}
