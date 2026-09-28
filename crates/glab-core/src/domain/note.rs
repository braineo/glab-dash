use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::User;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub body: String,
    pub author: User,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub system: bool,
    /// Always `false` on an issue's notes: GitLab resolves MR threads only.
    #[serde(default)]
    pub resolvable: bool,
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discussion {
    pub id: String,
    pub notes: Vec<Note>,
}

impl Discussion {
    /// GitLab's activity notes ("changed the description", "assigned to …")
    /// are mixed in with the real comments and dropped here.
    pub fn comments(&self) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(|n| !n.system)
    }

    /// GitLab tracks resolution per note, not per thread.
    pub fn resolved(&self) -> bool {
        let mut resolvable = self.notes.iter().filter(|n| n.resolvable).peekable();
        resolvable.peek().is_some() && resolvable.all(|n| n.resolved)
    }

    pub fn resolvable(&self) -> bool {
        self.notes.iter().any(|n| n.resolvable)
    }
}
