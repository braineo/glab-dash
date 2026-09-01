//! `Issue` deserializes a real GraphQL response.
//!
//! The fixture is an actual `IssueFields` payload from the root `issues` query
//! with identifying content replaced. It guards what the removed
//! `GqlRootIssue` conversion used to do by hand: a GID reduced to its numeric
//! tail, an `iid` left as the string GraphQL sends, connections unwrapped, and
//! the nullable iteration title that auto-generated iterations have.

use glab_core::domain::Issue;

const RESPONSE: &str = include_str!("issue_graphql_response.json");

#[test]
fn deserializes_graphql_response() {
    let issue: Issue = serde_json::from_str(RESPONSE).expect("IssueFields payload");

    // `id: ID!` and `iid: String!` stay strings. The id is normalized to the
    // `WorkItem` prefix even though the root query reports `Issue`.
    assert_eq!(issue.id, "gid://gitlab/WorkItem/42950");
    assert_eq!(issue.iid, "5998");
    assert_eq!(issue.author.as_ref().unwrap().id, "gid://gitlab/User/1");

    // Connections unwrap to plain vectors.
    assert_eq!(issue.assignees.len(), 1);
    assert_eq!(issue.assignees[0].username, "user2");
    assert_eq!(issue.labels, ["Is::Bug"]);

    // Nested status, reached through the accessors the UI uses.
    assert_eq!(issue.status_name(), Some("Backlog"));
    assert_eq!(issue.status_category(), Some("to_do"));

    // Auto-generated iterations have no title.
    let iter = issue.iteration.as_ref().expect("iteration");
    assert_eq!(iter.id, "gid://gitlab/Iteration/557");
    assert_eq!(iter.title, None);
    assert_eq!(iter.start_date.as_deref(), Some("2026-08-24"));

    // `userNotesCount` is selected now; it used to be hardcoded to 0.
    assert_eq!(issue.user_notes_count, 3);

    assert_eq!(issue.reference, "group/proj#5998");
    assert_eq!(issue.project_path(), "group/proj");
    assert_eq!(issue.state, "opened");
    assert!(issue.closed_at.is_none());
}

/// The id must be identical whichever query produced the issue: the two result
/// sets are deduplicated against each other, and `namespace.workItems` reports
/// `gid://gitlab/WorkItem/42950` where the root `issues` query reports
/// `gid://gitlab/Issue/42950`. Passing the raw GID through would make those
/// look like two different issues.
#[test]
fn both_queries_yield_the_same_id() {
    let from_issues: Issue = serde_json::from_str(RESPONSE).unwrap();
    let as_work_item = RESPONSE.replace("gid://gitlab/Issue/42950", "gid://gitlab/WorkItem/42950");
    let from_work_items: Issue = serde_json::from_str(&as_work_item).unwrap();

    assert_eq!(from_issues.id, from_work_items.id);
    assert_eq!(
        from_issues.id, "gid://gitlab/WorkItem/42950",
        "mutations address an issue as a work item, so that is the form kept"
    );
}

/// The type is also its own storage format: what `glab-db` writes must read
/// back identically, even though serializing emits plain arrays where GraphQL
/// sends `{ "nodes": [...] }` and a bare number where it sends a GID.
#[test]
fn round_trips_through_its_own_serialization() {
    let issue: Issue = serde_json::from_str(RESPONSE).unwrap();
    let stored = serde_json::to_string(&issue).unwrap();
    let back: Issue = serde_json::from_str(&stored).expect("cached row");

    assert_eq!(back.id, issue.id);
    assert_eq!(back.iid, issue.iid);
    assert_eq!(back.assignees.len(), issue.assignees.len());
    assert_eq!(back.labels, issue.labels);
    assert_eq!(back.status_name(), issue.status_name());
    assert_eq!(back.user_notes_count, issue.user_notes_count);
    assert_eq!(
        back.iteration.as_ref().map(|i| &i.id),
        issue.iteration.as_ref().map(|i| &i.id)
    );
}
