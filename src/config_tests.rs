use crate::config::Config;

#[test]
fn test_parse_config() {
    let toml_str = r#"
gitlab_url = "https://gitlab.example.com"
token = "glpat-test"
me = "binbin"
tracking_projects = ["org/tracker"]

[[teams]]
name = "frontend"
members = ["alice", "bob"]

[[teams]]
name = "platform"
members = ["charlie", "dave"]

[[filters]]
name = "My issues"
kind = "issue"

[[filters.conditions]]
field = "assignee"
op = "eq"
value = "$me"
"#;

    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.gitlab_url, "https://gitlab.example.com");
    assert_eq!(config.token, "glpat-test");
    assert_eq!(config.me, "binbin");
    assert_eq!(config.tracking_projects, vec!["org/tracker"]);
    assert!(config.is_tracking_project("org/tracker"));
    assert!(!config.is_tracking_project("other/repo"));
    assert_eq!(config.teams.len(), 2);
    assert_eq!(config.teams[0].name, "frontend");
    assert_eq!(config.teams[0].members, vec!["alice", "bob"]);
    assert_eq!(config.teams[1].name, "platform");
    assert_eq!(config.filters.len(), 1);
    assert_eq!(config.filters[0].name, "My issues");
    assert_eq!(config.filters[0].conditions[0].field, "assignee");
    assert_eq!(config.filters[0].conditions[0].op, "eq");
    assert_eq!(config.filters[0].conditions[0].value, "$me");
    assert_eq!(config.refresh_interval_secs, 60); // default
}

#[test]
fn test_parse_config_multi_project() {
    let toml_str = r#"
gitlab_url = "https://gitlab.com"
token = "test"
me = "binbin"
tracking_projects = ["org/tracker", "org/other-tracker"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.tracking_projects.len(), 2);
    assert!(config.is_tracking_project("org/tracker"));
    assert!(config.is_tracking_project("org/other-tracker"));
    assert!(!config.is_tracking_project("org/unrelated"));
    assert_eq!(config.primary_tracking_project(), "org/tracker");
}

#[test]
fn test_team_members_includes_me() {
    let toml_str = r#"
gitlab_url = "https://gitlab.com"
token = "test"
me = "binbin"
tracking_projects = ["org/repo"]

[[teams]]
name = "team"
members = ["alice", "bob"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let members = config.team_members(0);
    assert!(members.contains(&"alice".to_string()));
    assert!(members.contains(&"bob".to_string()));
    assert!(members.contains(&"binbin".to_string()));
}

#[test]
fn test_team_members_no_duplicate_me() {
    let toml_str = r#"
gitlab_url = "https://gitlab.com"
token = "test"
me = "alice"
tracking_projects = ["org/repo"]

[[teams]]
name = "team"
members = ["alice", "bob"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let members = config.team_members(0);
    assert_eq!(members.iter().filter(|m| *m == "alice").count(), 1);
}

#[test]
fn test_team_members_invalid_index() {
    let toml_str = r#"
gitlab_url = "https://gitlab.com"
token = "test"
me = "binbin"
tracking_projects = ["org/repo"]
teams = []
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let members = config.team_members(99);
    assert!(members.is_empty());
}
