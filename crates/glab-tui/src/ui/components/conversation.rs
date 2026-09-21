//! The comment threads on an issue or merge request, as rows in a
//! [`DetailBody`].
//!
//! Every row records the thread and the note it belongs to, so the key that
//! replies, edits or resolves reads its subject off the row under the cursor.
//! A thread holds its rail down every row it owns; a reply is inset past it but
//! keeps it, so the thread reads as one block.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use glab_core::domain::{Discussion, Note};

use crate::app::Overlay;
use crate::cmd::EventResult;
use crate::keybindings::KeyAction;
use crate::ui::components::detail_body::{self, DetailBody, Row};
use crate::ui::components::input::{CommentInput, CommentTarget};
use crate::ui::components::status_bar::format_span;
use crate::ui::{markdown, styles};

/// A thread's rail, heavier than the body's own spine.
const RAIL: &str = "\u{258E}";
/// The elbow that opens a reply under the note it answers.  Exactly as wide as
/// [`REPLY_INSET`], so a reply's own body lines up under its author row.
const REPLY_ELBOW: &str = "\u{2570}\u{2500}";
/// Columns a reply is inset past its root.
const REPLY_INSET: usize = 2;

/// The threads on the open item, and what the reader has folded away.
#[derive(Default)]
pub struct Conversation {
    pub discussions: Vec<Discussion>,
    pub loading: bool,
    /// Threads the reader has flipped away from their default fold: an open
    /// thread starts expanded and a resolved one starts collapsed, so an id in
    /// here means the reader asked for the opposite.
    folded: HashSet<String>,
}

impl Conversation {
    /// Bubbles anything else, resolving included: that needs the API client.
    pub fn handle_key(
        &mut self,
        action: KeyAction,
        body: &mut DetailBody,
        overlay: &mut Overlay,
    ) -> EventResult {
        match action {
            KeyAction::NextUnresolved => self.jump_unresolved(body, true),
            KeyAction::PrevUnresolved => self.jump_unresolved(body, false),
            KeyAction::ToggleThread => self.toggle_fold(body),
            KeyAction::ReplyThread => *overlay = draft_reply(self, body),
            KeyAction::NewThread => *overlay = draft_new_thread(),
            KeyAction::EditComment => {
                if let Some(draft) = draft_edit(self, body) {
                    *overlay = draft;
                }
            }
            _ => return EventResult::Bubble,
        }
        EventResult::Consumed
    }

    pub fn reset(&mut self) {
        self.discussions.clear();
        self.loading = false;
        self.folded.clear();
    }

    /// The cursor belongs to the body, so a reply landing leaves it alone.
    pub fn set_discussions(&mut self, discussions: Vec<Discussion>) {
        self.discussions = discussions;
        self.loading = false;
    }

    /// Everyone who has spoken on this item, for `@` completion — a thread
    /// pulls in people who are on no team and so are in no config.
    pub fn participants(&self) -> impl Iterator<Item = &str> {
        self.discussions
            .iter()
            .flat_map(|d| d.comments().map(|n| n.author.username.as_str()))
    }

    /// `None` on every row that belongs to no thread.
    pub fn thread_at_cursor(&self, body: &DetailBody) -> Option<&Discussion> {
        let Row::Thread { thread, .. } = body.cursor_row() else {
            return None;
        };
        self.discussions.get(thread)
    }

    /// The note the cursor is in, wrapped body rows included.
    pub fn note_at_cursor(&self, body: &DetailBody) -> Option<&Note> {
        let Row::Thread { thread, note, .. } = body.cursor_row() else {
            return None;
        };
        self.discussions.get(thread)?.comments().nth(note)
    }

    /// Jump to the row opening the next unresolved thread in `dir`, staying put
    /// when there is none that way.  On an issue nothing is resolvable, so every
    /// thread counts and this walks thread to thread.
    fn jump_unresolved(&self, body: &mut DetailBody, down: bool) {
        body.jump(down, |row| {
            matches!(row, Row::Thread { thread, head: true, .. }
                if !self.discussions[thread].resolved())
        });
    }

    /// Fold the thread under the cursor away, or open it back up.
    fn toggle_fold(&mut self, body: &DetailBody) {
        if let Some(id) = self.thread_at_cursor(body).map(|d| d.id.clone())
            && !self.folded.remove(&id)
        {
            self.folded.insert(id);
        }
    }

