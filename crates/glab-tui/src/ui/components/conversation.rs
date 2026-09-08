//! The conversation on an issue or merge request — its description and every
//! comment thread — as one flat list of rows a cursor walks.
//!
//! Rows are the unit, not notes: a pasted log is taller than the pane and has to
//! scroll, so bodies are wrapped to the pane width and each visual row becomes
//! its own [`Line`].  Every row records the thread it belongs to, so the key
//! that replies or resolves reads the thread straight off the row under the
//! cursor rather than asking the reader to pick one out of a list.
//!
//! Chrome holds the left two columns on every row, wrapped rows included: the
//! cursor bar, then the thread's rail.  A reply is inset past its root's rail
//! but keeps it, so a thread reads as one connected block however deep the
//! replies go and however far a body wraps.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use glab_core::domain::{Discussion, Note};

use crate::app::Overlay;
use crate::ui::components::input::CommentInput;
use crate::ui::components::status_bar::format_span;
use crate::ui::{markdown, styles};

/// The cursor's own column, marking the row every key acts on.
const CURSOR_BAR: &str = "\u{258C}";
/// A thread's rail, held down the left of every row the thread owns.
const RAIL: &str = "\u{258E}";
/// The description's rail: thinner than a thread's, so the whole conversation
/// shares one spine down the left without the description reading as a thread.
const DESC_RAIL: &str = "\u{258F}";
/// The elbow that opens a reply under the note it answers.  Exactly as wide as
/// [`REPLY_INSET`], so a reply's own body lines up under its author row.
const REPLY_ELBOW: &str = "\u{2570}\u{2500}";
/// Columns the chrome claims before a root note's text: cursor, rail, gap.
const ROOT_LEAD: usize = 3;
/// Columns a reply is inset past its root.
const REPLY_INSET: usize = 2;
/// Rows a page key moves by.
const PAGE: usize = 10;

/// What a row belongs to, so the row under the cursor answers what a key acts
/// on.  Chrome rows belong to nothing and no key touches them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    /// A section rule or a spacer.
    Chrome,
    /// A row of the item's own description.
    Description,
    /// A row of the thread at this index into `discussions`.  The row that
    /// names its author is the thread's `head`, and draws as a filled band —
    /// which is what divides one thread from the next without spending a blank
    /// row on it.
    Thread { thread: usize, head: bool },
}

/// The conversation's state: what was fetched, what the reader has folded away,
/// and where the cursor sits.
#[derive(Default)]
pub struct Conversation {
    pub discussions: Vec<Discussion>,
    pub loading: bool,
    /// Threads the reader has flipped away from their default fold: an open
    /// thread starts expanded and a resolved one starts collapsed, so an id in
    /// here means the reader asked for the opposite.
    folded: HashSet<String>,
    /// The rows the last render built, parallel to `lines`.  Key handling reads
    /// them, so they outlive the frame that produced them.
    kinds: Vec<RowKind>,
    /// Each row's styled content, starting at the rail — the cursor column is
    /// added while drawing, since it depends on where the cursor is.
    lines: Vec<Line<'static>>,
    cursor: usize,
    /// The first row drawn, moved only as far as keeping the cursor in view
    /// demands.
    offset: usize,
}

impl Conversation {
    pub fn reset(&mut self) {
        self.discussions.clear();
        self.loading = false;
        self.folded.clear();
        self.kinds.clear();
        self.lines.clear();
        self.cursor = 0;
        self.offset = 0;
    }

    /// Take a freshly fetched set of threads, keeping the cursor where it was
    /// so a reply landing does not throw the reader back to the top.
    pub fn set_discussions(&mut self, discussions: Vec<Discussion>) {
        self.discussions = discussions;
        self.loading = false;
    }

    pub fn move_up(&mut self) {
        if let Some(row) = self.next_stop(self.cursor, false) {
            self.cursor = row;
        }
    }

    pub fn move_down(&mut self) {
        if let Some(row) = self.next_stop(self.cursor, true) {
            self.cursor = row;
        }
    }

    pub fn move_top(&mut self) {
        self.cursor = self.settle(0, true);
    }

