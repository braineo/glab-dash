use super::condition::*;
use crate::gitlab::types::*;
use chrono::Utc;

fn make_user(username: &str) -> User {
    User {
        id: 1,
        username: username.to_string(),
        name: username.to_string(),
        avatar_url: None,
        web_url: String::new(),
    }
}

fn make_tracked_issue(
    title: &str,
    state: &str,
    assignees: &[&str],
    labels: &[&str],
    project: &str,
) -> TrackedIssue {
    TrackedIssue {
        issue: Issue {
            id: 1,
            iid: 1,
            title: title.to_string(),
            state: state.to_string(),
            author: Some(make_user("author")),
            assignees: assignees.iter().map(|u| make_user(u)).collect(),
            labels: labels
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            milestone: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            web_url: String::new(),
            description: None,
            user_notes_count: 0,
            references: None,
            custom_status: None,
            iteration: None,
            weight: None,
        },
        project_path: project.to_string(),
    }
}

fn make_tracked_mr(
    title: &str,
    state: &str,
    assignees: &[&str],
    reviewers: &[&str],
    draft: bool,
    approved_by: &[&str],
    project: &str,
) -> TrackedMergeRequest {
    TrackedMergeRequest {
        mr: MergeRequest {
            id: 1,
            iid: 1,
            title: title.to_string(),
            state: state.to_string(),
            author: Some(make_user("author")),
            assignees: assignees.iter().map(|u| make_user(u)).collect(),
            reviewers: reviewers.iter().map(|u| make_user(u)).collect(),
            labels: Vec::new(),
            milestone: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            web_url: String::new(),
            description: None,
            draft,
            work_in_progress: false,
            merge_status: None,
            source_branch: "feature".to_string(),
            target_branch: "main".to_string(),
            head_pipeline: None,
            user_notes_count: 0,
            references: None,
            approved_by: approved_by
                .iter()
                .map(|u| ApprovalUser { user: make_user(u) })
                .collect(),
        },
        project_path: project.to_string(),
    }
}

#[test]
fn test_filter_assignee_eq() {
    let issue = make_tracked_issue("Test issue", "opened", &["alice"], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "alice".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me", &[]));

    let conditions_miss = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "bob".to_string(),
    }];
    assert!(!matches_issue(&issue, &conditions_miss, "me", &[]));
}

#[test]
fn test_filter_assignee_none() {
    let issue = make_tracked_issue("Unassigned", "opened", &[], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "none".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me", &[]));
}

#[test]
fn test_filter_state() {
    let issue = make_tracked_issue("Closed", "closed", &[], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::State,
        op: Op::Eq,
        value: "opened".to_string(),
    }];
    assert!(!matches_issue(&issue, &conditions, "me", &[]));

    let conditions_neq = vec![FilterCondition {
        field: Field::State,
        op: Op::Neq,
        value: "opened".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions_neq, "me", &[]));
}

#[test]
fn test_filter_label_contains() {
    let issue = make_tracked_issue("Bug", "opened", &["alice"], &["bug", "urgent"], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Label,
        op: Op::Contains,
        value: "bug".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me", &[]));

    let not_conditions = vec![FilterCondition {
        field: Field::Label,
        op: Op::NotContains,
        value: "feature".to_string(),
    }];
    assert!(matches_issue(&issue, &not_conditions, "me", &[]));
}

#[test]
fn test_filter_me_variable() {
    let issue = make_tracked_issue("My issue", "opened", &["binbin"], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "$me".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "binbin", &[]));
    assert!(!matches_issue(&issue, &conditions, "alice", &[]));
}

#[test]
fn test_filter_multiple_conditions() {
    let issue = make_tracked_issue("Important bug", "opened", &["alice"], &["bug"], "org/repo");

    // All conditions must match (AND)
    let conditions = vec![
        FilterCondition {
            field: Field::Assignee,
            op: Op::Eq,
            value: "alice".to_string(),
        },
        FilterCondition {
            field: Field::Label,
            op: Op::Contains,
            value: "bug".to_string(),
        },
        FilterCondition {
            field: Field::State,
            op: Op::Eq,
            value: "opened".to_string(),
        },
    ];
    assert!(matches_issue(&issue, &conditions, "me", &[]));

    // One condition fails → doesn't match
    let conditions_fail = vec![
        FilterCondition {
            field: Field::Assignee,
            op: Op::Eq,
            value: "alice".to_string(),
        },
        FilterCondition {
            field: Field::State,
            op: Op::Eq,
            value: "closed".to_string(),
        },
    ];
    assert!(!matches_issue(&issue, &conditions_fail, "me", &[]));
}

