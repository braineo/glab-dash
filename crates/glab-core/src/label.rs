//! Scoped label names.
//!
//! A GitLab scoped label is written `scope::value`, and the value may itself
//! contain the separator (`workflow::workspace::hardware`): the scope is
//! everything before the first `::`, the value everything after it. GitLab
//! allows only one label per scope on an item, which is why selecting one
//! deselects its siblings.

/// The separator between a label's scope and its value.
pub const SEP: &str = "::";

/// The scope of a scoped label, or `None` when the label carries no scope.
pub fn scope(label: &str) -> Option<&str> {
    label.split_once(SEP).map(|(scope, _)| scope)
}

/// The value `label` carries in `scope`, or `None` when it is not in that scope.
pub fn value_in_scope<'a>(label: &'a str, scope: &str) -> Option<&'a str> {
    label
        .strip_prefix(scope)
        .and_then(|rest| rest.strip_prefix(SEP))
}

/// A label's `::`-separated segments, which is how a label is rendered as a
/// row of chips.
pub fn segments(label: &str) -> impl Iterator<Item = &str> {
    label.split(SEP)
}

/// Whether two labels claim the same scope, so setting one clears the other.
/// Unscoped labels never conflict, not even with each other.
fn same_scope(a: &str, b: &str) -> bool {
    matches!((scope(a), scope(b)), (Some(a), Some(b)) if a == b)
}

/// Toggle the label at `index` of a selection, enforcing GitLab's one label
/// per scope: selecting a scoped label clears any other selected label sharing
/// its scope. Deselecting clears nothing else.
///
/// # Panics
/// If `index` is out of range for either slice.
pub fn toggle(labels: &[String], selected: &mut [bool], index: usize) {
    if selected[index] {
        selected[index] = false;
        return;
    }
    for i in 0..labels.len() {
        if i != index && selected[i] && same_scope(&labels[i], &labels[index]) {
            selected[i] = false;
        }
    }
    selected[index] = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_is_everything_before_the_first_separator() {
        assert_eq!(scope("workflow::in progress"), Some("workflow"));
        assert_eq!(scope("workflow::workspace::hardware"), Some("workflow"));
        assert_eq!(scope("bug"), None);
    }

    #[test]
    fn a_value_is_everything_after_its_own_scope() {
        assert_eq!(
            value_in_scope("workflow::workspace::hardware", "workflow"),
            Some("workspace::hardware")
        );
        // A scope that is only a prefix of the label's own scope does not match.
        assert_eq!(value_in_scope("workflow::done", "work"), None);
        assert_eq!(value_in_scope("bug", "workflow"), None);
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn selecting_a_scoped_label_clears_its_siblings_but_not_other_scopes() {
        let labels = labels(&["p::1", "p::2", "workflow::done", "bug"]);
        let mut selected = [true, false, true, true];
        toggle(&labels, &mut selected, 1);
        assert_eq!(selected, [false, true, true, true]);
    }

    #[test]
    fn deselecting_leaves_every_other_label_alone() {
        let labels = labels(&["p::1", "p::2"]);
        let mut selected = [true, false];
        toggle(&labels, &mut selected, 0);
        assert_eq!(selected, [false, false]);
    }

    #[test]
    fn unscoped_labels_never_clear_each_other() {
        let labels = labels(&["bug", "regression"]);
        let mut selected = [true, false];
        toggle(&labels, &mut selected, 1);
        assert_eq!(selected, [true, true]);
    }

    #[test]
    fn only_scoped_labels_conflict_and_only_within_one_scope() {
        assert!(same_scope("p::1", "p::2"));
        assert!(!same_scope("p::1", "workflow::done"));
        assert!(!same_scope("bug", "regression"));
    }
}
