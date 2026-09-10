//! Who is on a team and which namespaces its board tracks — the two facts that
//! decide what work belongs to that team.

use crate::domain::{Issue, MergeRequest, User};
use serde::{Deserialize, Serialize};

/// A team, as written in the config and as used to slice the cache.
///
/// Teams may share `tracking_projects` — two teams working one tracker — and
/// are then told apart by their members; a team with its own namespaces gets
/// its own board and its own iteration cadence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub name: String,
    pub members: Vec<String>,
    /// The namespaces this team's board tracks.
    pub tracking_projects: Vec<String>,
}

impl Team {
    /// `me` always counts as one of the team, so your own work stays visible
    /// whichever team's view you are in — including teams you are not on.
    pub fn owns_issue(&self, item: &Issue, me: &str) -> bool {
        self.owns(
            item.project_path(),
            item.assignees.is_empty(),
            self.any_member(item.assignees.iter(), me),
        )
    }

    /// A merge request counts as the team's when a member is assigned *or*
    /// authored it — GitLab does not assign an MR to its author, so authorship
    /// is the only thing that keeps a member's own external MR on the board.
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

    /// Inside the team's own namespaces: its members' work, plus unassigned
    /// work — that is what lets two teams share a tracker and still get
    /// separate boards.  Anywhere else: only work a member is involved in,
    /// which is how external work picked up by assignment stays visible.
    fn owns(&self, project: &str, unassigned: bool, theirs: bool) -> bool {
        if self.tracks(project) {
            unassigned || theirs
        } else {
            theirs
        }
    }

    /// Whether any of `users` is on the team, `me` included.
    fn any_member<'a>(&self, mut users: impl Iterator<Item = &'a User>, me: &str) -> bool {
        users.any(|u| u.username == me || self.members.contains(&u.username))
    }

    /// True for a tracked namespace and for any project underneath one — a
    /// namespace may be a group, whose descendants the fetch walks.
    fn tracks(&self, project: &str) -> bool {
        self.tracking_projects.iter().any(|n| {
            project == n
                || project
                    .strip_prefix(n.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}
