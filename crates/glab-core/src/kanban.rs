use serde::{Deserialize, Serialize};

use crate::domain::WorkItemStatus;

/// Statuses match case-insensitively: a column is written by hand in the
/// config while the status names come from GitLab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanbanColumn {
    pub name: String,
    /// The empty name gathers items carrying no status at all.
    pub statuses: Vec<String>,
}

impl KanbanColumn {
    pub fn matches(&self, status: Option<&str>) -> bool {
        let status = status.unwrap_or_default();
        self.statuses
            .iter()
            .any(|listed| listed.eq_ignore_ascii_case(status))
    }

    /// Ordered by the position GitLab gives each status, led by a column for
    /// the items carrying none.
    pub fn from_statuses(statuses: &[WorkItemStatus]) -> Vec<Self> {
        let mut ordered: Vec<&WorkItemStatus> = statuses.iter().collect();
        ordered.sort_by_key(|status| status.position.unwrap_or(i32::MAX));

        let mut columns = vec![Self {
            name: "No Status".to_string(),
            statuses: vec![String::new()],
        }];
        columns.extend(ordered.into_iter().map(|status| Self {
            name: status.name.clone(),
            statuses: vec![status.name.clone()],
        }));
        columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(name: &str, position: Option<i32>) -> WorkItemStatus {
        WorkItemStatus {
            id: name.to_string(),
            name: name.to_string(),
            position,
            category: None,
        }
    }

    #[test]
    fn a_column_gathers_its_statuses_whatever_their_case() {
        let column = KanbanColumn {
            name: "Doing".to_string(),
            statuses: vec!["In Progress".to_string(), "In Review".to_string()],
        };
        assert!(column.matches(Some("in progress")));
        assert!(column.matches(Some("In Review")));
        assert!(!column.matches(Some("Done")));
        assert!(!column.matches(None));
    }

    #[test]
    fn the_derived_columns_lead_with_the_unstatused_one_then_follow_position() {
        let columns = KanbanColumn::from_statuses(&[
            status("Done", Some(2)),
            status("Todo", Some(0)),
            status("Doing", Some(1)),
        ]);
        let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["No Status", "Todo", "Doing", "Done"]);
        assert!(columns[0].matches(None));
    }
}
