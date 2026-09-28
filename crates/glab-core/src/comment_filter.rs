//! The config's `hide_comment` is a Lua chunk returning
//! `function(note) -> boolean`.  Only string, table and math are loaded, so a
//! config script can reach neither filesystem nor network.

use anyhow::{Result, anyhow};
use mlua::{Function, Lua, LuaOptions, StdLib};

use crate::domain::{Discussion, Note};

/// The username is padded with hyphens to match `bot` as a whole word, Lua
/// patterns having no alternation: `testing-bot` and `bot-testing` hide,
/// `robot` does not.
pub const DEFAULT: &str = r#"return function(note)
  local first = note.body:match("^%S+")
  return ("-" .. note.author .. "-"):match("%-bot%-") ~= nil
      or (first ~= nil and first:match("^/%a[%w_-]*$") ~= nil)
end
"#;

#[derive(Default)]
pub struct CommentFilter {
    script: Option<(Lua, Function)>,
}

impl CommentFilter {
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
            id: 1,
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
