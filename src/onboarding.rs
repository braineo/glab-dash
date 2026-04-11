use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::gitlab::client::GitLabClient;

const LOGO: &str = r"
   __ _  _       _             _           _
  / _` || |__ _ | |__  ___  __| | __ _ ___| |_
 | (_| || / _` || '_ \|___/ _` |/ _` |(_-<| ' \
  \__, ||_\__,_||_.__/   \__,_|\__,_|/__/|_||_|
  |___/
";

pub fn needs_onboarding() -> bool {
    match config_path() {
        Ok(p) => !p.exists(),
        Err(_) => true,
    }
}

pub async fn run_onboarding() -> Result<Config> {
    println!("{LOGO}");
    println!("  Welcome to glab-dash! Let's set up your configuration.\n");

    // Step 1: GitLab URL
    let gitlab_url = prompt_with_default("GitLab instance URL", "https://gitlab.com")?;

    // Step 2: Personal access token
    println!();
    println!("  Create a personal access token at:");
    println!("    {gitlab_url}/-/user_settings/personal_access_tokens");
    println!("  Required scopes: read_api, api");
    println!();
    let token = prompt_password("Personal access token (glpat-...)")?;

    // Step 3: Validate connection
    print!("\n  Validating connection... ");
    io::stdout().flush()?;
    let test_config = Config {
        gitlab_url: gitlab_url.clone(),
        token: token.clone(),
        me: String::new(),
        tracking_projects: Vec::new(),
        refresh_interval_secs: 60,
        teams: Vec::new(),
        filters: Vec::new(),
        sort_presets: Vec::new(),
        label_sort_orders: Vec::new(),
        kanban_columns: Vec::new(),
    };
    let client = GitLabClient::new(&test_config).context("Failed to create client")?;

    let username = fetch_current_user(&client).await;

    let detected_username = match username {
        Ok(u) => {
            println!("Connected as @{u}");
            u
        }
        Err(e) => {
            println!("Failed!");
            println!();
            println!("  Error: {e}");
            println!();
            println!("  Common causes:");
            println!("    - 401 Unauthorized: token is invalid or expired");
            println!("    - 403 Forbidden: token is missing the 'api' scope");
            println!("    - Connection error: wrong GitLab URL or network issue");
            println!();
            println!("  You can continue setup and fix the token later in the config file.");
            String::new()
        }
    };

    // Step 4: Username
    println!();
    let me = if detected_username.is_empty() {
        prompt_required("Your GitLab username")?
    } else {
        prompt_with_default("Your GitLab username", &detected_username)?
    };

    // Step 5: Tracking project
    println!();
    println!("  The tracking project is the main repo where your teams manage issues.");
    let tracking_project = prompt_required("Tracking project path (e.g. myorg/team-tracker)")?;

    // Step 6: Teams
    println!();
    println!("  Now let's set up your teams. You can add more later in the config file.");
    let mut teams = Vec::new();

    loop {
        println!();
        let team_name = prompt_optional(&format!(
            "Team {} name (or press Enter to finish)",
            teams.len() + 1
        ))?;
        if team_name.is_empty() {
            if teams.is_empty() {
                println!("  You need at least one team. Let's try again.");
                continue;
            }
            break;
        }

        let members_str = prompt_required(&format!(
            "  Members of '{team_name}' (comma-separated usernames)"
        ))?;
        let members: Vec<String> = members_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if members.is_empty() {
            println!("  No members added. Skipping this team.");
            continue;
        }

        println!(
            "  Added team '{}' with {} members: {}",
            team_name,
            members.len(),
            members.join(", ")
        );
        teams.push(crate::config::TeamConfig {
            name: team_name,
            members,
        });
    }

    // Step 7: Generate config
    let config = Config {
        gitlab_url: gitlab_url.clone(),
        token: token.clone(),
        me: me.clone(),
        tracking_projects: vec![tracking_project.clone()],
        refresh_interval_secs: 60,
        teams: teams.clone(),
        filters: default_filter_presets(),
        sort_presets: Vec::new(),
        label_sort_orders: Vec::new(),
        kanban_columns: Vec::new(),
    };

    // Step 8: Write config file
    let config_path = config_path()?;
    let toml_str = generate_toml(&config);

    println!("\n  Configuration preview:");
    println!("  ─────────────────────");
    for line in toml_str.lines() {
        println!("  {line}");
    }
    println!("  ─────────────────────");

    println!();
    let save = prompt_with_default(&format!("Save to {}? [Y/n]", config_path.display()), "Y")?;

    if save.to_lowercase() == "n" {
        println!("\n  Config not saved. You can create it manually at:");
        println!("    {}", config_path.display());
    } else {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(&config_path, &toml_str)
            .with_context(|| format!("Failed to write {}", config_path.display()))?;
        println!("\n  Config saved to {}", config_path.display());
    }

    println!("\n  Starting glab-dash...\n");

    Ok(config)
}

