use anyhow::Result;
use glab_core::domain::{ProjectLabel, User};
use reqwest::Method;
use urlencoding::encode;

use crate::client::GitLabClient;

impl GitLabClient {
    /// Labels inherited from ancestor groups are included.
    pub async fn list_project_labels(&self, project: &str) -> Result<Vec<ProjectLabel>> {
        let path = format!("/projects/{}/labels", encode(project));
        let request = self.rest(Method::GET, &path).query(&[("per_page", "100")]);
        Self::fetch(request).await
    }

    pub async fn get_authenticated_user(&self) -> Result<serde_json::Value> {
        Self::fetch(self.rest(Method::GET, "/user")).await
    }

    pub async fn search_users(&self, query: &str) -> Result<Vec<User>> {
        let request = self
            .rest(Method::GET, "/users")
            .query(&[("search", query), ("per_page", "20")]);
        Self::fetch(request).await
    }
}
