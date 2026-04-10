use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::gitlab::types::{ProjectLabel, TrackedIssue, TrackedMergeRequest, WorkItemStatus};

#[derive(Serialize, Deserialize)]
pub struct CacheData {
    /// Unix timestamp (seconds) when cache was written
    pub saved_at: u64,
    pub issues: Vec<TrackedIssue>,
    pub mrs: Vec<TrackedMergeRequest>,
    pub labels: Vec<ProjectLabel>,
    pub work_item_statuses: HashMap<String, Vec<WorkItemStatus>>,
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
pub fn save(
    issues: &[TrackedIssue],
    mrs: &[TrackedMergeRequest],
    labels: &[ProjectLabel],
    work_item_statuses: &HashMap<String, Vec<WorkItemStatus>>,
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
    };
    if let Ok(json) = serde_json::to_string(&data) {
        let _ = fs::write(&path, json);
    }
}
