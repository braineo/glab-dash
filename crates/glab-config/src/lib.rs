//! `config.toml`, deserialized straight into the domain's own types.  There is
//! no second, stringly-typed shape to convert from.

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use glab_core::filter::FilterCondition;
use glab_core::kanban::KanbanColumn;
use glab_core::sort::SortSpec;
use glab_core::sort::label_order::LabelOrders;
use glab_core::team::Team;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub gitlab_url: String,
    pub token: String,
    pub me: String,
    #[serde(default = "default_refresh")]
    pub refresh_interval_secs: u64,
    #[serde(default)]
    pub teams: Vec<Team>,
    #[serde(default)]
    pub filters: Vec<FilterPreset>,
    #[serde(default)]
    pub sort_presets: Vec<SortPreset>,
    #[serde(default)]
    pub label_sort_orders: LabelOrders,
    #[serde(default)]
    pub kanban_columns: Vec<KanbanColumn>,
    /// A Lua chunk returning `function(note) -> boolean`.
    #[serde(default)]
    pub hide_comment: Option<String>,
}

fn default_refresh() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterPreset {
    pub name: String,
    /// Which list the preset applies to: `issue` or `merge_request`.
    pub kind: String,
    #[serde(default)]
    pub conditions: Vec<FilterCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortPreset {
    pub name: String,
    /// Which list the preset applies to: `issue` or `merge_request`.
    pub kind: String,
    pub specs: Vec<SortSpec>,
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

        if let Ok(url) = std::env::var("GITLAB_URL") {
            config.gitlab_url = url;
        }
        if let Ok(token) = std::env::var("GITLAB_TOKEN") {
            config.token = token;
        }
        if config.teams.is_empty() {
            anyhow::bail!("at least one team must be configured");
        }
        if let Some(team) = config.teams.iter().find(|t| t.tracking_projects.is_empty()) {
            anyhow::bail!("team '{}' has no tracking_projects", team.name);
        }

        Ok(config)
    }

    pub fn is_tracking_project(&self, path: &str) -> bool {
        self.teams
            .iter()
            .any(|t| t.tracking_projects.iter().any(|p| p == path))
    }

    /// One fetch covers all teams, so switching teams filters rather than
    /// reloads.
    pub fn all_tracking_projects(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        for team in &self.teams {
            for p in &team.tracking_projects {
                if !all.contains(p) {
                    all.push(p.clone());
                }
            }
        }
        all
    }

    /// `None` spans every team's.
    pub fn team_tracking_projects(&self, team: Option<usize>) -> Vec<String> {
        match team.and_then(|i| self.teams.get(i)) {
            Some(t) => t.tracking_projects.clone(),
            None => self.all_tracking_projects(),
        }
    }

    /// For what needs a single project: statuses, the debug dump.
    pub fn primary_tracking_project(&self) -> &str {
        self.teams
            .first()
            .and_then(|t| t.tracking_projects.first())
            .map_or("", String::as_str)
    }

    /// Everything before the last `/`: iterations are defined on the group,
    /// not the project.
    pub fn team_tracking_group(&self, team: Option<usize>) -> &str {
        let primary = team
            .and_then(|i| self.teams.get(i))
            .and_then(|t| t.tracking_projects.first())
            .map_or_else(|| self.primary_tracking_project(), String::as_str);
        primary.rsplit_once('/').map_or(primary, |(group, _)| group)
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
        self.teams.get(team_idx).map_or_default(|t| {
            let mut m = t.members.clone();
            if !m.contains(&self.me) {
                m.push(self.me.clone());
            }
            m
        })
    }
}

fn config_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("GLAB_DASH_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let config_dir = dirs::config_dir().context("Could not determine config directory")?;
    Ok(config_dir.join("glab-dash").join("config.toml"))
}
