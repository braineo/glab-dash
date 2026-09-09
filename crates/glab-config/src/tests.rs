use crate::Config;
use glab_core::filter::{Field, Op};

#[test]
fn test_parse_config() {
    let toml_str = r#"
gitlab_url = "https://gitlab.example.com"
token = "glpat-test"
me = "binbin"

[[teams]]
name = "frontend"
members = ["alice", "bob"]
tracking_projects = ["org/tracker"]

[[teams]]
name = "platform"
members = ["charlie", "dave"]
tracking_projects = ["org/tracker"]

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
    assert_eq!(config.all_tracking_projects(), vec!["org/tracker"]);
    assert!(config.is_tracking_project("org/tracker"));
    assert!(!config.is_tracking_project("other/repo"));
    assert_eq!(config.teams.len(), 2);
    assert_eq!(config.teams[0].name, "frontend");
    assert_eq!(config.teams[0].members, vec!["alice", "bob"]);
    assert_eq!(config.teams[1].name, "platform");
    assert_eq!(config.filters.len(), 1);
    assert_eq!(config.filters[0].name, "My issues");
    assert_eq!(config.filters[0].conditions[0].field, Field::Assignee);
    assert_eq!(config.filters[0].conditions[0].op, Op::Eq);
    assert_eq!(config.filters[0].conditions[0].value, "$me");
    assert_eq!(config.refresh_interval_secs, 60); // default
}

#[test]
fn test_parse_config_multi_project() {
    let toml_str = r#"
gitlab_url = "https://gitlab.com"
token = "test"
me = "binbin"

[[teams]]
name = "team"
members = ["alice"]
tracking_projects = ["org/tracker", "org/other-tracker"]
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.all_tracking_projects().len(), 2);
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

[[teams]]
name = "team"
members = ["alice", "bob"]
tracking_projects = ["org/repo"]
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

[[teams]]
name = "team"
members = ["alice", "bob"]
tracking_projects = ["org/repo"]
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
teams = []
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let members = config.team_members(99);
    assert_eq!(members, Vec::<String>::new());
}

/// The minimum a config needs, so a test can add just the section it is about.
fn with(section: &str) -> String {
    format!(
        r#"
gitlab_url = "https://gitlab.com"
token = "test"
me = "binbin"

[[teams]]
name = "team"
members = ["alice"]
tracking_projects = ["org/repo"]
{section}
"#
    )
}

#[test]
fn a_misspelled_field_is_rejected_rather_than_dropped() {
    let err = toml::from_str::<Config>(&with(
        r#"
[[filters]]
name = "Mine"
kind = "issue"

[[filters.conditions]]
field = "assignees"
op = "eq"
value = "$me"
"#,
    ))
    .unwrap_err();
    assert!(err.to_string().contains("assignees"), "{err}");
}

#[test]
fn a_misspelled_key_is_rejected_rather_than_ignored() {
    let err = toml::from_str::<Config>(&with("refresh_interval_sec = 30")).unwrap_err();
    assert!(err.to_string().contains("refresh_interval_sec"), "{err}");
}

#[test]
fn an_operator_parses_from_either_its_name_or_its_symbol() {
    let named = toml::from_str::<Config>(&with(
        r#"
[[filters]]
name = "Mine"
kind = "merge_request"

[[filters.conditions]]
field = "approved_by"
op = "not_contains"
value = "$me"
"#,
    ))
    .unwrap();
    let symbolic = toml::from_str::<Config>(&with(
        r#"
[[filters]]
name = "Mine"
kind = "merge_request"

[[filters.conditions]]
field = "approved_by"
op = "!~"
value = "$me"
"#,
    ))
    .unwrap();
    assert_eq!(named.filters[0].conditions[0].op, Op::NotContains);
    assert_eq!(symbolic.filters[0].conditions[0].op, Op::NotContains);
}

#[test]
fn a_sort_preset_may_leave_the_direction_out() {
    let config = toml::from_str::<Config>(&with(
        r#"
[[sort_presets]]
name = "Newest"
kind = "issue"
specs = [{ field = "updated_at" }]
"#,
    ))
    .unwrap();
    let spec = &config.sort_presets[0].specs[0];
    assert_eq!(spec.field, glab_core::sort::SortField::UpdatedAt);
    assert_eq!(spec.direction, glab_core::sort::SortDirection::Desc);
}

#[test]
fn label_orders_and_kanban_columns_parse_into_the_domain_shapes() {
    let config = toml::from_str::<Config>(&with(
        r#"
[[label_sort_orders]]
scope = "workflow"
values = ["todo", "doing", "done"]

[[kanban_columns]]
name = "Doing"
statuses = ["In Progress", "In Review"]
"#,
    ))
    .unwrap();
    assert_eq!(
        config.label_sort_orders.values("workflow"),
        ["todo", "doing", "done"]
    );
    assert!(config.kanban_columns[0].matches(Some("in progress")));
}

#[test]
fn a_generated_config_reads_back() {
    let config = toml::from_str::<Config>(&with("")).unwrap();
    let round_tripped: Config = toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
    assert_eq!(round_tripped.me, config.me);
}

/// Two teams share one namespace, a third has its own.
fn scoped_config() -> Config {
    toml::from_str(
        r#"
gitlab_url = "https://gitlab.example.com"
token = "t"
me = "binbin"

[[teams]]
name = "alpha"
members = ["alice"]
tracking_projects = ["org/shared"]

[[teams]]
name = "beta"
members = ["bob"]
tracking_projects = ["org/shared"]

[[teams]]
name = "gamma"
members = ["carol"]
tracking_projects = ["org/gamma"]
"#,
    )
    .unwrap()
}

#[test]
fn each_team_names_its_own_projects_and_all_spans_them() {
    let config = scoped_config();
    assert_eq!(config.team_tracking_projects(Some(0)), ["org/shared"]);
    assert_eq!(config.team_tracking_projects(Some(2)), ["org/gamma"]);
    assert_eq!(
        config.team_tracking_projects(None),
        ["org/shared", "org/gamma"]
    );
    assert_eq!(config.all_tracking_projects(), ["org/shared", "org/gamma"]);
    assert_eq!(config.team_tracking_group(Some(2)), "org");
}