    /// Whether the thread shows only the row naming its first note.  A resolved
    /// thread is folded by default, since it is settled; either can be flipped.
    fn is_folded(&self, disc: &Discussion) -> bool {
        disc.resolved() != self.folded.contains(&disc.id)
    }
}

/// The row naming the first note of the thread the cursor is in.
fn head_row_of_cursor(body: &DetailBody) -> Option<usize> {
    let Row::Thread { thread, .. } = body.cursor_row() else {
        return None;
    };
    (0..=body.cursor()).rev().find(|&r| {
        matches!(
            body.rows().get(r),
            Some(Row::Thread { thread: t, head: true, .. }) if *t == thread
        )
    })
}

/// Every thread holding something a reader is meant to see.
pub fn push(body: &mut DetailBody, conv: &Conversation, width: usize) {
    let threads: Vec<usize> = (0..conv.discussions.len())
        .filter(|&i| conv.discussions[i].comments().next().is_some())
        .collect();

    if conv.loading {
        body.section("CONVERSATION", Some("loading".to_string()), width);
        return;
    }
    if threads.is_empty() {
        body.section("CONVERSATION", Some("no comments yet".to_string()), width);
        return;
    }

    let unresolved = threads
        .iter()
        .filter(|&&i| conv.discussions[i].resolvable() && !conv.discussions[i].resolved())
        .count();
    let count = threads.len();
    let open = if unresolved > 0 {
        format!(" \u{00B7} {unresolved} unresolved")
    } else {
        String::new()
    };
    let tally = format!("{count} thread{}{open}", detail_body::plural(count));
    body.section("CONVERSATION", Some(tally), width);

    for &i in &threads {
        push_thread(body, conv, i, width);
    }
}

/// The row naming the first note, its body, then each reply inset under it.  A
/// folded thread stops after the first row.
fn push_thread(body: &mut DetailBody, conv: &Conversation, index: usize, width: usize) {
    let disc = &conv.discussions[index];
    let comments: Vec<&Note> = disc.comments().collect();
    let Some((root, replies)) = comments.split_first() else {
        return;
    };
    let resolved = disc.resolved();
    let folded = conv.is_folded(disc);
    // The rail carries the thread's state.
    let rail = Span::styled(
        RAIL,
        Style::default().fg(if resolved {
            styles::green()
        } else {
            styles::border_active()
        }),
    );

    let mut head = head_spans(root, resolved);
    if folded && !replies.is_empty() {
        head.push(Span::styled(
            format!("  {} {} more", styles::ICON_ARROW, replies.len()),
            Style::default().fg(styles::text_dim()),
        ));
    }
    let head_row = Row::Thread {
        thread: index,
        note: 0,
        head: true,
    };
    // Each row carries its note's index, so `e` edits the right one.
    let body_row = |note: usize| Row::Thread {
        thread: index,
        note,
        head: false,
    };
    body.push(
        head_row,
        detail_body::indented(std::slice::from_ref(&rail), Line::from(head)),
    );
    if folded {
        return;
    }

    // Bodies render into the room the chrome leaves, then take it on every
    // row they wrapped onto.
    let render_body = |note: &Note, inset: usize| {
        detail_body::trim_blanks(markdown::render(
            note.body.trim_end(),
            "",
            width.saturating_sub(detail_body::LEAD + inset),
        ))
    };
    for line in render_body(root, 0) {
        body.push(
            body_row(0),
            detail_body::indented(std::slice::from_ref(&rail), line),
        );
    }
    for (i, reply) in replies.iter().enumerate() {
        let row = body_row(i + 1);
        let elbow = Span::styled(REPLY_ELBOW, Style::default().fg(styles::text_dim()));
        body.push(
            row,
            detail_body::indented(&[rail.clone(), elbow], Line::from(head_spans(reply, false))),
        );
        let inset = Span::raw(" ".repeat(REPLY_INSET));
        for line in render_body(reply, REPLY_INSET) {
            body.push(
                row,
                detail_body::indented(&[rail.clone(), inset.clone()], line),
            );
        }
    }
}

