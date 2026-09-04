//! The small REST reads that describe the instance rather than the work on it:
//! a project's labels, the authenticated user, and user search.

use anyhow::Result;
use glab_core::domain::{ProjectLabel, User};
use reqwest::Method;
use urlencoding::encode;

use crate::client::GitLabClient;

impl GitLabClient {
    /// List the labels defined on `project`, including the ones it inherits
    /// from its ancestor groups.
    pub async fn list_project_labels(&self, project: &str) -> Result<Vec<ProjectLabel>> {
        let path = format!("/projects/{}/labels", encode(project));
        let request = self.rest(Method::GET, &path).query(&[("per_page", "100")]);
        Self::send(request).await
    }

    /// Read the user the token authenticates as.
    pub async fn get_authenticated_user(&self) -> Result<serde_json::Value> {
        Self::send(self.rest(Method::GET, "/user")).await
    }

    /// Search users by name, username or email.
    pub async fn search_users(&self, query: &str) -> Result<Vec<User>> {
        let request = self
            .rest(Method::GET, "/users")
            .query(&[("search", query), ("per_page", "20")]);
        Self::send(request).await
    }
}
