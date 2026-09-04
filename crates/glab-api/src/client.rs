//! The client itself: how a request is addressed, sent, logged and read back.
//!
//! Two transports live here. [`GitLabClient::rest`] builds a REST v4 URL and
//! hands back a `reqwest` builder the caller finishes; [`GitLabClient::graphql`]
//! posts one document and surfaces GitLab's top-level `errors` array as a
//! failure. Above them sit the two shapes every caller needs and nobody should
//! rewrite: [`GitLabClient::paginate`] follows a connection's cursor to the end,
//! and [`mutation_payload`] rejects a mutation that reported errors in its
//! payload rather than its HTTP status.

use anyhow::{Context, Result};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::wire::{GqlPage, GqlResponse, Paged};

/// An authenticated connection to one GitLab instance.
///
/// Cloning is cheap — `reqwest` shares one connection pool across clones — so a
/// caller spawning a task per request clones freely.
#[derive(Clone)]
pub struct GitLabClient {
    http: reqwest::Client,
    base_url: String,
}

impl GitLabClient {
    /// Build a client for the instance at `gitlab_url`, authenticating every
    /// request with `token` as a personal access token.
    pub fn new(gitlab_url: &str, token: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "PRIVATE-TOKEN",
            HeaderValue::from_str(token).context("Invalid token")?,
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            http,
            base_url: gitlab_url.trim_end_matches('/').to_string(),
        })
    }

    /// Start a REST v4 request against `path`, which is appended to the API
    /// root as written (`/projects/{id}/labels`).
    pub(crate) fn rest(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/api/v4{path}", self.base_url))
    }

    /// Post one GraphQL document and return its response body. `op` names the
    /// operation for the trace log only; a top-level `errors` array fails the
    /// call, since GitLab reports a malformed or unauthorized query there with
    /// a 200 status.
    pub(crate) async fn graphql(
        &self,
        op: &'static str,
        query: &str,
        variables: Value,
    ) -> Result<Value> {
        let body = serde_json::json!({ "query": query, "variables": variables });
        let started = std::time::Instant::now();
        tracing::debug!(op, vars = %variables, "graphql →");

        let result = async {
            let resp = self
                .http
                .post(format!("{}/api/graphql", self.base_url))
                .json(&body)
                .send()
                .await?;
            Self::read::<Value>(resp).await
        }
        .await;

        let elapsed_ms = started.elapsed().as_millis();
        let json = match result {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(op, error = ?e, elapsed_ms, "graphql ✗");
                return Err(e);
            }
        };
        tracing::debug!(op, elapsed_ms, "graphql ✓");

        if let Some(errors) = json.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
        {
            anyhow::bail!("GraphQL: {}", join_messages(errors, Some("message")));
        }
        Ok(json)
    }

    /// Follow a GraphQL connection's cursor to the end, collecting every node.
    ///
    /// `variables` is called once per page with the cursor to resume from —
    /// `None` for the first — and the response is read as `D`, which names the
    /// connection through [`Paged`]. A `D` whose path to the connection is
    /// absent (an unknown project, a user the token cannot see) ends the walk
    /// with what has been collected rather than failing.
    pub(crate) async fn paginate<T, D>(
        &self,
        op: &'static str,
        query: &str,
        mut variables: impl FnMut(Option<&str>) -> Value,
    ) -> Result<Vec<T>>
    where
        D: DeserializeOwned + Paged<T>,
    {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let json = self
                .graphql(op, query, variables(cursor.as_deref()))
                .await?;
            let resp: GqlResponse<D> = serde_json::from_value(json)
                .with_context(|| format!("failed to deserialize {op} response"))?;

            let Some(GqlPage { nodes, page_info }) = resp.data.page() else {
                break;
            };
            all.extend(nodes);

            if !page_info.has_next_page {
                break;
            }
            cursor = page_info.end_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(all)
    }

    /// Send `request` and deserialize a successful response as `T`. A non-2xx
    /// status fails with the status, the URL and the body GitLab returned.
    pub(crate) async fn send<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T> {
        Self::read(request.send().await?).await
    }

    /// Send `request` for its status alone, discarding the body. `action` names
    /// the operation in the error a non-2xx status raises.
    pub(crate) async fn send_ok(request: reqwest::RequestBuilder, action: &str) -> Result<()> {
        let resp = request.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{action} failed ({status}): {body}");
        }
        Ok(())
    }

    async fn read<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
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

/// The payload of mutation `mutation`, or the errors it reported.
///
/// A GitLab mutation answers 200 with its failures in the payload's `errors`
/// array, so a caller that only checks the HTTP status silently accepts a
/// rejected write.
pub(crate) fn mutation_payload<'a>(json: &'a Value, mutation: &str) -> Result<&'a Value> {
    let payload = json
        .pointer(&format!("/data/{mutation}"))
        .with_context(|| format!("missing {mutation} in mutation response"))?;

    if let Some(errors) = payload.get("errors").and_then(Value::as_array)
        && !errors.is_empty()
    {
        anyhow::bail!("{}", join_messages(errors, None));
    }
    Ok(payload)
}

/// Join the messages of an error array. A mutation's errors are bare strings,
/// so `field` is `None`; a top-level GraphQL error is an object carrying its
/// text under the named field.
fn join_messages(errors: &[Value], field: Option<&str>) -> String {
    errors
        .iter()
        .filter_map(|e| match field {
            Some(field) => e.get(field)?.as_str(),
            None => e.as_str(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}