/// The overlay that drafts a reply into the thread under the cursor.
///
/// A standalone comment takes a reply as readily as a thread does — GitLab turns
/// it into one when the first reply lands.  Only with the cursor off every
/// thread entirely does this fall back to drafting a new one.
pub fn draft_reply(conv: &Conversation, body: &DetailBody) -> Overlay {
    let target = conv
        .thread_at_cursor(body)
        .map_or(CommentTarget::NewThread, |d| {
            CommentTarget::Reply(d.id.clone())
        });
    Overlay::CommentInput {
        input: CommentInput::default(),
        autocomplete: Box::default(),
        target,
    }
}

/// The overlay that rewrites the note under the cursor, opened with its current
/// text.  Whether the reader may edit it is GitLab's call, so this offers the
/// draft and lets the API refuse it.
pub fn draft_edit(conv: &Conversation, body: &DetailBody) -> Option<Overlay> {
    let note = conv.note_at_cursor(body)?;
    Some(Overlay::CommentInput {
        input: CommentInput::with_text(&note.body),
        autocomplete: Box::default(),
        target: CommentTarget::Edit(note.id),
    })
}

/// The overlay that drafts a new top-level thread.
pub fn draft_new_thread() -> Overlay {
    Overlay::CommentInput {
        input: CommentInput::default(),
        autocomplete: Box::default(),
        target: CommentTarget::NewThread,
    }
}

/// Float the opening note of the cursor's thread over the pane once its row has
/// scrolled off, so the reader still sees what the replies answer.
///
/// Built from the note rather than from the row that renders it: the row carries
/// the thread's rail, which inside the card would read as a second thread.
pub fn render_sticky_head(frame: &mut Frame, area: Rect, conv: &Conversation, body: &DetailBody) {
    let scrolled_off = head_row_of_cursor(body).is_some_and(|head| head < body.offset());
    if !scrolled_off {
        return;
    }
    let Some(thread) = conv.thread_at_cursor(body) else {
        return;
    };
    let Some(root) = thread.comments().next() else {
        return;
    };

    let card = Rect {
        height: area.height.min(2),
        ..area
    };
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(styles::border_active()))
        .style(Style::default().bg(styles::overlay()));
    let inner = block.inner(card);
    frame.render_widget(Clear, card);
    frame.render_widget(block, card);

    let mut spans = vec![Span::styled(
        " \u{25B2} ",
        Style::default().fg(styles::border_active()),
    )];
    spans.extend(head_spans(root, thread.resolved()));
    // The opening line of the body, so the card says what the thread is about
    // and not merely who started it.
    if let Some(opening) = root.body.lines().find(|l| !l.trim().is_empty()) {
        spans.push(Span::styled(
            format!("  \u{00B7}  {}", opening.trim()),
            Style::default().fg(styles::overlay_text_dim()),
        ));
    }
    let line = detail_body::fill(
        Line::from(spans),
        usize::from(inner.width),
        Style::default().bg(styles::overlay()),
    );
    frame.render_widget(Paragraph::new(line), inner);
}

/// The row naming a note: who wrote it, how long ago, and whether it is settled.
fn head_spans(note: &Note, resolved: bool) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(
            format!("@{}", note.author.username),
            Style::default()
                .fg(styles::cyan())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {}", ago(note.created_at)),
            Style::default().fg(styles::text_dim()),
        ),
    ];
    if resolved {
        spans.push(Span::styled(
            format!("  {} resolved", styles::ICON_CHECK),
            Style::default().fg(styles::green()),
        ));
    }
    spans
}