    pub fn move_bottom(&mut self) {
        self.cursor = self.settle(self.kinds.len().saturating_sub(1), false);
    }

    pub fn page_up(&mut self) {
        self.cursor = self.settle(self.cursor.saturating_sub(PAGE), false);
    }

    pub fn page_down(&mut self) {
        let target = (self.cursor + PAGE).min(self.kinds.len().saturating_sub(1));
        self.cursor = self.settle(target, true);
    }

    /// Jump to the row opening the next unresolved thread in `dir`, staying put
    /// when there is none that way.  On an issue nothing is resolvable, so every
    /// thread counts and this walks thread to thread.
    pub fn move_unresolved(&mut self, down: bool) {
        let open = |row: &usize| {
            matches!(self.kinds[*row], RowKind::Thread { thread, head: true }
                if !self.discussions[thread].resolved())
        };
        let found = if down {
            (self.cursor + 1..self.kinds.len()).find(open)
        } else {
            (0..self.cursor).rev().find(open)
        };
        if let Some(row) = found {
            self.cursor = row;
        }
    }

    /// The next row past `from` in `dir` that a key can act on, or `None` at the
    /// end.  Chrome — a section rule — is stepped over rather than landed on, so
    /// the cursor is never parked somewhere reply has nothing to reply to.
    fn next_stop(&self, from: usize, down: bool) -> Option<usize> {
        if down {
            (from + 1..self.kinds.len()).find(|&r| self.kinds[r] != RowKind::Chrome)
        } else {
            (0..from).rev().find(|&r| self.kinds[r] != RowKind::Chrome)
        }
    }

    /// `target` itself when a key can act on it, else the nearest row that one
    /// can — searching `dir` first, then back the other way, so a jump always
    /// lands somewhere addressable.
    fn settle(&self, target: usize, down: bool) -> usize {
        if self
            .kinds
            .get(target)
            .is_some_and(|k| *k != RowKind::Chrome)
        {
            return target;
        }
        self.next_stop(target, down)
            .or_else(|| self.next_stop(target, !down))
            .unwrap_or(target)
    }

    /// The thread the cursor is on, whether on the row naming its first note, a
    /// body row, or a reply.  `None` on the description and on chrome.
    pub fn thread_at_cursor(&self) -> Option<&Discussion> {
        match self.kinds.get(self.cursor)? {
            RowKind::Thread { thread, .. } => self.discussions.get(*thread),
            RowKind::Chrome | RowKind::Description => None,
        }
    }

