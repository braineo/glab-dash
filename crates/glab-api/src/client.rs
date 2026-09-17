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

use glab_core::domain::ItemKind;
use urlencoding::encode;

use crate::wire::{GqlPage, GqlResponse, Paged};

/// An authenticated connection to one GitLab instance.
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
            // Without this a stalled connection never returns, and the caller's
            // fetch task never finishes — which wedges every later refresh.
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            http,
            base_url: gitlab_url.trim_end_matches('/').to_string(),
        })
    }

    pub(crate) fn rest(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/api/v4{path}", self.base_url))
    }

    /// Post one GraphQL document and return its response body, retried when the
    /// request fails. Reads go through here; a mutation uses `graphql_once`.
    pub(crate) async fn graphql(
        &self,
        op: &'static str,
        query: &str,
        variables: Value,
    ) -> Result<Value> {
        with_retry(op, || self.graphql_once(op, query, variables.clone())).await
    }

    /// Post one GraphQL document, once. `op` names the operation for the trace
    /// log only; a top-level `errors` array fails the call, since GitLab reports
    /// a malformed or unauthorized query there with a 200 status.
    pub(crate) async fn graphql_once(
        &self,
        op: &'static str,
        query: &str,
        variables: Value,
    ) -> Result<Value> {
        let body = serde_json::json!({ "query": query, "variables": variables });
        let started = std::time::Instant::now();
        tracing::debug!(op, vars = %variables, "graphql →");

        let result = Self::send::<Value>(
            self.http
                .post(format!("{}/api/graphql", self.base_url))
                .json(&body),
        )
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
            return Err(GqlError(join_messages(errors, Some("message"))).into());
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

    /// Send a REST read, retried when the request fails. A write uses `send`:
    /// retrying it can write twice.
    pub(crate) async fn fetch<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T> {
        with_retry("rest", || async {
            let request = request
                .try_clone()
                .context("request body cannot be replayed")?;
            Self::send(request).await
        })
        .await
    }

    pub(crate) async fn send<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T> {
        let resp = request.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let url = resp.url().to_string();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(%status, %url, %body, "http ✗");
            anyhow::bail!("{status}: {body}");
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
pub(crate) fn get_mutation_payload<'a>(json: &'a Value, mutation: &str) -> Result<&'a Value> {
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

/// The REST route for `tail` under the item `iid` in `project`.  Both kinds
/// hang their sub-collections off the same shape, under the segment naming the
/// kind — which is the whole of what a REST route needs to know about it.
pub(crate) fn item_path(kind: ItemKind, project: &str, iid: &str, tail: &str) -> String {
    let segment = match kind {
        ItemKind::Issue => "issues",
        ItemKind::MergeRequest => "merge_requests",
    };
    format!("/projects/{}/{segment}/{iid}/{tail}", encode(project))
}

/// The user selection every other fragment spreads in turn.
const USER_FIELDS: &str = r"
    fragment UserFields on User {
        id username name webUrl
    }
";

/// Page size for every paginated list query.
pub(crate) const PAGE_SIZE: u32 = 100;

/// How many times a read is asked for before it fails.
const ATTEMPTS: u32 = 3;

/// An error GitLab answered with in the response body rather than in the
/// transport: a malformed query, a field the token may not read. Permanent, so
/// [`with_retry`] hands it back instead of asking again.
#[derive(Debug)]
struct GqlError(String);

impl std::fmt::Display for GqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GraphQL: {}", self.0)
    }
}

impl std::error::Error for GqlError {}

/// Run `attempt` again when it fails, backing off between tries.
///
/// A read is idempotent and a fetch cycle is expensive: one blip would
/// otherwise discard every page already walked and cancel every sibling walk.
/// A [`GqlError`] is permanent and is handed back on the first answer.
///
/// ponytail: retries any transport failure, a permanent 404 included. Classify
/// the status if a wrong path starts costing a second and a half to fail.
async fn with_retry<T, Fut: Future<Output = Result<T>>>(
    op: &str,
    attempt: impl Fn() -> Fut,
) -> Result<T> {
    let mut delay = std::time::Duration::from_millis(500);
    let mut tries = 1;
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(e) if tries >= ATTEMPTS || e.is::<GqlError>() => return Err(e),
            Err(e) => {
                tracing::warn!(op, tries, error = ?e, ?delay, "retrying");
                tokio::time::sleep(delay).await;
                delay *= 2;
                tries += 1;
            }
        }
    }
}

/// Join concurrent walks, restoring the order they were spawned in, and flatten
/// them with duplicates dropped: the same issue or merge request is reachable
/// from more than one namespace, project or member. The first walk to hold it
/// wins, so the result does not depend on which request came back first.
///
/// The first failed walk returns, dropping the set — which aborts the rest.
pub(crate) async fn join_walks<T: Send + 'static>(
    mut set: tokio::task::JoinSet<(usize, Result<Vec<T>>)>,
    id: impl Fn(&T) -> &str,
) -> Result<Vec<T>> {
    let mut walks = Vec::with_capacity(set.len());
    while let Some(joined) = set.join_next().await {
        let (idx, items) = joined.context("list task failed")?;
        walks.push((idx, items?));
    }
    walks.sort_by_key(|(idx, _)| *idx);

    let mut seen = std::collections::HashSet::new();
    Ok(walks
        .into_iter()
        .flat_map(|(_, items)| items)
        .filter(|item| seen.insert(id(item).to_string()))
        .collect())
}

/// `doc` followed by the fragments it spreads: `fields` itself, and the
/// `UserFields` every one of them spreads in turn.
pub(crate) fn document(doc: &str, fields: &str) -> String {
    format!("{doc}{fields}{USER_FIELDS}")
}
