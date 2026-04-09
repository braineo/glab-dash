use anyhow::{Context, Result};
use reqwest::header::{self, HeaderMap, HeaderValue};

use crate::config::Config;
use crate::gitlab::types::*;

#[derive(Clone)]
pub struct GitLabClient {
    client: reqwest::Client,
    base_url: String,
    config: Config,
}

impl GitLabClient {
    pub fn new(config: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "PRIVATE-TOKEN",
            HeaderValue::from_str(&config.token).context("Invalid token")?,
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        let base_url = config.gitlab_url.trim_end_matches('/').to_string();

        Ok(Self {
            client,
            base_url,
            config: config.clone(),
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v4{}", self.base_url, path)
    }

    fn encode_project(project: &str) -> String {
        project.replace('/', "%2F")
    }

    // ── Issues ──

    pub async fn list_project_issues(
        &self,
        project: &str,
        state: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<Issue>> {
        let url = self.api_url(&format!(
            "/projects/{}/issues",
            Self::encode_project(project)
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("state", state),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
                ("with_labels_details", "false"),
            ])
            .send()
            .await
            .context("Failed to fetch project issues")?;
        Self::handle_response(resp).await
    }

    pub async fn list_assigned_issues(
        &self,
        username: &str,
        state: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<Issue>> {
        let url = self.api_url("/issues");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("assignee_username", username),
                ("state", state),
                ("scope", "all"),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ])
            .send()
            .await
            .context("Failed to fetch assigned issues")?;
        Self::handle_response(resp).await
    }

    pub async fn get_issue(&self, project: &str, iid: u64) -> Result<Issue> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    pub async fn close_issue(&self, project: &str, iid: u64) -> Result<Issue> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .put(&url)
            .json(&serde_json::json!({"state_event": "close"}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn reopen_issue(&self, project: &str, iid: u64) -> Result<Issue> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .put(&url)
            .json(&serde_json::json!({"state_event": "reopen"}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn update_issue_labels(
        &self,
        project: &str,
        iid: u64,
        labels: &[String],
    ) -> Result<Issue> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .put(&url)
            .json(&serde_json::json!({"labels": labels.join(",")}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn update_issue_assignees(
        &self,
        project: &str,
        iid: u64,
        assignee_ids: &[u64],
    ) -> Result<Issue> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .put(&url)
            .json(&serde_json::json!({"assignee_ids": assignee_ids}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn create_issue_note(&self, project: &str, iid: u64, body: &str) -> Result<Note> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn list_issue_notes(&self, project: &str, iid: u64) -> Result<Vec<Note>> {
        let url = self.api_url(&format!(
            "/projects/{}/issues/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("sort", "asc"), ("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    // ── Merge Requests ──

    pub async fn list_project_mrs(
        &self,
        project: &str,
        state: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<MergeRequest>> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests",
            Self::encode_project(project)
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("state", state),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ])
            .send()
            .await
            .context("Failed to fetch project MRs")?;
        Self::handle_response(resp).await
    }

    pub async fn list_assigned_mrs(
        &self,
        username: &str,
        state: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<MergeRequest>> {
        let url = self.api_url("/merge_requests");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("assignee_username", username),
                ("state", state),
                ("scope", "all"),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ])
            .send()
            .await
            .context("Failed to fetch assigned MRs")?;
        Self::handle_response(resp).await
    }

    pub async fn list_reviewer_mrs(
        &self,
        username: &str,
        state: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<MergeRequest>> {
        let url = self.api_url("/merge_requests");
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("reviewer_username", username),
                ("state", state),
                ("scope", "all"),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ])
            .send()
            .await
            .context("Failed to fetch reviewer MRs")?;
        Self::handle_response(resp).await
    }

    pub async fn get_mr(&self, project: &str, iid: u64) -> Result<MergeRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    pub async fn approve_mr(&self, project: &str, iid: u64) -> Result<()> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/approve",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.post(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Approve failed ({status}): {body}");
        }
        Ok(())
    }

    pub async fn merge_mr(&self, project: &str, iid: u64) -> Result<MergeRequest> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/merge",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .put(&url)
            .json(&serde_json::json!({"should_remove_source_branch": true}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn create_mr_note(&self, project: &str, iid: u64, body: &str) -> Result<Note> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn list_mr_notes(&self, project: &str, iid: u64) -> Result<Vec<Note>> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/notes",
            Self::encode_project(project),
            iid
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("sort", "asc"), ("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    pub async fn get_mr_approvals(&self, project: &str, iid: u64) -> Result<MergeRequestApprovals> {
        let url = self.api_url(&format!(
            "/projects/{}/merge_requests/{}/approvals",
            Self::encode_project(project),
            iid
        ));
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    // ── Labels ──

    pub async fn list_project_labels(&self, project: &str) -> Result<Vec<ProjectLabel>> {
        let url = self.api_url(&format!(
            "/projects/{}/labels",
            Self::encode_project(project)
        ));
        let resp = self
            .client
            .get(&url)
            .query(&[("per_page", "100")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    // ── Authenticated User ──

    pub async fn get_authenticated_user(&self) -> Result<serde_json::Value> {
        let url = self.api_url("/user");
        let resp = self.client.get(&url).send().await?;
        Self::handle_response(resp).await
    }

    // ── Members / Users ──

    pub async fn search_users(&self, query: &str) -> Result<Vec<User>> {
        let url = self.api_url("/users");
        let resp = self
            .client
            .get(&url)
            .query(&[("search", query), ("per_page", "20")])
            .send()
            .await?;
        Self::handle_response(resp).await
    }

    // ── Fetch all data for dashboard ──

    pub async fn fetch_tracking_issues(&self, state: &str) -> Result<Vec<TrackedIssue>> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let issues = self
                .list_project_issues(&self.config.tracking_project, state, page, 100)
                .await?;
            let done = issues.len() < 100;
            for issue in issues {
                all.push(TrackedIssue {
                    project_path: self.config.tracking_project.clone(),
                    source: ItemSource::Tracking,
                    issue,
                });
            }
            if done {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    pub async fn fetch_external_issues(
        &self,
        members: &[String],
        state: &str,
    ) -> Result<Vec<TrackedIssue>> {
        let mut all = Vec::new();
        for member in members {
            let mut page = 1u32;
            loop {
                let issues = self.list_assigned_issues(member, state, page, 100).await?;
                let done = issues.len() < 100;
                for issue in issues {
                    // Skip issues from the tracking project
                    let project_path = issue
                        .references
                        .as_ref()
                        .map(|r| extract_project_from_ref(&r.full_ref))
                        .unwrap_or_default();
                    if project_path == self.config.tracking_project {
                        continue;
                    }
                    // Deduplicate by id
                    if all.iter().any(|t: &TrackedIssue| t.issue.id == issue.id) {
                        continue;
                    }
                    let source = ItemSource::External(project_path.clone());
                    all.push(TrackedIssue {
                        issue,
                        source,
                        project_path,
                    });
                }
                if done {
                    break;
                }
                page += 1;
            }
        }
        Ok(all)
    }

    pub async fn fetch_tracking_mrs(&self, state: &str) -> Result<Vec<TrackedMergeRequest>> {
        let mut all = Vec::new();
        let mut page = 1u32;
        loop {
            let mrs = self
                .list_project_mrs(&self.config.tracking_project, state, page, 100)
                .await?;
            let done = mrs.len() < 100;
            for mr in mrs {
                all.push(TrackedMergeRequest {
                    project_path: self.config.tracking_project.clone(),
                    source: ItemSource::Tracking,
                    mr,
                });
            }
            if done {
                break;
            }
            page += 1;
        }
        Ok(all)
    }

    pub async fn fetch_external_mrs(
        &self,
        members: &[String],
        state: &str,
    ) -> Result<Vec<TrackedMergeRequest>> {
        let mut all = Vec::new();
        for member in members {
            // Fetch both assigned and reviewer MRs
            for is_reviewer in [false, true] {
                let mut page = 1u32;
                loop {
                    let mrs = if is_reviewer {
                        self.list_reviewer_mrs(member, state, page, 100).await?
                    } else {
                        self.list_assigned_mrs(member, state, page, 100).await?
                    };
                    let done = mrs.len() < 100;
                    for mr in mrs {
                        let project_path = mr
                            .references
                            .as_ref()
                            .map(|r| extract_project_from_ref(&r.full_ref))
                            .unwrap_or_default();
                        if project_path == self.config.tracking_project {
                            continue;
                        }
                        if all.iter().any(|t: &TrackedMergeRequest| t.mr.id == mr.id) {
                            continue;
                        }
                        let source = ItemSource::External(project_path.clone());
                        all.push(TrackedMergeRequest {
                            mr,
                            source,
                            project_path,
                        });
                    }
                    if done {
                        break;
                    }
                    page += 1;
                }
            }
        }
        Ok(all)
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        if !status.is_success() {
            let url = resp.url().to_string();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{status} from {url}: {body}");
        }
        resp.json::<T>()
            .await
            .context("Failed to parse GitLab response")
    }
}

/// Extract project path from a full reference like "myorg/myrepo#123" or "myorg/myrepo!45"
fn extract_project_from_ref(full_ref: &str) -> String {
    // Full refs look like "group/project#123" or "group/subgroup/project!45"
    if let Some(idx) = full_ref.rfind(['#', '!']) {
        full_ref[..idx].to_string()
    } else {
        full_ref.to_string()
    }
}
