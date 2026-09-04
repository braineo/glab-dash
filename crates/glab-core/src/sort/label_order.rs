use std::cmp::Ordering;

/// Compare two items by their scoped labels.
///
/// Given a scope like `"workflow"`, this finds labels prefixed with `"workflow::"`
/// on each item, looks up the value portion in the priority list, and compares
/// by rank. Handles nested scopes naturally — `workflow::workspace::hardware::robot`
/// with scope `"workflow"` extracts value `"workspace::hardware::robot"`.
///
/// Items without a matching label sort last. Unlisted values sort after all listed ones.
pub fn compare_by_label_scope(
    labels_a: &[String],
    labels_b: &[String],
    scope: &str,
    priority: &[String],
) -> Ordering {
    let rank_a = scope_rank(labels_a, scope, priority);
    let rank_b = scope_rank(labels_b, scope, priority);
    rank_a.cmp(&rank_b)
}

fn scope_rank(labels: &[String], scope: &str, priority: &[String]) -> usize {
    let prefix = format!("{scope}::");
    // Find the best (lowest) rank among all matching labels
    let mut best = usize::MAX;
    for label in labels {
        if let Some(value) = label.strip_prefix(&prefix) {
            if let Some(pos) = priority.iter().position(|v| v == value) {
                best = best.min(pos);
            } else {
                // Unlisted value: after all listed, but before "no label"
                best = best.min(priority.len());
            }
        }
    }
    best
}
