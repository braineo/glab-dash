use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::filter::FilterCondition;
use crate::gitlab::types::{ProjectLabel, TrackedIssue, TrackedMergeRequest, WorkItemStatus};
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

#[derive(Serialize, Deserialize)]
pub struct CacheData {
    /// Unix timestamp (seconds) when cache was written
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
    /// Scope creep: issue_id → timestamp when added to current iteration.
    #[serde(default)]
    pub scope_creep_dates: HashMap<u64, DateTime<Utc>>,
    /// Shadow work: closed issues updated during current iteration date range.
    #[serde(default)]
    pub shadow_work_issues: Vec<TrackedIssue>,
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("glab-dash").join("cache.json"))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Load cached data if it exists. Returns None on any error.
pub fn load() -> Option<CacheData> {
    let path = cache_path()?;
    let data = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save data to cache. Errors are silently ignored (cache is best-effort).
#[allow(clippy::too_many_arguments)]
pub fn save(
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
    labels: &[ProjectLabel],
    work_item_statuses: &HashMap<String, Vec<WorkItemStatus>>,
    label_usage: &HashMap<String, u32>,
    issue_view_state: Option<ViewState>,
    mr_view_state: Option<ViewState>,
    scope_creep_dates: &HashMap<u64, DateTime<Utc>>,
    shadow_work_issues: &[TrackedIssue],
) {
    let Some(path) = cache_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let data = CacheData {
        saved_at: now_secs(),
        issues: issues.to_vec(),
        mrs: mrs.to_vec(),
        labels: labels.to_vec(),
        work_item_statuses: work_item_statuses.clone(),
        label_usage: label_usage.clone(),
        issue_view_state,
        mr_view_state,
        scope_creep_dates: scope_creep_dates.clone(),
        shadow_work_issues: shadow_work_issues.to_vec(),
    };
    if let Ok(json) = serde_json::to_string(&data) {
        let _ = fs::write(&path, json);
    }
}