#[test]
fn test_filter_title() {
    let issue = make_tracked_issue("Fix authentication bug", "opened", &[], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Title,
        op: Op::Contains,
        value: "auth".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me", &[]));
}

#[test]
fn test_mr_filter_draft() {
    let draft_mr = make_tracked_mr(
        "WIP: feature",
        "opened",
        &["alice"],
        &[],
        true,
        &[],
        "org/repo",
    );
    let ready_mr = make_tracked_mr(
        "Ready feature",
        "opened",
        &["alice"],
        &[],
        false,
        &[],
        "org/repo",
    );

    let not_draft = vec![FilterCondition {
        field: Field::Draft,
        op: Op::Eq,
        value: "false".to_string(),
    }];
    assert!(!matches_mr(&draft_mr, &not_draft, "me", &[]));
    assert!(matches_mr(&ready_mr, &not_draft, "me", &[]));
}

#[test]
fn test_mr_filter_approved_by() {
    let mr = make_tracked_mr(
        "Feature",
        "opened",
        &["alice"],
        &["bob"],
        false,
        &["charlie"],
        "org/repo",
    );

    let approved_by_me = vec![FilterCondition {
        field: Field::ApprovedBy,
        op: Op::NotContains,
        value: "$me".to_string(),
    }];
    // "me" hasn't approved, so NotContains should be true
    assert!(matches_mr(&mr, &approved_by_me, "me", &[]));
    // charlie has approved
    assert!(!matches_mr(&mr, &approved_by_me, "charlie", &[]));
}

#[test]
fn test_mr_filter_reviewer() {
    let mr = make_tracked_mr(
        "Feature",
        "opened",
        &[],
        &["bob", "charlie"],
        false,
        &[],
        "org/repo",
    );

    let reviewer_filter = vec![FilterCondition {
        field: Field::Reviewer,
        op: Op::Contains,
        value: "bob".to_string(),
    }];
    assert!(matches_mr(&mr, &reviewer_filter, "me", &[]));

    let not_reviewer = vec![FilterCondition {
        field: Field::Reviewer,
        op: Op::Contains,
        value: "alice".to_string(),
    }];
    assert!(!matches_mr(&mr, &not_reviewer, "me", &[]));
}

#[test]
fn test_empty_conditions_matches_all() {
    let issue = make_tracked_issue("Anything", "opened", &[], &[], "org/repo");
    assert!(matches_issue(&issue, &[], "me", &[]));
}

#[test]
fn test_field_from_str() {
    assert_eq!(Field::from_str("assignee"), Some(Field::Assignee));
    assert_eq!(Field::from_str("draft"), Some(Field::Draft));
    assert_eq!(Field::from_str("approved_by"), Some(Field::ApprovedBy));
    assert_eq!(Field::from_str("unknown"), None);
}

#[test]
fn test_op_from_str() {
    assert_eq!(Op::from_str("eq"), Some(Op::Eq));
    assert_eq!(Op::from_str("="), Some(Op::Eq));
    assert_eq!(Op::from_str("neq"), Some(Op::Neq));
    assert_eq!(Op::from_str("!="), Some(Op::Neq));
    assert_eq!(Op::from_str("contains"), Some(Op::Contains));
    assert_eq!(Op::from_str("~"), Some(Op::Contains));
    assert_eq!(Op::from_str("not_contains"), Some(Op::NotContains));
    assert_eq!(Op::from_str("!~"), Some(Op::NotContains));
    assert_eq!(Op::from_str("garbage"), None);
}

#[test]
fn test_condition_display() {
    let cond = FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "alice".to_string(),
    };
    assert_eq!(cond.display(), "assignee=alice");

    let cond2 = FilterCondition {
        field: Field::Draft,
        op: Op::Neq,
        value: "true".to_string(),
    };
    assert_eq!(cond2.display(), "draft!=true");
}

#[test]
fn test_filter_project() {
    let issue = make_tracked_issue("Bug", "opened", &[], &[], "other/project");
    let conditions = vec![FilterCondition {
        field: Field::Project,
        op: Op::Eq,
        value: "other/project".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me", &[]));

    let wrong_project = vec![FilterCondition {
        field: Field::Project,
        op: Op::Eq,
        value: "org/repo".to_string(),
    }];
    assert!(!matches_issue(&issue, &wrong_project, "me", &[]));
}
