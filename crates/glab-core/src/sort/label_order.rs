//! Ordering items by where their scoped labels fall in a user-declared order.
//!
//! A [`LabelOrder`] names one label scope and lists its values from first to
//! last, which is how a workflow or priority scope is given a meaning the label
//! text alone does not carry. [`LabelOrders`] holds every declared order and is
//! what a sort by that scope reads.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::label;

/// The declared order of the values within one label scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelOrder {
    /// The scope this orders, written without the `::` separator.
    pub scope: String,
    /// The scope's values, first to last.
    pub values: Vec<String>,
}

/// Every declared [`LabelOrder`], in the order they were declared.
///
/// Kept as a list rather than a map because a handful of scopes are declared at
/// most, and the declaration order is what a menu of them shows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LabelOrders(Vec<LabelOrder>);

impl LabelOrders {
    /// The declared values for `scope`, empty when no order is declared for it.
    pub fn values(&self, scope: &str) -> &[String] {
        self.0
            .iter()
            .find(|order| order.scope == scope)
            .map_or(&[], |order| order.values.as_slice())
    }

    /// The declared orders, for listing the scopes that can be sorted by.
    pub fn iter(&self) -> std::slice::Iter<'_, LabelOrder> {
        self.0.iter()
    }

    /// Where `labels` places an item in `scope`'s declared order.
    ///
    /// The best-ranked label wins when an item carries several in one scope. A
    /// value the order does not list ranks after every listed one, and an item
    /// carrying no label in the scope at all ranks last.
    pub fn rank(&self, labels: &[String], scope: &str) -> usize {
        let values = self.values(scope);
        let mut best = usize::MAX;
        for label in labels {
            if let Some(value) = label::value_in_scope(label, scope) {
                let rank = values
                    .iter()
                    .position(|declared| declared == value)
                    .unwrap_or(values.len());
                best = best.min(rank);
            }
        }
        best
    }

    /// Compare two items by their place in `scope`'s declared order.
    pub fn compare(&self, labels_a: &[String], labels_b: &[String], scope: &str) -> Ordering {
        self.rank(labels_a, scope).cmp(&self.rank(labels_b, scope))
    }
}

impl FromIterator<LabelOrder> for LabelOrders {
    fn from_iter<I: IntoIterator<Item = LabelOrder>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<'a> IntoIterator for &'a LabelOrders {
    type Item = &'a LabelOrder;
    type IntoIter = std::slice::Iter<'a, LabelOrder>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders() -> LabelOrders {
        [LabelOrder {
            scope: "workflow".to_string(),
            values: vec!["todo".to_string(), "doing".to_string(), "done".to_string()],
        }]
        .into_iter()
        .collect()
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_declared_value_ranks_by_its_position() {
        let orders = orders();
        assert_eq!(orders.rank(&labels(&["workflow::todo"]), "workflow"), 0);
        assert_eq!(orders.rank(&labels(&["workflow::done"]), "workflow"), 2);
    }

    #[test]
    fn an_undeclared_value_ranks_after_every_declared_one_but_before_no_label() {
        let orders = orders();
        let undeclared = orders.rank(&labels(&["workflow::blocked"]), "workflow");
        let missing = orders.rank(&labels(&["bug"]), "workflow");
        assert_eq!(undeclared, 3);
        assert_eq!(missing, usize::MAX);
        assert!(undeclared < missing);
    }

    #[test]
    fn the_best_ranked_label_wins_when_an_item_carries_several_in_one_scope() {
        let orders = orders();
        let both = labels(&["workflow::done", "workflow::todo"]);
        assert_eq!(orders.rank(&both, "workflow"), 0);
    }

    #[test]
    fn a_nested_value_keeps_its_separators() {
        let orders: LabelOrders = [LabelOrder {
            scope: "workflow".to_string(),
            values: vec!["workspace::hardware".to_string()],
        }]
        .into_iter()
        .collect();
        assert_eq!(
            orders.rank(&labels(&["workflow::workspace::hardware"]), "workflow"),
            0
        );
    }

    #[test]
    fn an_undeclared_scope_leaves_every_item_equal() {
        let orders = orders();
        assert_eq!(
            orders.compare(&labels(&["p::1"]), &labels(&["p::2"]), "p"),
            Ordering::Equal
        );
    }
}