pub fn generate_toml(config: &Config) -> String {
    toml::to_string_pretty(config).expect("Config should be serializable to TOML")
}

pub fn default_filter_presets() -> Vec<crate::config::FilterPreset> {
    vec![
        crate::config::FilterPreset {
            name: "My open issues".to_string(),
            kind: "issue".to_string(),
            conditions: vec![
                crate::config::PresetCondition {
                    field: "assignee".to_string(),
                    op: "eq".to_string(),
                    value: "$me".to_string(),
                },
                crate::config::PresetCondition {
                    field: "state".to_string(),
                    op: "eq".to_string(),
                    value: "opened".to_string(),
                },
            ],
        },
        crate::config::FilterPreset {
            name: "Unassigned issues".to_string(),
            kind: "issue".to_string(),
            conditions: vec![crate::config::PresetCondition {
                field: "assignee".to_string(),
                op: "eq".to_string(),
                value: "none".to_string(),
            }],
        },
        crate::config::FilterPreset {
            name: "My open MRs".to_string(),
            kind: "merge_request".to_string(),
            conditions: vec![
                crate::config::PresetCondition {
                    field: "author".to_string(),
                    op: "eq".to_string(),
                    value: "$me".to_string(),
                },
                crate::config::PresetCondition {
                    field: "state".to_string(),
                    op: "eq".to_string(),
                    value: "opened".to_string(),
                },
            ],
        },
        crate::config::FilterPreset {
            name: "Needs my review".to_string(),
            kind: "merge_request".to_string(),
            conditions: vec![
                crate::config::PresetCondition {
                    field: "reviewer".to_string(),
                    op: "contains".to_string(),
                    value: "$me".to_string(),
                },
                crate::config::PresetCondition {
                    field: "draft".to_string(),
                    op: "eq".to_string(),
                    value: "false".to_string(),
                },
                crate::config::PresetCondition {
                    field: "approved_by".to_string(),
                    op: "not_contains".to_string(),
                    value: "$me".to_string(),
                },
            ],
        },
        crate::config::FilterPreset {
            name: "Ready to merge".to_string(),
            kind: "merge_request".to_string(),
            conditions: vec![
                crate::config::PresetCondition {
                    field: "draft".to_string(),
                    op: "eq".to_string(),
                    value: "false".to_string(),
                },
                crate::config::PresetCondition {
                    field: "state".to_string(),
                    op: "eq".to_string(),
                    value: "opened".to_string(),
                },
            ],
        },
    ]
}

async fn fetch_current_user(client: &GitLabClient) -> Result<String> {
    // Use the /user endpoint to get the authenticated user
    let user: serde_json::Value = client.get_authenticated_user().await?;
    user.get("username")
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
        .context("No username in response")
}

fn prompt_with_default(prompt: &str, default: &str) -> Result<String> {
    print!("  {prompt} [{default}]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input.to_string())
    }
}

fn prompt_required(prompt: &str) -> Result<String> {
    loop {
        print!("  {prompt}: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        let input = input.trim().to_string();
        if !input.is_empty() {
            return Ok(input);
        }
        println!("  This field is required.");
    }
}

fn prompt_password(prompt: &str) -> Result<String> {
    loop {
        print!("  {prompt}: ");
        io::stdout().flush()?;
        let input = rpassword::read_password().context("Failed to read password")?;
        let input = input.trim().to_string();
        if !input.is_empty() {
            return Ok(input);
        }
        println!("  This field is required.");
    }
}

fn prompt_optional(prompt: &str) -> Result<String> {
    print!("  {prompt}: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("GLAB_DASH_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let config_dir = dirs::config_dir().context("Could not determine config directory")?;
    Ok(config_dir.join("glab-dash").join("config.toml"))
}
