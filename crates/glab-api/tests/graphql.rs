//! Transport-level behaviour every GraphQL caller depends on: following a
//! connection's cursor, and refusing a response GitLab answered 200 with but
//! filled with errors.

use glab_api::{GitLabClient, IssueState, MrState};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Answers each successive POST with the next scripted body, so one mock can
/// script a paginated walk.
struct Script(std::sync::Mutex<std::vec::IntoIter<Value>>);

impl Script {
    fn new(pages: Vec<Value>) -> Self {
        Script(std::sync::Mutex::new(pages.into_iter()))
    }
}

impl Respond for Script {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        let body = self
            .0
            .lock()
            .unwrap()
            .next()
            .expect("more GraphQL requests than scripted pages");
        ResponseTemplate::new(200).set_body_json(body)
    }
}

/// A work item node as `WorkItemFields` selects it, with no widgets set.
fn work_item(iid: &str) -> Value {
    json!({
        "id": format!("gid://gitlab/WorkItem/{iid}"),
        "iid": iid,
        "title": format!("issue {iid}"),
        "state": "OPEN",
        "author": null,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-02T00:00:00Z",
        "closedAt": null,
        "webUrl": format!("https://gitlab.test/g/p/-/issues/{iid}"),
        "reference": format!("g/p#{iid}"),
        "widgets": [],
    })
}

fn work_item_page(nodes: &[Value], next: Option<&str>) -> Value {
    json!({
        "data": { "namespace": { "workItems": {
            "nodes": nodes,
            "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
        }}}
    })
}

async fn mock_graphql(pages: Vec<Value>) -> (MockServer, GitLabClient) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/graphql"))
        .respond_with(Script::new(pages))
        .mount(&server)
        .await;
    let client = GitLabClient::new(&server.uri(), "token").unwrap();
    (server, client)
}

#[tokio::test]
async fn follows_the_cursor_to_the_last_page() {
    let (server, client) = mock_graphql(vec![
        work_item_page(&[work_item("1"), work_item("2")], Some("cursor-1")),
        work_item_page(&[work_item("3")], None),
    ])
    .await;

    let issues = client
        .list_namespace_issues(&["g/p".to_string()], Some(IssueState::Opened), None)
        .await
        .unwrap();

    assert_eq!(
        issues.iter().map(|i| i.iid.as_str()).collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    // Two pages, one request each — the walk stops when hasNextPage is false.
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn resumes_from_the_cursor_the_previous_page_returned() {
    let (server, client) = mock_graphql(vec![
        work_item_page(&[work_item("1")], Some("cursor-1")),
        work_item_page(&[], None),
    ])
    .await;

    client
        .list_namespace_issues(&["g/p".to_string()], None, None)
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    let vars = |r: &Request| r.body_json::<Value>().unwrap()["variables"].clone();
    assert_eq!(vars(&requests[0])["after"], Value::Null);
    assert_eq!(vars(&requests[1])["after"], "cursor-1");
}

#[tokio::test]
async fn stops_when_the_connection_is_absent() {
    // An unknown project answers `project: null` rather than an error.
    let (_server, client) = mock_graphql(vec![json!({ "data": { "project": null } })]).await;

    let mrs = client
        .list_project_mrs(&["g/gone".to_string()], Some(MrState::Opened), None)
        .await
        .unwrap();

    assert!(mrs.is_empty());
}

#[tokio::test]
async fn deduplicates_across_namespaces() {
    let (_server, client) = mock_graphql(vec![
        work_item_page(&[work_item("1"), work_item("2")], None),
        work_item_page(&[work_item("2"), work_item("3")], None),
    ])
    .await;

    let issues = client
        .list_namespace_issues(&["g/a".to_string(), "g/b".to_string()], None, None)
        .await
        .unwrap();

    assert_eq!(
        issues.iter().map(|i| i.iid.as_str()).collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
}

#[tokio::test]
async fn top_level_errors_fail_the_call() {
    // GitLab reports an unauthorized or malformed query with a 200 status.
    let (_server, client) = mock_graphql(vec![json!({
        "data": null,
        "errors": [{ "message": "Field 'weight' doesn't exist" }],
    })])
    .await;

    let err = client
        .list_namespace_issues(&["g/p".to_string()], None, None)
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("Field 'weight' doesn't exist"),
        "{err}"
    );
}

#[tokio::test]
async fn mutation_errors_fail_the_call() {
    // A rejected write also answers 200, with its reason in the payload.
    let (_server, client) = mock_graphql(vec![json!({
        "data": { "workItemUpdate": {
            "errors": ["Status is not available", "Iteration is not in the cadence"],
            "workItem": null,
        }}
    })])
    .await;

    let err = client
        .update_issue_status("gid://gitlab/WorkItem/1", "gid://gitlab/Status/9")
        .await
        .unwrap_err();

    assert_eq!(
        err.to_string(),
        "Status is not available, Iteration is not in the cadence"
    );
}
