use super::condition::*;
use crate::domain::*;
use chrono::Utc;

fn make_user(username: &str) -> User {
    User {
        id: "gid://gitlab/User/1".to_string(),
        username: username.to_string(),
    }
}

fn make_issue(
    title: &str,
    state: &str,
    assignees: &[&str],
    labels: &[&str],
    project: &str,
) -> Issue {
    Issue {
        id: "gid://gitlab/User/1".to_string(),
        iid: "1".to_string(),
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
        closed_at: None,
        web_url: String::new(),
        description: None,
        user_notes_count: 0,
        reference: format!("{project}#1"),
        status: None,
        iteration: None,
        weight: None,
    }
}

fn make_mr(
    title: &str,
    state: &str,
    assignees: &[&str],
    reviewers: &[&str],
    draft: bool,
    approved_by: &[&str],
    project: &str,
) -> MergeRequest {
    MergeRequest {
        id: "gid://gitlab/User/1".to_string(),
        iid: "1".to_string(),
        title: title.to_string(),
        state: state.to_string(),
        author: Some(make_user("author")),
        assignees: assignees.iter().map(|u| make_user(u)).collect(),
        reviewers: reviewers.iter().map(|u| make_user(u)).collect(),
        labels: Vec::new(),
        milestone: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        web_url: None,
        description: None,
        draft,
        source_branch: "feature".to_string(),
        target_branch: "main".to_string(),
        head_pipeline: None,
        user_notes_count: None,
        reference: format!("{project}!1"),
        approved_by: approved_by.iter().map(|u| make_user(u)).collect(),
        diff_stats_summary: None,
        approved: None,
        resolvable_discussions_count: None,
        resolved_discussions_count: None,
        detailed_merge_status: None,
    }
}

#[test]
fn test_filter_assignee_eq() {
    let issue = make_issue("Test issue", "opened", &["alice"], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "alice".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me"));

    let conditions_miss = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "bob".to_string(),
    }];
    assert!(!matches_issue(&issue, &conditions_miss, "me"));
}

#[test]
fn test_filter_assignee_none() {
    let issue = make_issue("Unassigned", "opened", &[], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "none".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me"));
}

#[test]
fn test_filter_state() {
    let issue = make_issue("Closed", "closed", &[], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::State,
        op: Op::Eq,
        value: "opened".to_string(),
    }];
    assert!(!matches_issue(&issue, &conditions, "me"));

    let conditions_neq = vec![FilterCondition {
        field: Field::State,
        op: Op::Neq,
        value: "opened".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions_neq, "me"));
}

#[test]
fn test_filter_label_contains() {
    let issue = make_issue("Bug", "opened", &["alice"], &["bug", "urgent"], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Label,
        op: Op::Contains,
        value: "bug".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me"));

    let not_conditions = vec![FilterCondition {
        field: Field::Label,
        op: Op::NotContains,
        value: "feature".to_string(),
    }];
    assert!(matches_issue(&issue, &not_conditions, "me"));
}

#[test]
fn test_filter_me_variable() {
    let issue = make_issue("My issue", "opened", &["binbin"], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Assignee,
        op: Op::Eq,
        value: "$me".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "binbin"));
    assert!(!matches_issue(&issue, &conditions, "alice"));
}

#[test]
fn test_filter_multiple_conditions() {
    let issue = make_issue("Important bug", "opened", &["alice"], &["bug"], "org/repo");

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
    assert!(matches_issue(&issue, &conditions, "me"));

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
    assert!(!matches_issue(&issue, &conditions_fail, "me"));
}