fn ago(at: DateTime<Utc>) -> String {
    let secs = Utc::now().signed_duration_since(at).num_seconds();
    format_span(u64::try_from(secs).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use glab_core::domain::{Discussion, Note, User};

    use super::{CommentTarget, Conversation, DetailBody, Overlay, Row};
    use crate::keybindings::KeyAction;
    use crate::ui::components::detail_body::band;

    fn user(name: &str) -> User {
        User {
            id: name.to_string(),
            username: name.to_string(),
        }
    }

    fn note(author: &str, body: &str, resolvable: bool, resolved: bool) -> Note {
        Note {
            id: 1,
            body: body.to_string(),
            author: user(author),
            created_at: Utc::now(),
            system: false,
            resolvable,
            resolved,
        }
    }

    fn thread(id: &str, notes: Vec<Note>) -> Discussion {
        Discussion {
            id: id.to_string(),
            notes,
        }
    }

    /// The discussion GitLab returns for a plain one-off comment.
    fn standalone(id: &str, note: Note) -> Discussion {
        thread(id, vec![note])
    }

    /// Filled the way a detail view fills one.
    fn body(conv: &Conversation, description: Option<&str>, width: usize) -> DetailBody {
        let mut body = DetailBody::default();
        rebuild(&mut body, conv, description, width);
        body
    }

    fn rebuild(
        body: &mut DetailBody,
        conv: &Conversation,
        description: Option<&str>,
        width: usize,
    ) {
        body.begin();
        body.description(description, width);
        super::push(body, conv, width);
    }

    /// The thread a drafted reply is addressed to, or `None` when it drafts a
    /// new one.
    fn reply_target(conv: &Conversation, body: &DetailBody) -> Option<String> {
        match super::draft_reply(conv, body) {
            Overlay::CommentInput { target, .. } => match target {
                CommentTarget::Reply(id) => Some(id),
                CommentTarget::NewThread => None,
                CommentTarget::Edit(_) => panic!("reply should never edit"),
            },
            _ => panic!("reply should draft a comment"),
        }
    }

    /// Reply addresses the thread under the cursor even when that thread is a
    /// lone comment with no replies yet — GitLab turns one into a thread when
    /// the first reply lands, so there is nothing to refuse.
    #[test]
    fn reply_addresses_a_lone_comment_rather_than_starting_a_new_thread() {
        let conv = Conversation {
            discussions: vec![
                standalone("d1", note("alice", "a single comment", false, false)),
                thread("d2", vec![note("bob", "in a thread", false, false)]),
            ],
            ..Conversation::default()
        };
        let mut body = body(&conv, Some("the description"), 60);

        for (index, id) in [(0usize, "d1"), (1, "d2")] {
            let rows: Vec<usize> = (0..body.rows().len())
                .filter(|&r| matches!(body.rows()[r], Row::Thread { thread: t, .. } if t == index))
                .collect();
            assert!(!rows.is_empty(), "thread {index} has no rows");
            for row in rows {
                body.set_cursor(row);
                assert_eq!(
                    reply_target(&conv, &body).as_deref(),
                    Some(id),
                    "row {row} should reply into {id}"
                );
            }
        }

        // Only the description, which is no thread at all, drafts a new one.
        let description = body
            .rows()
            .iter()
            .position(|r| *r == Row::Description)
            .expect("the description has a row");
        body.set_cursor(description);
        assert_eq!(reply_target(&conv, &body), None);
    }

    /// Two threads, the second resolved, with a reply on the first.
    fn conversation() -> Conversation {
        Conversation {
            discussions: vec![
                thread(
                    "d1",
                    vec![
                        note("alice", "why is the runner full?", false, false),
                        note("bob", "the layer cache never expires", false, false),
                    ],
                ),
                thread("d2", vec![note("carol", "raised the quota", true, true)]),
            ],
            ..Conversation::default()
        }
    }

    #[test]
    fn every_row_of_a_thread_answers_with_that_thread() {
        let conv = conversation();
        let mut body = body(&conv, None, 60);
        let threads: Vec<Option<&str>> = body
            .rows()
            .iter()
            .map(|row| match row {
                Row::Thread { thread, .. } => Some(conv.discussions[*thread].id.as_str()),
                Row::Chrome | Row::Description | Row::Related(_) => None,
            })
            .collect();
        assert!(
            threads.iter().filter(|t| **t == Some("d1")).count() >= 4,
            "{:?} in {:?}",
            threads,
            body.text()
        );
        for (row, expected) in threads.iter().enumerate() {
            body.set_cursor(row);
            assert_eq!(
                conv.thread_at_cursor(&body).map(|d| d.id.as_str()),
                *expected,
                "row {row}: {:?}",
                body.text()[row]
            );
        }
    }

    /// Every row a note owns answers with that note — the wrapped tail of a
    /// body as much as the row naming its author — so `e` edits the comment the
    /// cursor is in rather than whichever one opened the thread.
    #[test]
    fn every_row_of_a_note_answers_with_that_note() {
        let conv = Conversation {
            discussions: vec![thread(
                "d1",
                vec![
                    Note {
                        id: 10,
                        ..note(
                            "alice",
                            "the root, long enough that it has to wrap onto a second row",
                            false,
                            false,
                        )
                    },
                    Note {
                        id: 11,
                        ..note("bob", "a reply", false, false)
                    },
                ],
            )],
            ..Conversation::default()
        };
        let mut body = body(&conv, None, 36);
        let text = body.text();
        let row_with = |needle: &str| {
            text.iter()
                .position(|r| r.contains(needle))
                .unwrap_or_else(|| panic!("no row holds {needle:?}: {text:?}"))
        };

        // The root's wrapped tail names nobody, and still answers with the root.
        body.set_cursor(row_with("second row"));
        assert_eq!(conv.note_at_cursor(&body).map(|n| n.id), Some(10));

        // And the draft `e` opens on the reply is addressed to the reply,
        // holding its text rather than the root's.
        body.set_cursor(row_with("a reply"));
        match super::draft_edit(&conv, &body) {
            Some(Overlay::CommentInput { input, target, .. }) => {
                assert_eq!(target, CommentTarget::Edit(11));
                assert_eq!(input.text(), "a reply");
            }
            _ => panic!("the reply should draft an edit"),
        }
    }

    /// A reply keeps its root's rail, so the thread stays one connected block,
    /// and every row of a wrapped body keeps it too.
    #[test]
    fn a_reply_is_inset_but_keeps_the_rail() {
        let conv = Conversation {
            discussions: vec![thread(
                "d1",
                vec![
                    note("alice", "root", false, false),
                    note(
                        "bob",
                        "a reply long enough that it has to wrap onto a second row",
                        false,
                        false,
                    ),
                ],
            )],
            ..Conversation::default()
        };
        let body = body(&conv, None, 36);
        let rows: Vec<String> = body
            .text()
            .into_iter()
            .filter(|r| r.contains("wrap") || r.contains("second") || r.contains("@bob"))
            .collect();
        assert!(rows.len() >= 2, "{rows:?} should have wrapped");
        for row in &rows {
            assert!(row.starts_with(super::RAIL), "{row:?} lost the rail");
        }
        assert!(
            rows[0].contains(super::REPLY_ELBOW),
            "{rows:?} lost the elbow"
        );
    }

    /// Folding a thread away pulls the view back so the last row still sits at
    /// the bottom of the pane, rather than stranding it past the end.
    #[test]
    fn folding_a_thread_away_does_not_strand_the_view_past_the_end() {
        let mut conv = conversation();
        let mut body = body(&conv, None, 60);
        body.handle_key(KeyAction::Bottom);
        body.scroll_into_view(4);
        assert_eq!(body.offset(), body.rows().len() - 4);

        let open = body
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Thread { thread: 0, .. }))
            .expect("the open thread has a row");
        body.set_cursor(open);
        conv.toggle_fold(&body);
        rebuild(&mut body, &conv, None, 60);
        body.scroll_into_view(4);
        assert!(
            body.offset() + 4 <= body.rows().len().max(4),
            "offset {} strands {} rows",
            body.offset(),
            body.rows().len()
        );
    }

    /// A resolved thread arrives folded to its opening row; the fold key opens
    /// it, and closes an open one.
    #[test]
    fn a_resolved_thread_starts_folded_and_the_key_flips_either_way() {
        let mut conv = conversation();
        let mut body = body(&conv, None, 60);
        assert!(
            !body.text().iter().any(|r| r.contains("raised the quota")),
            "{:?}",
            body.text()
        );

        // Land on the resolved thread's only row and open it.
        let resolved = body
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Thread { thread: 1, .. }))
            .expect("the resolved thread has a row");
        body.set_cursor(resolved);
        conv.toggle_fold(&body);
        rebuild(&mut body, &conv, None, 60);
        assert!(
            body.text().iter().any(|r| r.contains("raised the quota")),
            "{:?}",
            body.text()
        );

        // The same key folds the open thread away.
        let open = body
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Thread { thread: 0, .. }))
            .expect("the open thread has a row");
        body.set_cursor(open);
        conv.toggle_fold(&body);
        rebuild(&mut body, &conv, None, 60);
        assert!(
            !body.text().iter().any(|r| r.contains("layer cache")),
            "{:?}",
            body.text()
        );
    }

    /// The cursor's thread head is what the floating card shows, found by
    /// walking back to where the thread's rows start.
    #[test]
    fn the_head_row_of_the_cursor_opens_its_thread() {
        let conv = conversation();
        let mut body = body(&conv, None, 60);
        let first = body
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Thread { thread: 0, .. }))
            .expect("the thread has rows");
        let last = body
            .rows()
            .iter()
            .rposition(|r| matches!(r, Row::Thread { thread: 0, .. }))
            .expect("the thread has rows");
        assert!(last > first);
        for row in first..=last {
            body.set_cursor(row);
            assert_eq!(super::head_row_of_cursor(&body), Some(first), "row {row}");
        }
        // Chrome belongs to no thread, so there is nothing to float.
        body.set_cursor(0);
        assert_eq!(super::head_row_of_cursor(&body), None);
    }

    /// `J` and `K` land on the row opening a thread and pass over a resolved
    /// one, so they walk what still needs an answer.
    #[test]
    fn unresolved_jumps_skip_a_settled_thread() {
        let conv = Conversation {
            discussions: vec![
                thread("open-1", vec![note("alice", "why?", true, false)]),
                thread("settled", vec![note("bob", "fixed", true, true)]),
                thread("open-2", vec![note("carol", "and this?", true, false)]),
            ],
            ..Conversation::default()
        };
        let mut body = body(&conv, None, 60);

        body.handle_key(KeyAction::Top);
        assert_eq!(
            conv.thread_at_cursor(&body).map(|d| d.id.as_str()),
            Some("open-1")
        );
        conv.jump_unresolved(&mut body, true);
        assert_eq!(
            conv.thread_at_cursor(&body).map(|d| d.id.as_str()),
            Some("open-2"),
            "J should pass over the resolved thread"
        );
        conv.jump_unresolved(&mut body, true);
        assert_eq!(
            conv.thread_at_cursor(&body).map(|d| d.id.as_str()),
            Some("open-2"),
            "with none left, J stays put"
        );
        conv.jump_unresolved(&mut body, false);
        assert_eq!(
            conv.thread_at_cursor(&body).map(|d| d.id.as_str()),
            Some("open-1")
        );
    }

    /// One thread runs straight into the next with no blank row between them —
    /// the band on the row naming the author is what divides them.
    #[test]
    fn threads_are_divided_by_a_band_not_a_blank_row() {
        let conv = conversation();
        let body = body(&conv, None, 60);
        let first_of_second = body
            .rows()
            .iter()
            .position(|r| matches!(r, Row::Thread { thread: 1, .. }))
            .expect("the second thread has rows");
        assert_eq!(
            body.rows()[first_of_second],
            Row::Thread {
                thread: 1,
                note: 0,
                head: true
            }
        );
        // The row before it belongs to the first thread, not to a spacer.
        assert!(
            matches!(
                body.rows()[first_of_second - 1],
                Row::Thread { thread: 0, .. }
            ),
            "{:?} sits between the threads",
            body.rows()[first_of_second - 1]
        );
        assert_eq!(
            band(body.rows()[first_of_second], false),
            Some(crate::ui::styles::surface())
        );
        assert_eq!(
            band(
                Row::Thread {
                    thread: 1,
                    note: 0,
                    head: false
                },
                false
            ),
            None
        );
    }

    /// Notes run without a blank row between them: the row naming the next
    /// author is the separator, so a conversation stays dense.
    #[test]
    fn notes_do_not_leave_blank_rows_behind_them() {
        let conv = Conversation {
            discussions: vec![thread(
                "d1",
                vec![
                    note("alice", "first\n\nsecond paragraph", false, false),
                    note("bob", "reply", false, false),
                ],
            )],
            ..Conversation::default()
        };
        let body = body(&conv, None, 60);
        let thread_rows: Vec<String> = body
            .text()
            .into_iter()
            .zip(body.rows())
            .filter(|(_, row)| matches!(row, Row::Thread { .. }))
            .map(|(text, _)| text)
            .collect();
        assert!(
            !thread_rows
                .last()
                .is_some_and(|r| r.trim_start_matches(super::RAIL).trim().is_empty()),
            "{thread_rows:?} ends on a blank row"
        );
    }
}
