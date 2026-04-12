use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Serialize, de::DeserializeOwned};

use crate::cache;
use crate::gitlab::types::{
    Iteration, ProjectLabel, TrackedIssue, TrackedMergeRequest, WorkItemStatus,
};

const SCHEMA_VERSION: u32 = 1;

/// SQLite-backed persistence layer.
///
/// Replaces the old JSON cache with targeted per-table writes.
/// All methods are synchronous — individual writes take microseconds,
/// batch upserts single-digit milliseconds.
pub struct Db {
    conn: Connection,
}

fn db_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("glab-dash").join("data.db"))
}

fn json_cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("glab-dash").join("cache.json"))
}

impl Db {
    /// Open (or create) the database at `~/.cache/glab-dash/data.db`.
    /// Runs schema migrations and one-time JSON→SQLite migration if needed.
    pub fn open() -> Result<Self> {
        let path = db_path().context("could not determine cache directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        let db = Self { conn };
        db.migrate()?;

        // One-time migration from old JSON cache
        if let Some(json_path) = json_cache_path()
            && json_path.exists()
        {
            let _ = db.migrate_from_json(&json_path);
        }

        Ok(db)
    }

    /// Open an in-memory database (for tests).
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    // ── Schema migration ────────────────────────────────────────────

    fn migrate(&self) -> Result<()> {
        let version: u32 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            self.conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS issues (
                    id           INTEGER PRIMARY KEY,
                    iid          INTEGER NOT NULL,
                    project_path TEXT NOT NULL,
                    state        TEXT NOT NULL,
                    updated_at   TEXT NOT NULL,
                    data         TEXT NOT NULL,
                    UNIQUE(project_path, iid)
                );
                CREATE INDEX IF NOT EXISTS idx_issues_state ON issues(state);
                CREATE INDEX IF NOT EXISTS idx_issues_updated ON issues(updated_at);

                CREATE TABLE IF NOT EXISTS merge_requests (
                    id           INTEGER PRIMARY KEY,
                    iid          INTEGER NOT NULL,
                    project_path TEXT NOT NULL,
                    state        TEXT NOT NULL,
                    updated_at   TEXT NOT NULL,
                    data         TEXT NOT NULL,
                    UNIQUE(project_path, iid)
                );
                CREATE INDEX IF NOT EXISTS idx_mrs_state ON merge_requests(state);

                CREATE TABLE IF NOT EXISTS labels (
                    id   INTEGER PRIMARY KEY,
                    data TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS iterations (
                    id   TEXT PRIMARY KEY,
                    data TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS work_item_statuses (
                    project_path TEXT PRIMARY KEY,
                    data         TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS kv (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                ",
            )?;
        }

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    // ── Batch upserts (after API fetch) ─────────────────────────────

    pub fn upsert_issues(&self, issues: &[TrackedIssue]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO issues (id, iid, project_path, state, updated_at, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for item in issues {
                let data = serde_json::to_string(item).context("serialize TrackedIssue")?;
                stmt.execute(params![
                    item.issue.id,
                    item.issue.iid,
                    item.project_path,
                    item.issue.state,
                    item.issue.updated_at.to_rfc3339(),
                    data,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_mrs(&self, mrs: &[TrackedMergeRequest]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO merge_requests (id, iid, project_path, state, updated_at, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for item in mrs {
                let data = serde_json::to_string(item).context("serialize TrackedMergeRequest")?;
                stmt.execute(params![
                    item.mr.id,
                    item.mr.iid,
                    item.project_path,
                    item.mr.state,
                    item.mr.updated_at.to_rfc3339(),
                    data,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_labels(&self, labels: &[ProjectLabel]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt =
                tx.prepare_cached("INSERT OR REPLACE INTO labels (id, data) VALUES (?1, ?2)")?;
            for label in labels {
                let data = serde_json::to_string(label).context("serialize ProjectLabel")?;
                stmt.execute(params![label.id, data])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_iterations(&self, iters: &[Iteration]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt =
                tx.prepare_cached("INSERT OR REPLACE INTO iterations (id, data) VALUES (?1, ?2)")?;
            for iter in iters {
                let data = serde_json::to_string(iter).context("serialize Iteration")?;
                stmt.execute(params![iter.id, data])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_work_item_statuses(&self, project: &str, statuses: &[WorkItemStatus]) -> Result<()> {
        let data = serde_json::to_string(statuses)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO work_item_statuses (project_path, data) VALUES (?1, ?2)",
            params![project, data],
        )?;
        Ok(())
    }

    // ── Reads ───────────────────────────────────────────────────────

    pub fn load_issues(&self, state: Option<&str>) -> Result<Vec<TrackedIssue>> {
        let mut items = Vec::new();
        if let Some(state) = state {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT data FROM issues WHERE state = ?1")?;
            let rows = stmt.query_map(params![state], |row| row.get::<_, String>(0))?;
            for row in rows {
                let json = row?;
                if let Ok(item) = serde_json::from_str(&json) {
                    items.push(item);
                }
            }
        } else {
            let mut stmt = self.conn.prepare_cached("SELECT data FROM issues")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                let json = row?;
                if let Ok(item) = serde_json::from_str(&json) {
                    items.push(item);
                }
            }
        }
        Ok(items)
    }

    pub fn load_mrs(&self, state: Option<&str>) -> Result<Vec<TrackedMergeRequest>> {
        let mut items = Vec::new();
        if let Some(state) = state {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT data FROM merge_requests WHERE state = ?1")?;
            let rows = stmt.query_map(params![state], |row| row.get::<_, String>(0))?;
            for row in rows {
                let json = row?;
                if let Ok(item) = serde_json::from_str(&json) {
                    items.push(item);
                }
            }
        } else {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT data FROM merge_requests")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                let json = row?;
                if let Ok(item) = serde_json::from_str(&json) {
                    items.push(item);
                }
            }
        }
        Ok(items)
    }

    pub fn load_labels(&self) -> Result<Vec<ProjectLabel>> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM labels")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            let json = row?;
            if let Ok(item) = serde_json::from_str(&json) {
                items.push(item);
            }
        }
        Ok(items)
    }

    pub fn load_iterations(&self) -> Result<Vec<Iteration>> {
        let mut stmt = self.conn.prepare_cached("SELECT data FROM iterations")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            let json = row?;
            if let Ok(item) = serde_json::from_str(&json) {
                items.push(item);
            }
        }
        Ok(items)
    }

    pub fn load_work_item_statuses(&self) -> Result<HashMap<String, Vec<WorkItemStatus>>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT project_path, data FROM work_item_statuses")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (project, json) = row?;
            if let Ok(statuses) = serde_json::from_str(&json) {
                map.insert(project, statuses);
            }
        }
        Ok(map)
    }

    /// Load a single issue by project path + iid (for detail views).
    #[allow(dead_code)] // Will be used when sync_issue_list_for_detail is removed
    pub fn load_issue_by_key(&self, project: &str, iid: u64) -> Result<Option<TrackedIssue>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT data FROM issues WHERE project_path = ?1 AND iid = ?2")?;
        let mut rows = stmt.query_map(params![project, iid], |row| row.get::<_, String>(0))?;
        if let Some(row) = rows.next() {
            let json = row?;
            Ok(serde_json::from_str(&json).ok())
        } else {
            Ok(None)
        }
    }

    /// Query closed issues updated after a given date, excluding those in a
    /// specific iteration. Used for shadow work detection.
    pub fn query_shadow_work(
        &self,
        updated_after: &str,
        exclude_iteration_id: Option<&str>,
    ) -> Result<Vec<TrackedIssue>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT data FROM issues WHERE state = 'closed' AND updated_at >= ?1",
        )?;
        let rows = stmt.query_map(params![updated_after], |row| row.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            let json = row?;
            if let Ok(item) = serde_json::from_str::<TrackedIssue>(&json) {
                // Exclude issues that belong to the current iteration
                let dominated = exclude_iteration_id.is_some_and(|iter_id| {
                    item.issue
                        .iteration
                        .as_ref()
                        .is_some_and(|i| i.id == iter_id)
                });
                if !dominated {
                    items.push(item);
                }
            }
        }
        Ok(items)
    }

    // ── Key-value store ─────────────────────────────────────────────

    pub fn set_kv<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
            params![key, json],
        )?;
        Ok(())
    }

    pub fn get_kv<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT value FROM kv WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        if let Some(row) = rows.next() {
            let json = row?;
            Ok(serde_json::from_str(&json).ok())
        } else {
            Ok(None)
        }
    }

    // ── JSON cache migration ────────────────────────────────────────

    fn migrate_from_json(&self, json_path: &std::path::Path) -> Result<()> {
        let data = std::fs::read_to_string(json_path)?;
        let cached: cache::CacheData = serde_json::from_str(&data)?;

        self.upsert_issues(&cached.issues)?;
        self.upsert_mrs(&cached.mrs)?;
        self.upsert_labels(&cached.labels)?;
        self.upsert_iterations(&cached.iterations)?;

        for (project, statuses) in &cached.work_item_statuses {
            self.set_work_item_statuses(project, statuses)?;
        }

        // Migrate shadow work issues into the issues table
        self.upsert_issues(&cached.shadow_work_issues)?;

        // Migrate key-value data
        self.set_kv("label_usage", &cached.label_usage)?;
        self.set_kv("scope_creep_dates", &cached.scope_creep_dates)?;

        if let Some(vs) = &cached.issue_view_state {
            self.set_kv("issue_view_state", vs)?;
        }
        if let Some(vs) = &cached.mr_view_state {
            self.set_kv("mr_view_state", vs)?;
        }

        // Rename old cache file
        let backup = json_path.with_extension("json.bak");
        std::fs::rename(json_path, backup)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitlab::types::{Issue, MergeRequest};
    use chrono::Utc;

    fn make_issue(id: u64, state: &str) -> TrackedIssue {
        TrackedIssue {
            project_path: "test/project".to_string(),
            issue: Issue {
                id,
                iid: id,
                title: format!("Issue {id}"),
                state: state.to_string(),
                author: None,
                assignees: vec![],
                labels: vec![],
                milestone: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                web_url: String::new(),
                description: None,
                user_notes_count: 0,
                references: None,
                custom_status: None,
                iteration: None,
                weight: None,
            },
        }
    }

    fn make_mr(id: u64, state: &str) -> TrackedMergeRequest {
        TrackedMergeRequest {
            project_path: "test/project".to_string(),
            mr: MergeRequest {
                id,
                iid: id,
                title: format!("MR {id}"),
                state: state.to_string(),
                author: None,
                assignees: vec![],
                reviewers: vec![],
                labels: vec![],
                milestone: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                web_url: String::new(),
                description: None,
                draft: false,
                work_in_progress: false,
                merge_status: None,
                source_branch: "feature".to_string(),
                target_branch: "main".to_string(),
                head_pipeline: None,
                user_notes_count: 0,
                references: None,
                approved_by: vec![],
                diff_additions: None,
                diff_deletions: None,
                diff_file_count: None,
                approved: None,
                unresolved_threads: None,
            },
        }
    }

    #[test]
    fn test_issue_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let issues = vec![make_issue(1, "opened"), make_issue(2, "closed")];
        db.upsert_issues(&issues).unwrap();

        let all = db.load_issues(None).unwrap();
        assert_eq!(all.len(), 2);

        let opened = db.load_issues(Some("opened")).unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].issue.id, 1);

        let closed = db.load_issues(Some("closed")).unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].issue.id, 2);
    }

