mod issue;
mod item;
mod merge_request;
mod note;

use serde::{Deserialize, Serialize};

pub use issue::{Issue, Iteration, Milestone, StatusCategory, StatusValue, WorkItemStatus};
pub use item::{Item, ItemKind, ItemRef, RelatedItem, Relation};
pub use merge_request::{DiffStats, MergeRequest, PipelineRef};
pub use note::{Discussion, Note};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// REST returns a number, GraphQL `gid://gitlab/User/{id}`.
    #[serde(deserialize_with = "crate::de::user_id")]
    pub id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLabel {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// Strip the `#123` / `!45` suffix off a full reference, leaving the project
/// path: `group/project#123`, `group/sub/project!45`.
fn project_from_reference(full_ref: &str) -> &str {
    match full_ref.rfind(['#', '!']) {
        Some(idx) => &full_ref[..idx],
        None => full_ref,
    }
}

#[cfg(test)]
mod tests {
    use super::project_from_reference;

    #[test]
    fn strips_the_iid_suffix_from_a_full_reference() {
        assert_eq!(project_from_reference("group/project#123"), "group/project");
        assert_eq!(
            project_from_reference("group/sub/project!45"),
            "group/sub/project"
        );
    }

    #[test]
    fn a_reference_without_an_iid_is_already_a_project_path() {
        assert_eq!(project_from_reference("group/project"), "group/project");
    }
}
