use crate::config::Config;
use crate::onboarding::generate_toml;

#[test]
fn test_generate_toml_roundtrip() {
    let config = Config {
        gitlab_url: "https://gitlab.example.com".to_string(),
        token: "glpat-test123".to_string(),
        me: "binbin".to_string(),
        tracking_projects: vec!["org/tracker".to_string()],
        refresh_interval_secs: 60,
        teams: vec![
            crate::config::TeamConfig {
                name: "frontend".to_string(),
                members: vec!["alice".to_string(), "bob".to_string()],
            },
            crate::config::TeamConfig {
                name: "platform".to_string(),
                members: vec!["charlie".to_string()],
            },
        ],
        filters: vec![crate::config::FilterPreset {
            name: "My issues".to_string(),
            kind: "issue".to_string(),
            conditions: vec![crate::config::PresetCondition {
                field: "assignee".to_string(),
                op: "eq".to_string(),
                value: "$me".to_string(),
            }],
        }],
        sort_presets: Vec::new(),
        label_sort_orders: Vec::new(),
        kanban_columns: Vec::new(),
    };

    let toml_str = generate_toml(&config);

    // Parse back and verify
    let parsed: Config = toml::from_str(&toml_str).expect("Generated TOML should be parseable");
    assert_eq!(parsed.gitlab_url, "https://gitlab.example.com");
    assert_eq!(parsed.token, "glpat-test123");
    assert_eq!(parsed.me, "binbin");
    assert_eq!(parsed.tracking_projects, vec!["org/tracker"]);
    assert_eq!(parsed.refresh_interval_secs, 60);
    assert_eq!(parsed.teams.len(), 2);
    assert_eq!(parsed.teams[0].name, "frontend");
    assert_eq!(parsed.teams[0].members, vec!["alice", "bob"]);
    assert_eq!(parsed.teams[1].name, "platform");
    assert_eq!(parsed.teams[1].members, vec!["charlie"]);
    assert_eq!(parsed.filters.len(), 1);
    assert_eq!(parsed.filters[0].name, "My issues");
}

#[test]
fn test_generate_toml_contains_all_fields() {
    let config = Config {
        gitlab_url: "https://gitlab.com".to_string(),
        token: "glpat-abc".to_string(),
        me: "user".to_string(),
        tracking_projects: vec!["a/b".to_string()],
        refresh_interval_secs: 120,
        teams: vec![],
        filters: vec![],
        sort_presets: Vec::new(),
        label_sort_orders: Vec::new(),
        kanban_columns: Vec::new(),
    };

    let toml_str = generate_toml(&config);
    assert!(toml_str.contains("gitlab_url"));
    assert!(toml_str.contains("token"));
    assert!(toml_str.contains("me"));
    assert!(toml_str.contains("tracking_projects"));
    assert!(toml_str.contains("refresh_interval_secs = 120"));
}

#[test]
fn test_default_filter_presets() {
    let presets = crate::onboarding::default_filter_presets();
    assert!(presets.len() >= 4);

    let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"My open issues"));
    assert!(names.contains(&"My open MRs"));
    assert!(names.contains(&"Needs my review"));
    assert!(names.contains(&"Unassigned issues"));

    // Verify "Needs my review" has the right conditions
    let needs_review = presets
        .iter()
        .find(|p| p.name == "Needs my review")
        .unwrap();
    assert_eq!(needs_review.kind, "merge_request");
    assert_eq!(needs_review.conditions.len(), 3);
    assert!(
        needs_review
            .conditions
            .iter()
            .any(|c| c.field == "approved_by" && c.op == "not_contains" && c.value == "$me")
    );
}
