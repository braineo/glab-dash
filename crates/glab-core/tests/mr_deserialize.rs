//! `MergeRequest` deserializes a real GraphQL response.
//!
//! The fixture is an actual `MrFields` payload from GitLab with identifying
//! content replaced. It guards the shapes that are easy to get wrong: GIDs
//! where a `u64` is wanted, a string `iid`, connections that need unwrapping,
//! and the `SCREAMING_CASE` pipeline status the UI matches lowercase.

use glab_core::domain::MergeRequest;

const RESPONSE: &str = include_str!("mr_graphql_response.json");

#[test]
fn deserializes_graphql_response() {
    let mr: MergeRequest = serde_json::from_str(RESPONSE).expect("MrFields payload");

    // `id: ID!` and `iid: String!` are kept as the strings GraphQL sends.
    assert_eq!(mr.id, "gid://gitlab/MergeRequest/80652");
    assert_eq!(mr.iid, "146");
    assert_eq!(mr.author.as_ref().unwrap().id, "gid://gitlab/User/1");

    // Connections unwrap to plain vectors.
    assert!(mr.assignees.is_empty());
    assert_eq!(mr.reviewers.len(), 1);
    assert_eq!(mr.reviewers[0].username, "user2");
    assert_eq!(mr.labels, ["workflow::doing"]);

    // Nested selections stay nested; derived values are computed.
    let stats = mr.diff_stats().expect("diffStatsSummary");
    assert_eq!(
        (stats.additions, stats.deletions, stats.file_count),
        (24, 0, 1)
    );
    assert_eq!(mr.unresolved_threads(), 0);
    assert_eq!(mr.notes_count(), 2);

    // GitLab reports SUCCESS; the UI matches and sorts on lowercase.
    assert_eq!(mr.pipeline_status(), Some("success"));

    assert_eq!(mr.reference.as_deref(), Some("group/proj!146"));
    assert_eq!(mr.state, "opened");
    assert!(!mr.draft);
}

/// The type is also its own storage format: what `glab-db` writes must read
/// back identically, even though serializing emits plain arrays where GraphQL
/// sends `{ "nodes": [...] }`.
#[test]
fn round_trips_through_its_own_serialization() {
    let mr: MergeRequest = serde_json::from_str(RESPONSE).unwrap();
    let stored = serde_json::to_string(&mr).unwrap();
    let back: MergeRequest = serde_json::from_str(&stored).expect("cached row");

    assert_eq!(back.id, mr.id);
    assert_eq!(back.iid, mr.iid);
    assert_eq!(back.reviewers.len(), mr.reviewers.len());
    assert_eq!(back.reviewers[0].username, mr.reviewers[0].username);
    assert_eq!(back.labels, mr.labels);
    assert_eq!(back.pipeline_status(), mr.pipeline_status());
    assert_eq!(back.unresolved_threads(), mr.unresolved_threads());
}
