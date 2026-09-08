use chrono::{Duration, Utc};

use crate::domain::*;

use super::label_order::{LabelOrder, LabelOrders};
use super::spec::*;

/// The declared order of one scope's values.
fn label_orders(scope: &str, values: &[&str]) -> LabelOrders {
    [LabelOrder {
        scope: scope.to_string(),
        values: values.iter().map(ToString::to_string).collect(),
    }]
    .into_iter()
    .collect()
}

fn make_user(username: &str) -> User {
    User {
        id: "gid://gitlab/User/1".to_string(),
        username: username.to_string(),
    }
}

fn make_issue(iid: u64, title: &str, labels: &[&str], updated_days_ago: i64) -> Issue {
    Issue {
        id: iid.to_string(),
        iid: iid.to_string(),
        title: title.to_string(),
        state: "opened".to_string(),
        author: Some(make_user("author")),
        assignees: vec![],
        labels: labels
            .iter()
            .map(std::string::ToString::to_string)
            .collect(),
        milestone: None,
        created_at: Utc::now() - Duration::days(updated_days_ago + 10),
        updated_at: Utc::now() - Duration::days(updated_days_ago),
        closed_at: None,
        web_url: String::new(),
        description: None,
        user_notes_count: 0,
        reference: format!("org/repo#{iid}"),
        status: None,
        iteration: None,
        weight: None,
    }
}

#[test]
fn test_sort_by_updated_at_desc() {
    let issues = vec![
        make_issue(1, "Old", &[], 10),
        make_issue(2, "New", &[], 1),
        make_issue(3, "Mid", &[], 5),
    ];
    let mut indices: Vec<usize> = vec![0, 1, 2];
    let specs = vec![SortSpec {
        field: SortField::UpdatedAt,
        direction: SortDirection::Desc,
        label_scope: None,
    }];
    sort_issues(&mut indices, &issues, &specs, &LabelOrders::default());
    // Most recent first: New(1d), Mid(5d), Old(10d)
    assert_eq!(indices, vec![1, 2, 0]);
}

#[test]
fn test_sort_by_iid_asc() {
    let issues = vec![
        make_issue(30, "C", &[], 0),
        make_issue(10, "A", &[], 0),
        make_issue(20, "B", &[], 0),
    ];
    let mut indices: Vec<usize> = vec![0, 1, 2];
    let specs = vec![SortSpec {
        field: SortField::Iid,
        direction: SortDirection::Asc,
        label_scope: None,
    }];
    sort_issues(&mut indices, &issues, &specs, &LabelOrders::default());
    assert_eq!(indices, vec![1, 2, 0]); // 10, 20, 30
}

#[test]
fn test_multi_key_sort() {
    let mut issues = vec![
        make_issue(1, "A", &[], 1),
        make_issue(2, "B", &[], 5),
        make_issue(3, "C", &[], 1),
    ];
    issues[0].state = "opened".to_string();
    issues[1].state = "closed".to_string();
    issues[2].state = "opened".to_string();

    let mut indices: Vec<usize> = vec![0, 1, 2];
    let specs = vec![
        SortSpec {
            field: SortField::State,
            direction: SortDirection::Asc,
            label_scope: None,
        },
        SortSpec {
            field: SortField::Iid,
            direction: SortDirection::Desc,
            label_scope: None,
        },
    ];
    sort_issues(&mut indices, &issues, &specs, &LabelOrders::default());
    // opened items first (3 desc, 1 desc), then closed (2)
    assert_eq!(indices, vec![2, 0, 1]);
}

#[test]
fn test_label_scope_sort() {
    let issues = vec![
        make_issue(1, "Done", &["workflow::done"], 0),
        make_issue(2, "Backlog", &["workflow::backlog"], 0),
        make_issue(3, "Review", &["workflow::review"], 0),
    ];
    let label_orders = label_orders("workflow", &["backlog", "in_progress", "review", "done"]);
    let mut indices: Vec<usize> = vec![0, 1, 2];
    let specs = vec![SortSpec {
        field: SortField::Label,
        direction: SortDirection::Asc,
        label_scope: Some("workflow".to_string()),
    }];
    sort_issues(&mut indices, &issues, &specs, &label_orders);
    // backlog(0) < review(2) < done(3)
    assert_eq!(indices, vec![1, 2, 0]);
}

#[test]
fn test_label_scope_nested() {
    let issues = vec![
        make_issue(1, "Robot", &["workflow::workspace::hardware::robot"], 0),
        make_issue(2, "Simple", &["workflow::backlog"], 0),
    ];
    let label_orders = label_orders("workflow", &["backlog", "workspace::hardware::robot"]);
    let mut indices: Vec<usize> = vec![0, 1];
    let specs = vec![SortSpec {
        field: SortField::Label,
        direction: SortDirection::Asc,
        label_scope: Some("workflow".to_string()),
    }];
    sort_issues(&mut indices, &issues, &specs, &label_orders);
    // backlog(0) < workspace::hardware::robot(1)
    assert_eq!(indices, vec![1, 0]);
}

#[test]
fn test_label_scope_missing_sorts_last() {
    let issues = vec![
        make_issue(1, "Has label", &["p::high"], 0),
        make_issue(2, "No label", &[], 0),
        make_issue(3, "Has label", &["p::low"], 0),
    ];
    let label_orders = label_orders("p", &["high", "low"]);
    let mut indices: Vec<usize> = vec![0, 1, 2];
    let specs = vec![SortSpec {
        field: SortField::Label,
        direction: SortDirection::Asc,
        label_scope: Some("p".to_string()),
    }];
    sort_issues(&mut indices, &issues, &specs, &label_orders);
    // high(0) < low(1) < none(MAX)
    assert_eq!(indices, vec![0, 2, 1]);
}

#[test]
fn test_compare_by_label_scope_direct() {
    let orders = label_orders("p", &["critical", "high", "medium", "low"]);

    let a = vec!["p::high".to_string()];
    let b = vec!["p::low".to_string()];
    assert_eq!(orders.compare(&a, &b, "p"), std::cmp::Ordering::Less);

    let c = vec!["unrelated".to_string()];
    assert_eq!(orders.compare(&a, &c, "p"), std::cmp::Ordering::Less);
}

#[test]
fn test_empty_specs_preserves_order() {
    let issues = vec![make_issue(3, "C", &[], 0), make_issue(1, "A", &[], 0)];
    let mut indices: Vec<usize> = vec![0, 1];
    sort_issues(&mut indices, &issues, &[], &LabelOrders::default());
    assert_eq!(indices, vec![0, 1]); // unchanged
}
