use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub gitlab_url: String,
    pub token: String,
    pub me: String,
    pub tracking_projects: Vec<String>,
    #[serde(default = "default_refresh")]
    pub refresh_interval_secs: u64,
    #[serde(default)]
    pub teams: Vec<TeamConfig>,
    #[serde(default)]
    pub filters: Vec<FilterPreset>,
    #[serde(default)]
    pub sort_presets: Vec<SortPreset>,
    #[serde(default)]
    pub label_sort_orders: Vec<LabelSortOrderConfig>,
}

fn default_refresh() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterPreset {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub conditions: Vec<PresetCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetCondition {
    pub field: String,
    pub op: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortPreset {
    pub name: String,
    pub kind: String,
    pub specs: Vec<SortSpecConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortSpecConfig {
    pub field: String,
    #[serde(default = "default_desc")]
    pub direction: String,
    #[serde(default)]
    pub label_scope: Option<String>,
}

fn default_desc() -> String {
    "desc".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSortOrderConfig {
    pub scope: String,
    pub values: Vec<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            anyhow::bail!(
                "Config file not found at {}.\nCreate it with gitlab_url, token, me, tracking_projects, and teams.",
                path.display()
            );
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        let mut config: Config =
            toml::from_str(&contents).context("Failed to parse config TOML")?;

        // Environment variable overrides
        if let Ok(url) = std::env::var("GITLAB_URL") {
            config.gitlab_url = url;
        }
        if let Ok(token) = std::env::var("GITLAB_TOKEN") {
            config.token = token;
        }
        if let Ok(project) = std::env::var("GITLAB_PROJECT") {
            config.tracking_projects = vec![project];
        }

        if config.tracking_projects.is_empty() {
            anyhow::bail!("tracking_projects must not be empty");
        }

        Ok(config)
    }

    pub fn is_tracking_project(&self, path: &str) -> bool {
        self.tracking_projects.iter().any(|p| p == path)
    }

    /// The first tracking project (used as primary for iterations, statuses, etc.)
    pub fn primary_tracking_project(&self) -> &str {
        self.tracking_projects
            .first()
            .map_or("", |s| s.as_str())
    }

    pub fn all_members(&self) -> Vec<String> {
        let mut members: Vec<String> = self.teams.iter().flat_map(|t| t.members.clone()).collect();
        if !members.contains(&self.me) {
            members.push(self.me.clone());
        }
        members.sort();
        members.dedup();
        members
    }

    pub fn team_members(&self, team_idx: usize) -> Vec<String> {
        self.teams
            .get(team_idx)
            .map(|t| {
                let mut m = t.members.clone();
                if !m.contains(&self.me) {
                    m.push(self.me.clone());
                }
                m
            })
            .unwrap_or_default()
    }
}

fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("GLAB_DASH_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let config_dir = dirs::config_dir().context("Could not determine config directory")?;
    Ok(config_dir.join("glab-dash").join("config.toml"))
}