    #[test]
    fn test_mr_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let mrs = vec![make_mr(1, "opened"), make_mr(2, "merged")];
        db.upsert_mrs(&mrs).unwrap();

        let all = db.load_mrs(None).unwrap();
        assert_eq!(all.len(), 2);

        let opened = db.load_mrs(Some("opened")).unwrap();
        assert_eq!(opened.len(), 1);
    }

    #[test]
    fn test_upsert_replaces() {
        let db = Db::open_in_memory().unwrap();
        let mut issue = make_issue(1, "opened");
        db.upsert_issues(&[issue.clone()]).unwrap();

        issue.issue.state = "closed".to_string();
        db.upsert_issues(&[issue]).unwrap();

        let all = db.load_issues(None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].issue.state, "closed");
    }

    #[test]
    fn test_load_issue_by_key() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_issues(&[make_issue(1, "opened")]).unwrap();

        let found = db.load_issue_by_key("test/project", 1).unwrap();
        assert!(found.is_some());

        let missing = db.load_issue_by_key("test/project", 999).unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_kv_round_trip() {
        let db = Db::open_in_memory().unwrap();

        let usage: HashMap<String, u32> = [("bug".to_string(), 5), ("feature".to_string(), 3)]
            .into_iter()
            .collect();
        db.set_kv("label_usage", &usage).unwrap();

        let loaded: Option<HashMap<String, u32>> = db.get_kv("label_usage").unwrap();
        assert_eq!(loaded.unwrap(), usage);
    }

    #[test]
    fn test_kv_missing_key() {
        let db = Db::open_in_memory().unwrap();
        let val: Option<String> = db.get_kv("nonexistent").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_work_item_statuses_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let statuses = vec![WorkItemStatus {
            id: "1".to_string(),
            name: "Done".to_string(),
            icon_name: None,
            color: None,
            position: Some(1),
            category: Some("done".to_string()),
        }];
        db.set_work_item_statuses("test/project", &statuses)
            .unwrap();

        let loaded = db.load_work_item_statuses().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["test/project"][0].name, "Done");
    }

    #[test]
    fn test_labels_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let labels = vec![ProjectLabel {
            id: 1,
            name: "bug".to_string(),
            color: Some("#ff0000".to_string()),
        }];
        db.upsert_labels(&labels).unwrap();

        let loaded = db.load_labels().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "bug");
    }

    #[test]
    fn test_shadow_work_query() {
        let db = Db::open_in_memory().unwrap();
        let mut closed = make_issue(1, "closed");
        closed.issue.updated_at = chrono::DateTime::parse_from_rfc3339("2026-04-10T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let mut old_closed = make_issue(2, "closed");
        old_closed.issue.updated_at = chrono::DateTime::parse_from_rfc3339("2026-03-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        db.upsert_issues(&[closed, old_closed]).unwrap();

        let shadow = db
            .query_shadow_work("2026-04-01T00:00:00+00:00", None)
            .unwrap();
        assert_eq!(shadow.len(), 1);
        assert_eq!(shadow[0].issue.id, 1);
    }
}