    /// Fold the thread under the cursor away, or open it back up.
    pub fn toggle_fold(&mut self) {
        if let Some(id) = self.thread_at_cursor().map(|d| d.id.clone())
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

    /// Rebuild every row for a pane `width` columns wide.
    ///
    /// Run on each draw rather than cached against a fingerprint of the item and
    /// its threads: a conversation is tens of small bodies, so re-parsing them
    /// costs well under a millisecond, and nothing can go stale between a key
    /// press and the frame that answers it.  Worth caching only if a very long
    /// conversation ever shows up in a profile.
    fn build(&mut self, description: Option<&str>, width: usize) {
        self.kinds.clear();
        self.lines.clear();

        if let Some(desc) = description.map(str::trim).filter(|d| !d.is_empty()) {
            self.push_section("DESCRIPTION", None, width);
            let body = markdown::render(desc, "", width.saturating_sub(ROOT_LEAD));
            let rail = Span::styled(DESC_RAIL, Style::default().fg(styles::border()));
            for line in trim_blanks(body) {
                self.push(
                    RowKind::Description,
                    indented(std::slice::from_ref(&rail), line),
                );
            }
        }

        let threads: Vec<usize> = (0..self.discussions.len())
            .filter(|&i| self.discussions[i].comments().next().is_some())
            .collect();
        let unresolved = threads
            .iter()
            .filter(|&&i| self.discussions[i].resolvable() && !self.discussions[i].resolved())
            .count();

        if self.loading {
            self.push_section("CONVERSATION", Some("loading".to_string()), width);
            return;
        }
        if threads.is_empty() {
            self.push_section("CONVERSATION", Some("no comments yet".to_string()), width);
            return;
        }

        let count = threads.len();
        let open = if unresolved > 0 {
            format!(" \u{00B7} {unresolved} unresolved")
        } else {
            String::new()
        };
        let tally = format!("{count} thread{}{open}", plural(count));
        self.push_section("CONVERSATION", Some(tally), width);

        for &i in &threads {
            self.push_thread(i, width);
        }
    }

    /// Push a section rule: a chip naming the section, an optional tally, and a
    /// rule run out to the pane edge so the eye has a hard edge to rest on
    /// without spending a blank row on it.
    fn push_section(&mut self, title: &str, tally: Option<String>, width: usize) {
        let mut spans = vec![
            Span::raw(" "),
            Span::styled(
                format!(" {title} "),
                Style::default()
                    .fg(styles::cyan())
                    .bg(styles::overlay())
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(tally) = tally {
            spans.push(Span::styled(
                format!(" {tally} "),
                Style::default().fg(styles::text_dim()),
            ));
        }
        let used: usize = spans.iter().map(Span::width).sum();
        spans.push(Span::styled(
            "\u{2500}".repeat(width.saturating_sub(used + 1)),
            Style::default().fg(styles::border()),
        ));
        self.push(RowKind::Chrome, Line::from(spans));
    }

    /// Push one thread: the row naming its first note, that note's body, then
    /// each reply inset under it.  A folded thread stops after the first row.
    fn push_thread(&mut self, index: usize, width: usize) {
        let disc = &self.discussions[index];
        let comments: Vec<&Note> = disc.comments().collect();
        let Some((root, replies)) = comments.split_first() else {
            return;
        };
        let resolved = disc.resolved();
        let folded = self.is_folded(disc);
        // The rail carries the thread's state: settled, or still live.
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
        let head_kind = RowKind::Thread {
            thread: index,
            head: true,
        };
        let body_kind = RowKind::Thread {
            thread: index,
            head: false,
        };
        // Collected rather than pushed straight through `push`, which wants all
        // of `self` while `disc` still holds a borrow of its threads.
        let mut rows = vec![(
            head_kind,
            indented(std::slice::from_ref(&rail), Line::from(head)),
        )];
        if folded {
            self.extend(rows);
            return;
        }

        // Bodies render into the room the chrome leaves, then take the chrome on
        // every row they wrapped onto.
        let body = |note: &Note, inset: usize| {
            trim_blanks(markdown::render(
                note.body.trim_end(),
                "",
                width.saturating_sub(ROOT_LEAD + inset),
            ))
        };
        for line in body(root, 0) {
            rows.push((body_kind, indented(std::slice::from_ref(&rail), line)));
        }
        for reply in replies {
            let elbow = Span::styled(REPLY_ELBOW, Style::default().fg(styles::text_dim()));
            rows.push((
                body_kind,
                indented(&[rail.clone(), elbow], Line::from(head_spans(reply, false))),
            ));
            let inset = Span::raw(" ".repeat(REPLY_INSET));
            for line in body(reply, REPLY_INSET) {
                rows.push((body_kind, indented(&[rail.clone(), inset.clone()], line)));
            }
        }
        self.extend(rows);
    }

    fn push(&mut self, kind: RowKind, line: Line<'static>) {
        self.kinds.push(kind);
        self.lines.push(line);
    }

    fn extend(&mut self, rows: Vec<(RowKind, Line<'static>)>) {
        for (kind, line) in rows {
            self.push(kind, line);
        }
    }

    /// Move `offset` no further than keeping the cursor inside a pane `height`
    /// rows tall, so the view stays put while the cursor roams within it.
    fn scroll_into_view(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        self.cursor = self.cursor.min(self.kinds.len().saturating_sub(1));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
        // Never leave dead space below the last row: folding a thread away can
        // otherwise strand the view past the end of what is left.
        self.offset = self.offset.min(self.kinds.len().saturating_sub(height));
    }

    /// The row naming the first note of the thread the cursor is in.
    fn head_row_of_cursor(&self) -> Option<usize> {
        let RowKind::Thread { thread, .. } = self.kinds.get(self.cursor)? else {
            return None;
        };
        (0..=self.cursor).rev().find(|&r| {
            self.kinds.get(r)
                == Some(&RowKind::Thread {
                    thread: *thread,
                    head: true,
                })
        })
    }
}

/// The overlay that drafts a reply into the thread under the cursor.
///
/// A standalone comment takes a reply as readily as a thread does — GitLab turns
/// it into one when the first reply lands.  Only with the cursor off every
/// thread entirely, which means on the description, does this fall back to
/// drafting a new thread.
pub fn draft_reply(state: &Conversation) -> Overlay {
    let reply_discussion_id = state.thread_at_cursor().map(|d| d.id.clone());
    Overlay::CommentInput {
        input: CommentInput::default(),
        autocomplete: Box::default(),
        reply_discussion_id,
    }
}

/// The overlay that drafts a new top-level thread.
pub fn draft_new_thread() -> Overlay {
    Overlay::CommentInput {
        input: CommentInput::default(),
        autocomplete: Box::default(),
        reply_discussion_id: None,
    }
}

/// Draw the conversation into `area`, rebuilding its rows for that width first.
pub fn render(frame: &mut Frame, area: Rect, state: &mut Conversation, description: Option<&str>) {
    let width = usize::from(area.width);
    let height = usize::from(area.height);
    state.build(description, width);
    state.scroll_into_view(height);

    let end = (state.offset + height).min(state.lines.len());
    let rows: Vec<Line<'static>> = (state.offset..end)
        .map(|row| {
            let selected = row == state.cursor;
            let mut spans = vec![Span::styled(
                if selected { CURSOR_BAR } else { " " },
                Style::default().fg(styles::orange()),
            )];
            spans.extend(state.lines[row].spans.clone());
            let line = Line::from(spans);
            // The cursor's tint wins over the band, so a selected head row still
            // reads as selected.
            match band(state.kinds[row], selected) {
                Some(bg) => fill(line, width, Style::default().bg(bg)),
                None => line,
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), area);

    // The thread's own first note has scrolled off, so the reader has lost what
    // the replies under the cursor are answering: float it back over the top.
    if let Some(head) = state.head_row_of_cursor()
        && head < state.offset
        && let Some(thread) = state.thread_at_cursor()
        && let Some(root) = thread.comments().next()
    {
        floating_head(frame, area, root, thread.resolved());
    }
}

/// Float the thread's opening note over the top of the pane, on the overlay
/// background so it reads as sitting above the list rather than in it.
///
/// Built from the note rather than from the row that renders it: the row carries
/// the thread's rail, which inside the card would read as a second thread.
fn floating_head(frame: &mut Frame, area: Rect, root: &Note, resolved: bool) {
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
    spans.extend(head_spans(root, resolved));
    // The opening line of the body, so the card says what the thread is about
    // and not merely who started it.
    if let Some(opening) = root.body.lines().find(|l| !l.trim().is_empty()) {
        spans.push(Span::styled(
            format!("  \u{00B7}  {}", opening.trim()),
            Style::default().fg(styles::overlay_text_dim()),
        ));
    }
    let line = fill(
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

/// Put `chrome` in the columns to the left of `line`, then a single gap.
fn indented(chrome: &[Span<'static>], line: Line<'static>) -> Line<'static> {
    let mut spans = chrome.to_vec();
    spans.push(Span::raw(" "));
    spans.extend(line.spans);
    Line::from(spans)
}

/// The background a row fills its whole width with: the cursor's tint on the
/// selected row, and a quieter band on the row that opens a thread.  The band is
/// what separates one thread from the next, in place of the blank row a gap
/// would cost — and it doubles as the thread's header.
fn band(kind: RowKind, selected: bool) -> Option<Color> {
    if selected {
        return Some(styles::highlight());
    }
    matches!(kind, RowKind::Thread { head: true, .. }).then_some(styles::surface())
}

/// Pad `line` out to `width` columns and lay `style` under it, so its
/// background reaches the pane edge instead of stopping at its text.  Spans that
/// set a background of their own — a section chip, a code span — keep it.
fn fill(line: Line<'static>, width: usize, style: Style) -> Line<'static> {
    let used: usize = line.spans.iter().map(Span::width).sum();
    let mut spans = line.spans;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    Line::from(spans).style(style)
}

/// Drop the blank rows a block-level renderer leaves after its last block, which
/// would otherwise open a gap between every note.
fn trim_blanks(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    while lines
        .last()
        .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
    {
        lines.pop();
    }
    lines
}

fn ago(at: DateTime<Utc>) -> String {
    let secs = Utc::now().signed_duration_since(at).num_seconds();
    format_span(u64::try_from(secs).unwrap_or(0))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use glab_core::domain::{Discussion, Note, User};

    use super::{Conversation, Overlay, RowKind};

    fn user(name: &str) -> User {
        User {
            id: name.to_string(),
            username: name.to_string(),
        }
    }

    fn note(id: u64, author: &str, body: &str, resolvable: bool, resolved: bool) -> Note {
        Note {
            id,
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

    /// The thread a drafted reply is addressed to, or `None` when it drafts a
    /// new one.
    fn reply_target(state: &Conversation) -> Option<String> {
        match super::draft_reply(state) {
            Overlay::CommentInput {
                reply_discussion_id,
                ..
            } => reply_discussion_id,
            _ => panic!("reply should draft a comment"),
        }
    }

    /// Reply addresses the thread under the cursor even when that thread is a
    /// lone comment with no replies yet — GitLab turns one into a thread when
    /// the first reply lands, so there is nothing to refuse.
    #[test]
    fn reply_addresses_a_lone_comment_rather_than_starting_a_new_thread() {
        let mut state = Conversation {
            discussions: vec![
                standalone("d1", note(1, "alice", "a single comment", false, false)),
                thread("d2", vec![note(2, "bob", "in a thread", false, false)]),
            ],
            ..Conversation::default()
        };
        state.build(Some("the description"), 60);

        for (thread, id) in [(0usize, "d1"), (1, "d2")] {
            let rows: Vec<usize> = (0..state.kinds.len())
                .filter(
                    |&r| matches!(state.kinds[r], RowKind::Thread { thread: t, .. } if t == thread),
                )
                .collect();
            assert!(!rows.is_empty(), "thread {thread} has no rows");
            for row in rows {
                state.cursor = row;
                assert_eq!(
                    reply_target(&state).as_deref(),
                    Some(id),
                    "row {row} should reply into {id}"
                );
            }
        }

        // Only the description, which is no thread at all, drafts a new one.
        state.cursor = state
            .kinds
            .iter()
            .position(|k| *k == RowKind::Description)
            .expect("the description has a row");
        assert_eq!(reply_target(&state), None);
    }

    /// Two threads, the second resolved, with a reply on the first.
    fn conversation() -> Conversation {
        Conversation {
            discussions: vec![
                thread(
                    "d1",
                    vec![
                        note(1, "alice", "why is the runner full?", false, false),
                        note(2, "bob", "the layer cache never expires", false, false),
                    ],
                ),
                thread("d2", vec![note(3, "carol", "raised the quota", true, true)]),
            ],
            ..Conversation::default()
        }
    }

    /// The plain text of every row, chrome included.
    fn rows(state: &Conversation) -> Vec<String> {
        state
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn every_row_of_a_thread_answers_with_that_thread() {
        let mut state = conversation();
        state.build(None, 60);
        let threads: Vec<Option<&str>> = (0..state.kinds.len())
            .map(|row| match state.kinds[row] {
                RowKind::Thread { thread, .. } => Some(state.discussions[thread].id.as_str()),
                RowKind::Chrome | RowKind::Description => None,
            })
            .collect();
        assert!(
            threads.iter().filter(|t| **t == Some("d1")).count() >= 4,
            "{:?} in {:?}",
            threads,
            rows(&state)
        );
        for (row, expected) in threads.iter().enumerate() {
            state.cursor = row;
            assert_eq!(
                state.thread_at_cursor().map(|d| d.id.as_str()),
                *expected,
                "row {row}: {:?}",
                rows(&state)[row]
            );
        }
    }

    /// A reply keeps its root's rail, so the thread stays one connected block,
    /// and every row of a wrapped body keeps it too.
    #[test]
    fn a_reply_is_inset_but_keeps_the_rail() {
        let mut state = Conversation {
            discussions: vec![thread(
                "d1",
                vec![
                    note(1, "alice", "root", false, false),
                    note(
                        2,
                        "bob",
                        "a reply long enough that it has to wrap onto a second row",
                        false,
                        false,
                    ),
                ],
            )],
            ..Conversation::default()
        };
        state.build(None, 36);
        let body: Vec<String> = rows(&state)
            .into_iter()
            .filter(|r| r.contains("wrap") || r.contains("second") || r.contains("@bob"))
            .collect();
        assert!(body.len() >= 2, "{body:?} should have wrapped");
        for row in &body {
            assert!(row.starts_with(super::RAIL), "{row:?} lost the rail");
        }
        assert!(
            body[0].contains(super::REPLY_ELBOW),
            "{body:?} lost the elbow"
        );
    }

    /// Folding a thread away pulls the view back so the last row still sits at
    /// the bottom of the pane, rather than stranding it past the end.
    #[test]
    fn folding_a_thread_away_does_not_strand_the_view_past_the_end() {
        let mut state = conversation();
        state.build(None, 60);
        state.move_bottom();
        state.scroll_into_view(4);
        assert_eq!(state.offset, state.kinds.len() - 4);

        state.cursor = state
            .kinds
            .iter()
            .position(|k| matches!(k, RowKind::Thread { thread: 0, .. }))
            .expect("the open thread has a row");
        state.toggle_fold();
        state.build(None, 60);
        state.scroll_into_view(4);
        assert!(
            state.offset + 4 <= state.kinds.len().max(4),
            "offset {} strands {} rows",
            state.offset,
            state.kinds.len()
        );
    }

    /// A resolved thread arrives folded to its opening row; the fold key opens
    /// it, and closes an open one.
    #[test]
    fn a_resolved_thread_starts_folded_and_the_key_flips_either_way() {
        let mut state = conversation();
        state.build(None, 60);
        assert!(
            !rows(&state).iter().any(|r| r.contains("raised the quota")),
            "{:?}",
            rows(&state)
        );

        // Land on the resolved thread's only row and open it.
        let resolved = state
            .kinds
            .iter()
            .position(|k| matches!(k, RowKind::Thread { thread: 1, .. }))
            .expect("the resolved thread has a row");
        state.cursor = resolved;
        state.toggle_fold();
        state.build(None, 60);
        assert!(
            rows(&state).iter().any(|r| r.contains("raised the quota")),
            "{:?}",
            rows(&state)
        );

        // The same key folds the open thread away.
        state.cursor = state
            .kinds
            .iter()
            .position(|k| matches!(k, RowKind::Thread { thread: 0, .. }))
            .expect("the open thread has a row");
        state.toggle_fold();
        state.build(None, 60);
        assert!(
            !rows(&state).iter().any(|r| r.contains("layer cache")),
            "{:?}",
            rows(&state)
        );
    }

    /// The cursor's thread head is what the floating card shows, found by
    /// walking back to where the thread's rows start.
    #[test]
    fn the_head_row_of_the_cursor_opens_its_thread() {
        let mut state = conversation();
        state.build(None, 60);
        let first = state
            .kinds
            .iter()
            .position(|k| matches!(k, RowKind::Thread { thread: 0, .. }))
            .expect("the thread has rows");
        let last = state
            .kinds
            .iter()
            .rposition(|k| matches!(k, RowKind::Thread { thread: 0, .. }))
            .expect("the thread has rows");
        assert!(last > first);
        for row in first..=last {
            state.cursor = row;
            assert_eq!(state.head_row_of_cursor(), Some(first), "row {row}");
        }
        // Chrome belongs to no thread, so there is nothing to float.
        state.cursor = 0;
        assert_eq!(state.head_row_of_cursor(), None);
    }

    /// The cursor never parks on a section rule, so reply always has something
    /// to reply to: stepping onto one carries straight through it.
    #[test]
    fn the_cursor_steps_over_rows_no_key_can_act_on() {
        let mut state = conversation();
        state.build(Some("a description"), 60);
        assert!(
            state.kinds.contains(&RowKind::Chrome),
            "the fixture should have section rules to step over"
        );

        state.move_top();
        let mut seen = vec![state.cursor];
        for _ in 0..state.kinds.len() {
            state.move_down();
            seen.push(state.cursor);
        }
        for row in seen {
            assert_ne!(
                state.kinds[row],
                RowKind::Chrome,
                "landed on chrome at row {row}"
            );
        }

        state.move_bottom();
        assert_ne!(state.kinds[state.cursor], RowKind::Chrome);
        for _ in 0..state.kinds.len() {
            state.move_up();
            assert_ne!(state.kinds[state.cursor], RowKind::Chrome);
        }
        state.page_down();
        assert_ne!(state.kinds[state.cursor], RowKind::Chrome);
        state.page_up();
        assert_ne!(state.kinds[state.cursor], RowKind::Chrome);
    }

    /// `J` and `K` land on the row opening a thread and pass over a resolved
    /// one, so they walk what still needs an answer.
    #[test]
    fn unresolved_jumps_skip_a_settled_thread() {
        let mut state = Conversation {
            discussions: vec![
                thread("open-1", vec![note(1, "alice", "why?", true, false)]),
                thread("settled", vec![note(2, "bob", "fixed", true, true)]),
                thread("open-2", vec![note(3, "carol", "and this?", true, false)]),
            ],
            ..Conversation::default()
        };
        state.build(None, 60);

        state.move_top();
        assert_eq!(
            state.thread_at_cursor().map(|d| d.id.as_str()),
            Some("open-1")
        );
        state.move_unresolved(true);
        assert_eq!(
            state.thread_at_cursor().map(|d| d.id.as_str()),
            Some("open-2"),
            "J should pass over the resolved thread"
        );
        state.move_unresolved(true);
        assert_eq!(
            state.thread_at_cursor().map(|d| d.id.as_str()),
            Some("open-2"),
            "with none left, J stays put"
        );
        state.move_unresolved(false);
        assert_eq!(
            state.thread_at_cursor().map(|d| d.id.as_str()),
            Some("open-1")
        );
    }

    /// One thread runs straight into the next with no blank row between them —
    /// the band on the row naming the author is what divides them.
    #[test]
    fn threads_are_divided_by_a_band_not_a_blank_row() {
        let mut state = conversation();
        state.build(None, 60);
        let first_of_second = state
            .kinds
            .iter()
            .position(|k| matches!(k, RowKind::Thread { thread: 1, .. }))
            .expect("the second thread has rows");
        assert_eq!(
            state.kinds[first_of_second],
            RowKind::Thread {
                thread: 1,
                head: true
            }
        );
        // The row before it belongs to the first thread, not to a spacer.
        assert!(
            matches!(
                state.kinds[first_of_second - 1],
                RowKind::Thread { thread: 0, .. }
            ),
            "{:?} sits between the threads",
            state.kinds[first_of_second - 1]
        );
        assert_eq!(
            super::band(state.kinds[first_of_second], false),
            Some(crate::ui::styles::surface())
        );
        assert_eq!(
            super::band(
                RowKind::Thread {
                    thread: 1,
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
        let mut state = Conversation {
            discussions: vec![thread(
                "d1",
                vec![
                    note(1, "alice", "first\n\nsecond paragraph", false, false),
                    note(2, "bob", "reply", false, false),
                ],
            )],
            ..Conversation::default()
        };
        state.build(None, 60);
        let thread_rows: Vec<String> = rows(&state)
            .into_iter()
            .zip(&state.kinds)
            .filter(|(_, k)| matches!(k, RowKind::Thread { .. }))
            .map(|(r, _)| r)
            .collect();
        assert_eq!(
            thread_rows.last().map(String::as_str),
            Some(format!("{}   reply", super::RAIL).as_str()),
            "{thread_rows:?}"
        );
    }
}
