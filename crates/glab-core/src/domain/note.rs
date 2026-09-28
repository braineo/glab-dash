//! The notes on an item, and the threads they hang in.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::User;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    /// GitLab's own id for the note, which is what an edit addresses.
    pub id: u64,
    pub body: String,
    pub author: User,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub system: bool,
    /// Whether GitLab lets this note's thread be resolved.  Only a merge
    /// request's notes ever are; an issue's are always `false`.
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
    /// The notes a reader is meant to see, in order — GitLab's own activity
    /// notes ("changed the description", "assigned to …") are not comments.
    pub fn comments(&self) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(|n| !n.system)
    }

    /// Whether the thread is resolved.  GitLab tracks this per note; a thread
    /// counts as resolved once every note that can be is.
    pub fn resolved(&self) -> bool {
        let mut resolvable = self.notes.iter().filter(|n| n.resolvable).peekable();
        resolvable.peek().is_some() && resolvable.all(|n| n.resolved)
    }

    /// Whether the thread can be resolved at all.
    pub fn resolvable(&self) -> bool {
        self.notes.iter().any(|n| n.resolvable)
    }
}
