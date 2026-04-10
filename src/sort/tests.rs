use std::collections::HashMap;

use chrono::{Duration, Utc};

use crate::gitlab::types::*;

use super::label_order::compare_by_label_scope;
use super::spec::*;

fn make_user(username: &str) -> User {
    User {
        id: 1,
        username: username.to_string(),
        name: username.to_string(),
        avatar_url: None,
        web_url: String::new(),
    }
}

fn make_issue(iid: u64, title: &str, labels: &[&str], updated_days_ago: i64) -> TrackedIssue {
    TrackedIssue {
        issue: Issue {
            id: iid,
            iid,
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
            web_url: String::new(),
            description: None,
            user_notes_count: 0,
            references: None,
            custom_status: None,
            iteration: None,
            weight: None,
        },
        project_path: "org/repo".to_string(),
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
    sort_issues(&mut indices, &issues, &specs, &HashMap::new());
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
    sort_issues(&mut indices, &issues, &specs, &HashMap::new());
    assert_eq!(indices, vec![1, 2, 0]); // 10, 20, 30
}

#[test]
fn test_multi_key_sort() {
    let mut issues = vec![
        make_issue(1, "A", &[], 1),
        make_issue(2, "B", &[], 5),
        make_issue(3, "C", &[], 1),
    ];
    issues[0].issue.state = "opened".to_string();
    issues[1].issue.state = "closed".to_string();
    issues[2].issue.state = "opened".to_string();

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
    sort_issues(&mut indices, &issues, &specs, &HashMap::new());
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
    let mut label_orders = HashMap::new();
    label_orders.insert(
        "workflow".to_string(),
        vec![
            "backlog".to_string(),
            "in_progress".to_string(),
            "review".to_string(),
            "done".to_string(),
        ],
    );
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
    let mut label_orders = HashMap::new();
    label_orders.insert(
        "workflow".to_string(),
        vec![
            "backlog".to_string(),
            "workspace::hardware::robot".to_string(),
        ],
    );
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
    let mut label_orders = HashMap::new();
    label_orders.insert("p".to_string(), vec!["high".to_string(), "low".to_string()]);
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
    let priority = vec![
        "critical".to_string(),
        "high".to_string(),
        "medium".to_string(),
        "low".to_string(),
    ];

    let a = vec!["p::high".to_string()];
    let b = vec!["p::low".to_string()];
    assert_eq!(
        compare_by_label_scope(&a, &b, "p", &priority),
        std::cmp::Ordering::Less,
    );

    let c = vec!["unrelated".to_string()];
    assert_eq!(
        compare_by_label_scope(&a, &c, "p", &priority),
        std::cmp::Ordering::Less,
    );
}

#[test]
fn test_empty_specs_preserves_order() {
    let issues = vec![make_issue(3, "C", &[], 0), make_issue(1, "A", &[], 0)];
    let mut indices: Vec<usize> = vec![0, 1];
    sort_issues(&mut indices, &issues, &[], &HashMap::new());
    assert_eq!(indices, vec![0, 1]); // unchanged
}
