use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::filter::FilterCondition;
use crate::gitlab::types::{
    Iteration, ProjectLabel, TrackedIssue, TrackedMergeRequest, WorkItemStatus,
};
use crate::sort::SortSpec;

/// Persisted filter/sort state for a single list view.
#[derive(Default, Serialize, Deserialize)]
pub struct ViewState {
    #[serde(default)]
    pub conditions: Vec<FilterCondition>,
    #[serde(default)]
    pub sort_specs: Vec<SortSpec>,
    #[serde(default)]
    pub fuzzy_query: String,
}

/// Legacy JSON cache format — used only for migration to SQLite.
#[derive(Deserialize)]
pub struct CacheData {
    #[allow(dead_code)]
    pub saved_at: u64,
    pub issues: Vec<TrackedIssue>,
    pub mrs: Vec<TrackedMergeRequest>,
    pub labels: Vec<ProjectLabel>,
    pub work_item_statuses: HashMap<String, Vec<WorkItemStatus>>,
    #[serde(default)]
    pub label_usage: HashMap<String, u32>,
    #[serde(default)]
    pub issue_view_state: Option<ViewState>,
    #[serde(default)]
    pub mr_view_state: Option<ViewState>,
    #[serde(default)]
    pub scope_creep_dates: HashMap<u64, DateTime<Utc>>,
    #[serde(default)]
    pub shadow_work_issues: Vec<TrackedIssue>,
    #[serde(default)]
    pub iterations: Vec<Iteration>,
}
