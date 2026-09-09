//! Which comments are worth reading, and which are noise the reader asked to
//! never see again.
//!
//! The reader decides, in the config's `hide_comment` — a Lua chunk returning
//! `function(note) -> boolean`.  Bot accounts, what the bot wrote, and the
//! quick-action comments that exist to trigger a workflow rather than to say
//! anything are all the same one rule:
//!
//! [`DEFAULT`] is what a generated config starts with.
//!
//! The interpreter is loaded with the string, table and math libraries and
//! nothing else, so a config script cannot reach the filesystem or the network
//! however it is written.

use anyhow::{Result, anyhow};
use mlua::{Function, Lua, LuaOptions, StdLib};

use crate::domain::{Discussion, Note};

/// The rule a generated config ships with: bot accounts, and comments whose
/// first word is a quick action (`/test --some-param`).  Testing the first word
/// whole is what keeps a comment opening on `/etc/hosts` — a path, not a
/// command — in the conversation.
///
/// Padding the username with hyphens is how `bot` is matched as a whole word
/// wherever it sits, Lua patterns having no alternation: `testing-bot` and
/// `bot-testing` both hide, `robot` does not.
pub const DEFAULT: &str = r#"return function(note)
  local first = note.body:match("^%S+")
  return ("-" .. note.author .. "-"):match("%-bot%-") ~= nil
      or (first ~= nil and first:match("^/%a[%w_-]*$") ~= nil)
end
"#;

/// The reader's comment rule, compiled once.  The default hides nothing, which
/// is what an empty config means.
#[derive(Default)]
pub struct CommentFilter {
    /// The compiled `hide_comment` predicate, and the state it lives in.
    script: Option<(Lua, Function)>,
}

impl CommentFilter {
    /// Compile `src` — the config's `hide_comment` — failing on a chunk that
    /// will not compile or does not evaluate to a function.  A broken predicate
    /// is worth saying out loud rather than quietly passing every comment
    /// through.
    // ponytail: no instruction-count hook, so `while true do end` in the config
    // hangs whoever calls `visible_threads`.  Add `Lua::set_hook` if that bites.
    pub fn new(src: &str) -> Result<Self> {
        let lua = Lua::new_with(
            StdLib::STRING | StdLib::TABLE | StdLib::MATH,
            LuaOptions::default(),
        )
        .map_err(|e| anyhow!("failed to start the Lua interpreter: {e}"))?;
        let predicate = lua
            .load(src)
            .set_name("hide_comment")
            .eval::<Function>()
            .map_err(|e| anyhow!("hide_comment must be a chunk returning function(note): {e}"))?;
        Ok(Self {
            script: Some((lua, predicate)),
        })
    }

    /// The threads worth reading: hidden notes are dropped, and a thread left
    /// with no comment goes with them.
    pub fn visible_threads(&self, discussions: Vec<Discussion>) -> Vec<Discussion> {
        discussions
            .into_iter()
            .filter_map(|mut d| {
                d.notes.retain(|n| !self.hides(n));
                let any = d.comments().next().is_some();
                any.then_some(d)
            })
            .collect()
    }

    /// Whether this note should stay out of the conversation.  A script that
    /// raises keeps the note: the reader is better off seeing bot chatter than
    /// silently losing a comment to a typo in their config.
    pub fn hides(&self, note: &Note) -> bool {
        let Some((lua, predicate)) = &self.script else {
            return false;
        };
        match note_table(lua, note).and_then(|t| predicate.call::<bool>(t)) {
            Ok(hide) => hide,
            Err(e) => {
                tracing::warn!("hide_comment failed: {e}");
                false
            }
        }
    }
}

/// The note as the script sees it.
fn note_table(lua: &Lua, note: &Note) -> mlua::Result<mlua::Table> {
    let t = lua.create_table()?;
    t.set("author", note.author.username.as_str())?;
    t.set("body", note.body.as_str())?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::CommentFilter;
    use crate::domain::{Discussion, Note, User};
    use chrono::Utc;

    fn note(author: &str, body: &str) -> Note {
        Note {
            body: body.to_string(),
            author: User {
                id: "1".into(),
                username: author.to_string(),
            },
            created_at: Utc::now(),
            system: false,
            resolvable: false,
            resolved: false,
        }
    }

    fn thread(id: &str, notes: Vec<Note>) -> Discussion {
        Discussion {
            id: id.to_string(),
            notes,
        }
    }

    #[test]
    fn drops_bot_threads_and_quick_actions_but_keeps_paths() {
        let filter = CommentFilter::new(super::DEFAULT).unwrap();
        let threads = vec![
            thread("bot", vec![note("gitlab-bot", "e2e results: all green")]),
            thread("prefix", vec![note("bot-testing", "coverage dropped")]),
            thread("robot", vec![note("robotnik", "not a bot account")]),
            thread("cmd", vec![note("alice", "/test --some-param --other")]),
            thread(
                "mixed",
                vec![
                    note("gitlab-bot", "pipeline passed"),
                    note("alice", "looks good"),
                ],
            ),
            thread("path", vec![note("alice", "/etc/hosts needs the entry")]),
            thread("prose", vec![note("alice", "see /etc/hosts\nand fix it")]),
        ];
        let kept = filter.visible_threads(threads);
        let ids: Vec<&str> = kept.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["robot", "mixed", "path", "prose"]);
        assert_eq!(kept[0].notes.len(), 1);
    }

    #[test]
    fn the_default_filter_hides_nothing() {
        assert!(!CommentFilter::default().hides(&note("gitlab-bot", "/test")));
    }

    #[test]
    fn a_broken_script_is_an_error_and_a_raising_one_keeps_the_note() {
        assert!(CommentFilter::new("this is not lua").is_err());
        let filter = CommentFilter::new("return function(note) error('boom') end").unwrap();
        assert!(!filter.hides(&note("alice", "hi")));
    }

    #[test]
    fn the_sandbox_has_no_filesystem() {
        let filter =
            CommentFilter::new("return function(note) return io.open('/etc/passwd') ~= nil end")
                .unwrap();
        assert!(!filter.hides(&note("alice", "hi")));
    }
}
