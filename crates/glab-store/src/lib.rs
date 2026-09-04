//! The local SQLite store: every issue, merge request, label, iteration and
//! work-item status glab-dash has fetched, plus the generic key-value slots
//! callers persist their own state in.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Serialize, de::DeserializeOwned};

use glab_core::domain::{Issue, Iteration, MergeRequest, ProjectLabel, WorkItemStatus};

const SCHEMA_VERSION: u32 = 2;

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

impl Db {
    /// Open (or create) the database at `~/.cache/glab-dash/data.db`.
    pub fn open() -> Result<Self> {
        let path = db_path().context("could not determine cache directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        let db = Self { conn };
        db.migrate()?;
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
                    id           TEXT PRIMARY KEY,
                    iid          TEXT NOT NULL,
                    project_path TEXT NOT NULL,
                    state        TEXT NOT NULL,
                    updated_at   TEXT NOT NULL,
                    data         TEXT NOT NULL,
                    UNIQUE(project_path, iid)
                );
                CREATE INDEX IF NOT EXISTS idx_issues_state ON issues(state);
                CREATE INDEX IF NOT EXISTS idx_issues_updated ON issues(updated_at);

                CREATE TABLE IF NOT EXISTS merge_requests (
                    id           TEXT PRIMARY KEY,
                    iid          TEXT NOT NULL,
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

        if version < 2 {
            self.conn.execute_batch(
                "ALTER TABLE issues ADD COLUMN closed_at TEXT;
                 CREATE INDEX IF NOT EXISTS idx_issues_closed ON issues(closed_at);",
            )?;
        }

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    // ── Batch upserts (after API fetch) ─────────────────────────────

    pub fn upsert_issues(&self, issues: &[Issue]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO issues (id, iid, project_path, state, updated_at, closed_at, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for item in issues {
                let data = serde_json::to_string(item).context("serialize Issue")?;
                stmt.execute(params![
                    item.id,
                    item.iid,
                    item.project_path(),
                    item.state,
                    item.updated_at.to_rfc3339(),
                    item.closed_at.map(|dt| dt.to_rfc3339()),
                    data,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_mrs(&self, mrs: &[MergeRequest]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO merge_requests (id, iid, project_path, state, updated_at, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for item in mrs {
                let data = serde_json::to_string(item).context("serialize MergeRequest")?;
                stmt.execute(params![
                    item.id,
                    item.iid,
                    item.project_path(),
                    item.state,
                    item.updated_at.to_rfc3339(),
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

    /// Load issues, optionally filtered to a single state.
    pub fn load_issues(&self, state: Option<&str>) -> Result<Vec<Issue>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT data FROM issues WHERE ?1 IS NULL OR state = ?1")?;
        let rows = stmt.query_map(params![state], |row| row.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            if let Ok(item) = serde_json::from_str(&row?) {
                items.push(item);
            }
        }
        Ok(items)
    }

    /// Load merge requests, optionally filtered to a single state.
    pub fn load_mrs(&self, state: Option<&str>) -> Result<Vec<MergeRequest>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT data FROM merge_requests WHERE ?1 IS NULL OR state = ?1")?;
        let rows = stmt.query_map(params![state], |row| row.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            if let Ok(item) = serde_json::from_str(&row?) {
                items.push(item);
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

    /// Query issues closed within a date range, excluding those in a
    /// specific iteration. Used for shadow work detection.
    pub fn query_shadow_work(
        &self,
        closed_after: &str,
        closed_before: &str,
        exclude_iteration_id: Option<&str>,
    ) -> Result<Vec<Issue>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT data FROM issues WHERE state = 'closed' AND closed_at >= ?1 AND closed_at <= ?2",
        )?;
        let rows = stmt.query_map(params![closed_after, closed_before], |row| {
            row.get::<_, String>(0)
        })?;
        let mut items = Vec::new();
        for row in rows {
            let json = row?;
            if let Ok(item) = serde_json::from_str::<Issue>(&json) {
                // Exclude issues that belong to the current iteration
                let dominated = exclude_iteration_id.is_some_and(|iter_id| {
                    item.iteration.as_ref().is_some_and(|i| i.id == iter_id)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use glab_core::domain::{Issue, MergeRequest};

    fn make_issue(id: u64, state: &str) -> Issue {
        Issue {
            id: id.to_string(),
            iid: id.to_string(),
            title: format!("Issue {id}"),
            state: state.to_string(),
            author: None,
            assignees: vec![],
            labels: vec![],
            milestone: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: if state == "closed" {
                Some(Utc::now())
            } else {
                None
            },
            web_url: String::new(),
            description: None,
            user_notes_count: 0,
            reference: format!("test/project#{id}"),
            status: None,
            iteration: None,
            weight: None,
        }
    }

    fn make_mr(id: u64, state: &str) -> MergeRequest {
        MergeRequest {
            id: id.to_string(),
            iid: id.to_string(),
            title: format!("MR {id}"),
            state: state.to_string(),
            author: None,
            assignees: vec![],
            reviewers: vec![],
            labels: vec![],
            milestone: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            web_url: None,
            description: None,
            draft: false,
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
            head_pipeline: None,
            user_notes_count: None,
            reference: format!("test/project!{id}"),
            approved_by: vec![],
            diff_stats_summary: None,
            approved: None,
            resolvable_discussions_count: None,
            resolved_discussions_count: None,
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
        assert_eq!(opened[0].id, "1");

        let closed = db.load_issues(Some("closed")).unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id, "2");
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

        issue.state = "closed".to_string();
        db.upsert_issues(&[issue]).unwrap();

        let all = db.load_issues(None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].state, "closed");
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
        closed.closed_at = Some(
            chrono::DateTime::parse_from_rfc3339("2026-04-10T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );

        let mut old_closed = make_issue(2, "closed");
        old_closed.closed_at = Some(
            chrono::DateTime::parse_from_rfc3339("2026-03-01T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );

        db.upsert_issues(&[closed, old_closed]).unwrap();

        let shadow = db
            .query_shadow_work(
                "2026-04-01T00:00:00+00:00",
                "2026-04-30T23:59:59+00:00",
                None,
            )
            .unwrap();
        assert_eq!(shadow.len(), 1);
        assert_eq!(shadow[0].id, "1");
    }
}
