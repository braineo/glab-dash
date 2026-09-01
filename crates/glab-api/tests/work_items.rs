//! Folding a work item's heterogeneous `widgets` array into the flat
//! `Issue` shape — the one place a GitLab response does not map field for
//! field onto the domain type.

use glab_api::GitLabClient;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn issue_from(work_item: Value) -> glab_core::domain::Issue {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "namespace": { "workItems": {
                "nodes": [work_item],
                "pageInfo": { "hasNextPage": false, "endCursor": null },
            }}}
        })))
        .mount(&server)
        .await;

    let client = GitLabClient::new(&server.uri(), "token").unwrap();
    client
        .list_namespace_issues(&["g/p".to_string()], None, None)
        .await
        .unwrap()
        .pop()
        .expect("one issue")
}

fn base(widgets: &Value) -> Value {
    json!({
        "id": "gid://gitlab/WorkItem/42",
        "iid": "42",
        "title": "Ship the thing",
        "state": "OPEN",
        "author": { "id": 7, "username": "ada", "name": "Ada", "webUrl": "https://gitlab.test/ada" },
        "createdAt": "2026-01-01T09:00:00Z",
        "updatedAt": "2026-01-02T09:00:00Z",
        "closedAt": null,
        "webUrl": "https://gitlab.test/g/p/-/issues/42",
        "reference": "g/p#42",
        "widgets": widgets,
    })
}

#[tokio::test]
async fn folds_every_widget_onto_the_issue() {
    let issue = issue_from(base(&json!([
        { "assignees": { "nodes": [
            { "id": 1, "username": "ada", "name": "Ada", "webUrl": "https://gitlab.test/ada" }
        ]}},
        { "labels": { "nodes": [{ "title": "bug" }, { "title": "team::core" }] } },
        { "milestone": { "title": "v2" } },
        { "status": { "name": "In progress", "category": "in_progress" } },
        { "description": "the body" },
        { "iteration": {
            "id": "gid://gitlab/Iteration/3", "title": "Sprint 3",
            "startDate": "2026-01-01", "dueDate": "2026-01-14", "state": "current"
        }},
        { "weight": 5 },
    ])))
    .await;

    assert_eq!(issue.assignees.len(), 1);
    assert_eq!(issue.labels, ["bug", "team::core"]);
    assert_eq!(issue.milestone.as_ref().unwrap().title, "v2");
    assert_eq!(issue.status_name(), Some("In progress"));
    assert_eq!(issue.description.as_deref(), Some("the body"));
    let iteration = issue.iteration.as_ref().unwrap();
    assert_eq!(iteration.title.as_deref(), Some("Sprint 3"));
    assert_eq!(issue.weight, Some(5));
}

#[tokio::test]
async fn an_issue_with_no_widgets_set_keeps_its_own_fields() {
    let issue = issue_from(base(&json!([]))).await;

    assert_eq!(issue.iid, "42");
    assert_eq!(issue.title, "Ship the thing");
    assert_eq!(issue.reference, "g/p#42");
    assert!(issue.assignees.is_empty());
    assert_eq!(issue.labels, [] as [std::string::String; 0]);
    assert_eq!(issue.weight, None);
}

#[tokio::test]
async fn work_item_state_is_normalized_to_the_rest_spelling() {
    // `workItems` answers OPEN/CLOSED where the root `issues` query and the
    // database both use opened/closed; the two result sets get merged.
    let opened = issue_from(base(&json!([]))).await;
    assert_eq!(opened.state, "opened");

    let mut closed_node = base(&json!([]));
    closed_node["state"] = json!("CLOSED");
    closed_node["closedAt"] = json!("2026-01-03T09:00:00Z");
    let closed = issue_from(closed_node).await;
    assert_eq!(closed.state, "closed");
    assert!(closed.closed_at.is_some());
}
