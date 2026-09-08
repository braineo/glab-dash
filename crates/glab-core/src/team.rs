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
        self.owns(item.project_path(), &item.assignees, me)
    }

    pub fn owns_mr(&self, item: &MergeRequest, me: &str) -> bool {
        self.owns(item.project_path(), &item.assignees, me)
    }

    /// Inside the team's own namespaces: its members' work, plus unassigned
    /// work — that is what lets two teams share a tracker and still get
    /// separate boards.  Anywhere else: only work assigned to a member, which
    /// is how external issues picked up by assignment stay visible.
    fn owns(&self, project: &str, assignees: &[User], me: &str) -> bool {
        let theirs = assignees
            .iter()
            .any(|a| a.username == me || self.members.contains(&a.username));
        if self.tracks(project) {
            assignees.is_empty() || theirs
        } else {
            theirs
        }
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
