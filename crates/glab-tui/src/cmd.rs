use glab_core::domain::{Issue, Iteration, MergeRequest};
use glab_core::domain::{ItemKind, Relation};

/// Handlers mutate state in place and push these to `self.ui.pending_cmds`;
/// the event loop drains them through `execute_pending_cmds`, the only place
/// that performs I/O.
///
/// A mutation that is one client call and a result message is a `Spawn*`
/// variant.  A flow needing state access for gid lookups or user searches
/// keeps its `tokio::spawn` in the originating method.
#[derive(Debug)]
pub enum Cmd {
    PersistIssues,
    PersistMrs,
    /// Snapshotted before the in-memory open-only filter, so shadow-work
    /// queries still have the closed ones.
    PersistIssuesFull(Vec<Issue>),
    PersistMrsFull(Vec<MergeRequest>),
    PersistLabels,
    PersistIterations,
    PersistStatuses {
        project: String,
    },
    PersistViewState,
    PersistUnplannedWork,
    PersistLabelUsage,
    PersistLastFetchedAt(u64),

    FetchAll,
    FetchAllFull,
    FetchHealthData,

    SpawnCloseIssue {
        issue_id: String,
    },
    SpawnReopenIssue {
        issue_id: String,
    },
    SpawnCloseMr {
        project: String,
        iid: String,
    },
    SpawnApproveMr {
        project: String,
        iid: String,
    },
    SpawnMergeMr {
        project: String,
        iid: String,
    },
    SpawnMoveIteration {
        issue_id: String,
        target_gid: Option<String>,
        old_iteration: Option<Iteration>,
    },

    FetchRelated {
        kind: ItemKind,
        gid: String,
    },

    AddLink {
        gid: String,
        target_gid: String,
        relation: Relation,
    },
    RemoveLink {
        gid: String,
        target_gid: String,
    },

    MentionInMr {
        gid: String,
        target_gid: String,
        relation: Relation,
    },
    SpawnSetStatus {
        project: String,
        issue_id: String,
        iid: String,
        status_id: String,
        status_display: String,
    },
}

/// `reconcile` reads these and runs the downstream refilter / refresh / health
/// calls they imply.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Dirty {
    pub issues: bool,
    pub mrs: bool,
    pub labels: bool,
    pub iterations: bool,
    pub statuses: bool,
    /// Filter conditions, sort specs or the fuzzy query.
    pub view_state: bool,
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

/// What a view handler writes back: the data it dirtied, the [`Cmd`]s it
/// queued, and whether the frame needs repainting.
pub struct Effects<'a> {
    pub dirty: &'a mut Dirty,
    pub cmds: &'a mut Vec<Cmd>,
    pub needs_redraw: &'a mut bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    Consumed,
    Bubble,
    Quit,
}

impl EventResult {
    pub fn handled(self) -> bool {
        !matches!(self, Self::Bubble)
    }
}
