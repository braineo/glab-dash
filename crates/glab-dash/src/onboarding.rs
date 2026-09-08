use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

use glab_api::GitLabClient;
use glab_config::{Config, FilterPreset};
use glab_core::filter::{Field, FilterCondition, Op};
use glab_core::sort::label_order::LabelOrders;
use glab_core::team::Team;

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
    let client = GitLabClient::new(&gitlab_url, &token).context("Failed to create client")?;

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

    // Step 5: Teams
    println!();
    println!("  Now let's set up your teams. You can add more later in the config file.");
    println!("  Each team names the projects it tracks; teams sharing a board name the same ones.");
    let mut teams: Vec<Team> = Vec::new();

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

        // Most teams share the previous one's board, so offer it as the default.
        let previous = teams.last().map(|t| t.tracking_projects.join(", "));
        let prompt =
            format!("  Projects '{team_name}' tracks (comma-separated, e.g. myorg/team-tracker)");
        let projects_str = match &previous {
            Some(p) => prompt_with_default(&prompt, p)?,
            None => prompt_required(&prompt)?,
        };
        let tracking_projects: Vec<String> = projects_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if tracking_projects.is_empty() {
            println!("  No projects given. Skipping this team.");
            continue;
        }

        println!(
            "  Added team '{}' with {} members: {}",
            team_name,
            members.len(),
            members.join(", ")
        );
        teams.push(Team {
            name: team_name,
            members,
            tracking_projects,
        });
    }

    // Step 6: Generate config
    let config = Config {
        gitlab_url: gitlab_url.clone(),
        token: token.clone(),
        me: me.clone(),
        refresh_interval_secs: 60,
        teams: teams.clone(),
        filters: default_filter_presets(),
        sort_presets: Vec::new(),
        label_sort_orders: LabelOrders::default(),
        kanban_columns: Vec::new(),
        theme: None,
    };

    // Step 7: Write config file
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

/// One condition of a preset, spelled in the domain's own vocabulary.
fn condition(field: Field, op: Op, value: &str) -> FilterCondition {
    FilterCondition {
        field,
        op,
        value: value.to_string(),
    }
}

fn preset(name: &str, kind: &str, conditions: Vec<FilterCondition>) -> FilterPreset {
    FilterPreset {
        name: name.to_string(),
        kind: kind.to_string(),
        conditions,
    }
}

pub fn default_filter_presets() -> Vec<FilterPreset> {
    vec![
        preset(
            "My open issues",
            "issue",
            vec![
                condition(Field::Assignee, Op::Eq, "$me"),
                condition(Field::State, Op::Eq, "opened"),
            ],
        ),
        preset(
            "Unassigned issues",
            "issue",
            vec![condition(Field::Assignee, Op::Eq, "none")],
        ),
        preset(
            "My open MRs",
            "merge_request",
            vec![
                condition(Field::Author, Op::Eq, "$me"),
                condition(Field::State, Op::Eq, "opened"),
            ],
        ),
        preset(
            "Needs my review",
            "merge_request",
            vec![
                condition(Field::Reviewer, Op::Contains, "$me"),
                condition(Field::Draft, Op::Eq, "false"),
                condition(Field::ApprovedBy, Op::NotContains, "$me"),
            ],
        ),
        preset(
            "Ready to merge",
            "merge_request",
            vec![
                condition(Field::Draft, Op::Eq, "false"),
                condition(Field::State, Op::Eq, "opened"),
            ],
        ),
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
