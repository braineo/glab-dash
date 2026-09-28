use crate::domain::{Issue, Item, MergeRequest, User};
use serde::{Deserialize, Serialize};

/// Teams may share `tracking_projects` and are then told apart by members.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub name: String,
    pub members: Vec<String>,
    pub tracking_projects: Vec<String>,
}

impl Team {
    /// `me` always counts as one of the team, including teams you are not on.
    pub fn owns_issue(&self, item: &Issue, me: &str) -> bool {
        self.owns(
            item.project_path(),
            item.assignees.is_empty(),
            self.any_member(item.assignees.iter(), me),
        )
    }

    /// Authorship counts as well as assignment: GitLab does not assign an MR
    /// to its author.
    pub fn owns_mr(&self, item: &MergeRequest, me: &str) -> bool {
        self.owns(
            item.project_path(),
            item.assignees.is_empty(),
            self.any_member(
                item.assignees
                    .iter()
                    .chain(item.author.as_ref())
                    .chain(item.reviewers.iter()),
                me,
            ),
        )
    }

    /// Inside the team's own namespaces: its members' work plus unassigned
    /// work.  Anywhere else: only work a member is involved in.
    fn owns(&self, project: &str, unassigned: bool, theirs: bool) -> bool {
        if self.tracks(project) {
            unassigned || theirs
        } else {
            theirs
        }
    }

    fn any_member<'a>(&self, mut users: impl Iterator<Item = &'a User>, me: &str) -> bool {
        users.any(|u| u.username == me || self.members.contains(&u.username))
    }

    /// True for a tracked namespace and for any project underneath one.
    fn tracks(&self, project: &str) -> bool {
        self.tracking_projects.iter().any(|n| {
            project == n
                || project
                    .strip_prefix(n.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}