#[test]
fn test_filter_title() {
    let issue = make_issue("Fix authentication bug", "opened", &[], &[], "org/repo");
    let conditions = vec![FilterCondition {
        field: Field::Title,
        op: Op::Contains,
        value: "auth".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me"));
}

#[test]
fn test_mr_filter_draft() {
    let draft_mr = make_mr(
        "WIP: feature",
        "opened",
        &["alice"],
        &[],
        true,
        &[],
        "org/repo",
    );
    let ready_mr = make_mr(
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
    assert!(!matches_mr(&draft_mr, &not_draft, "me"));
    assert!(matches_mr(&ready_mr, &not_draft, "me"));
}

#[test]
fn test_mr_filter_approved_by() {
    let mr = make_mr(
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
    assert!(matches_mr(&mr, &approved_by_me, "me"));
    // charlie has approved
    assert!(!matches_mr(&mr, &approved_by_me, "charlie"));
}

#[test]
fn test_mr_filter_reviewer() {
    let mr = make_mr(
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
    assert!(matches_mr(&mr, &reviewer_filter, "me"));

    let not_reviewer = vec![FilterCondition {
        field: Field::Reviewer,
        op: Op::Contains,
        value: "alice".to_string(),
    }];
    assert!(!matches_mr(&mr, &not_reviewer, "me"));
}

#[test]
fn test_empty_conditions_matches_all() {
    let issue = make_issue("Anything", "opened", &[], &[], "org/repo");
    assert!(matches_issue(&issue, &[], "me"));
}

#[test]
fn test_field_from_str() {
    assert_eq!(Field::from_str("assignee"), Some(Field::Assignee));
    assert_eq!(Field::from_str("draft"), Some(Field::Draft));
    assert_eq!(Field::from_str("approved_by"), Some(Field::ApprovedBy));
    assert_eq!(Field::from_str("unknown"), None);
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
    let issue = make_issue("Bug", "opened", &[], &[], "other/project");
    let conditions = vec![FilterCondition {
        field: Field::Project,
        op: Op::Eq,
        value: "other/project".to_string(),
    }];
    assert!(matches_issue(&issue, &conditions, "me"));

    let wrong_project = vec![FilterCondition {
        field: Field::Project,
        op: Op::Eq,
        value: "org/repo".to_string(),
    }];
    assert!(!matches_issue(&issue, &wrong_project, "me"));
}

/// Two teams share one tracker; a third has its own.
#[test]
fn a_team_owns_its_namespace_and_its_people_but_not_a_co_tenants_work() {
    use crate::team::Team;
    let at = |project, assignee: &[&str]| make_issue("t", "opened", assignee, &[], project);
    let team = |member: &str, ns: &str| Team {
        name: member.to_string(),
        members: vec![member.to_string()],
        tracking_projects: vec![ns.to_string()],
    };
    let shared = team("alice", "org/shared");
    let own = team("carol", "org/own");

    // Its own namespace: members' work and unassigned work.
    assert!(shared.owns_issue(&at("org/shared", &["alice"]), "me"));
    assert!(shared.owns_issue(&at("org/shared/widget", &[]), "me")); // descendant project
    // Sharing a tracker: the co-tenant team's people are filtered out.
    assert!(!shared.owns_issue(&at("org/shared", &["bob"]), "me"));
    // Another team's board, unassigned — not this team's problem.
    assert!(!own.owns_issue(&at("org/shared", &[]), "me"));
    // Outside the board, their own work still shows.
    assert!(own.owns_issue(&at("elsewhere/lib", &["carol"]), "me"));
    assert!(!own.owns_issue(&at("elsewhere/lib", &["alice"]), "me"));
    // Your own work follows you into any team's view.
    assert!(own.owns_issue(&at("elsewhere/lib", &["me"]), "me"));
}

#[test]
fn a_team_owns_an_mr_a_member_authored_even_with_no_assignee() {
    use crate::team::Team;
    let team = Team {
        name: "carol".to_string(),
        members: vec!["carol".to_string()],
        tracking_projects: vec!["org/own".to_string()],
    };
    let authored_by = |project, who: &str| {
        let mut mr = make_mr("t", "opened", &[], &[], false, &[], project);
        mr.author = Some(make_user(who));
        mr
    };

    // Outside the board an unassigned MR rides in on its author alone.
    assert!(team.owns_mr(&authored_by("elsewhere/lib", "carol"), "me"));
    assert!(team.owns_mr(&authored_by("elsewhere/lib", "me"), "me"));
    assert!(!team.owns_mr(&authored_by("elsewhere/lib", "alice"), "me"));
    // A reviewer alone is still not ownership.
    let mut reviewed = authored_by("elsewhere/lib", "alice");
    reviewed.reviewers = vec![make_user("carol")];
    assert!(!team.owns_mr(&reviewed, "me"));
    // Inside the board, an outsider's unassigned MR still shows — the
    // unassigned rule is unchanged by widening to authorship.
    assert!(team.owns_mr(&authored_by("org/own", "alice"), "me"));
}

#[test]
fn test_filter_merge_status() {
    let mut mr = make_mr("Ready", "opened", &[], &[], false, &["alice"], "org/repo");
    mr.detailed_merge_status = Some("mergeable".to_string());
    let mergeable = vec![FilterCondition {
        field: Field::MergeStatus,
        op: Op::Eq,
        value: "mergeable".to_string(),
    }];
    assert!(matches_mr(&mr, &mergeable, "me"));

    // Approved by someone, but a rule or an open thread still blocks the merge.
    mr.detailed_merge_status = Some("discussions_not_resolved".to_string());
    assert!(!matches_mr(&mr, &mergeable, "me"));
}
