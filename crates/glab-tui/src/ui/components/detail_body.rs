//! The body of a detail view: whatever its sections put in it, as one flat list
//! of rows a single cursor walks.
//!
//! A visual row is the unit, so a body too tall for the pane scrolls and every
//! row — wrapped tails included — records what it belongs to.  A view pushes its
//! own sections in whatever order it reads best; what fills them is not this
//! module's business.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::keybindings::KeyAction;
use crate::ui::{markdown, styles};

/// The cursor's own column, marking the row every key acts on.
const CURSOR_BAR: &str = "\u{258C}";
/// The spine a section holds down the left of its rows.
pub const RAIL: &str = "\u{258F}";
/// Columns the chrome claims before a row's text: cursor, rail, gap.
pub const LEAD: usize = 3;
/// Rows a page key moves by.
const PAGE: usize = 10;

/// What a row belongs to, so the row under the cursor answers what a key acts
/// on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A section rule or a spacer.
    Chrome,
    /// A row of the item's own description.
    Description,
    /// The row for the related item at this index into the view's related list.
    Related(usize),
    /// A row of the thread at this index into `discussions`, belonging to its
    /// `note`th comment.  Its `head` names the thread's first author.
    Thread {
        thread: usize,
        note: usize,
        head: bool,
    },
}

impl Row {
    /// Chrome is stepped over, never landed on: no key acts on it.
    fn is_stop(self) -> bool {
        self != Row::Chrome
    }
}

/// The rows of one detail view, and where the reader is in them.
///
/// Rebuilt on each draw: tens of small bodies re-parse in well under a
/// millisecond, and nothing can go stale.  ponytail: cache against a
/// fingerprint if a very long conversation ever shows up in a profile.
#[derive(Default)]
pub struct DetailBody {
    /// Parallel to `lines`.  Key handling reads them, so they outlive the
    /// frame that built them.
    rows: Vec<Row>,
    /// Each row from the rail on; the cursor column is added while drawing.
    lines: Vec<Line<'static>>,
    cursor: usize,
    /// The first row drawn.
    offset: usize,
}

impl DetailBody {
    // ── Building, once per draw ──────────────────────────────────────

    /// The cursor and offset survive a build: they are where the reader is.
    pub fn begin(&mut self) {
        self.rows.clear();
        self.lines.clear();
    }

    /// A chip naming the section, an optional tally, and a rule to the pane
    /// edge.
    pub fn section(&mut self, title: &str, tally: Option<String>, width: usize) {
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
        self.push(Row::Chrome, Line::from(spans));
    }

    pub fn push(&mut self, row: Row, line: Line<'static>) {
        self.rows.push(row);
        self.lines.push(line);
    }

    pub fn extend(&mut self, rows: Vec<(Row, Line<'static>)>) {
        for (row, line) in rows {
            self.push(row, line);
        }
    }

    /// Nothing at all for an item with no description.
    pub fn description(&mut self, description: Option<&str>, width: usize) {
        let Some(text) = description.map(str::trim).filter(|d| !d.is_empty()) else {
            return;
        };
        self.section("DESCRIPTION", None, width);
        let body = markdown::render(text, "", width.saturating_sub(LEAD));
        let rail = Span::styled(RAIL, Style::default().fg(styles::border()));
        for line in trim_blanks(body) {
            self.push(
                Row::Description,
                indented(std::slice::from_ref(&rail), line),
            );
        }
    }

    // ── Reading ──────────────────────────────────────────────────────

    /// [`Row::Chrome`] when the body is empty, which no key acts on either.
    pub fn cursor_row(&self) -> Row {
        self.rows.get(self.cursor).copied().unwrap_or(Row::Chrome)
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn set_cursor(&mut self, row: usize) {
        self.cursor = row;
    }

    /// The plain text of every row, chrome included.
    #[cfg(test)]
    pub fn text(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    // ── Navigating ───────────────────────────────────────────────────

    /// `true` when the key was one of these, so a caller can try its own.
    pub fn handle_key(&mut self, action: KeyAction) -> bool {
        match action {
            KeyAction::MoveDown => self.cursor = self.step(self.cursor, true),
            KeyAction::MoveUp => self.cursor = self.step(self.cursor, false),
            KeyAction::Top => self.cursor = self.settle(0, true),
            KeyAction::Bottom => {
                self.cursor = self.settle(self.rows.len().saturating_sub(1), false);
            }
            KeyAction::PageDown => {
                let target = (self.cursor + PAGE).min(self.rows.len().saturating_sub(1));
                self.cursor = self.settle(target, true);
            }
            KeyAction::PageUp => self.cursor = self.settle(self.cursor.saturating_sub(PAGE), false),
            _ => return false,
        }
        true
    }

    /// Stays put when no row that way accepts.
    pub fn jump(&mut self, down: bool, stop: impl Fn(Row) -> bool) {
        let found = if down {
            (self.cursor + 1..self.rows.len()).find(|&r| stop(self.rows[r]))
        } else {
            (0..self.cursor).rev().find(|&r| stop(self.rows[r]))
        };
        if let Some(row) = found {
            self.cursor = row;
        }
    }

    /// `from` itself at the end.
    fn step(&self, from: usize, down: bool) -> usize {
        self.next_stop(from, down).unwrap_or(from)
    }

    fn next_stop(&self, from: usize, down: bool) -> Option<usize> {
        if down {
            (from + 1..self.rows.len()).find(|&r| self.rows[r].is_stop())
        } else {
            (0..from).rev().find(|&r| self.rows[r].is_stop())
        }
    }

    /// `target`, or the nearest row a key can act on — `dir` first, then back,
    /// so a jump always lands somewhere addressable.
    fn settle(&self, target: usize, down: bool) -> usize {
        if self.rows.get(target).is_some_and(|r| r.is_stop()) {
            return target;
        }
        self.next_stop(target, down)
            .or_else(|| self.next_stop(target, !down))
            .unwrap_or(target)
    }

    /// Move `offset` no further than keeping the cursor inside `height` rows.
    pub fn scroll_into_view(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }
        // Folding a thread away would otherwise strand the view past the end.
        self.offset = self.offset.min(self.rows.len().saturating_sub(height));
    }

    // ── Drawing ──────────────────────────────────────────────────────

    /// Scrolls to wherever the cursor now is.
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let width = usize::from(area.width);
        let height = usize::from(area.height);
        self.scroll_into_view(height);

        let end = (self.offset + height).min(self.lines.len());
        let rows: Vec<Line<'static>> = (self.offset..end)
            .map(|row| {
                let selected = row == self.cursor;
                let mut spans = vec![Span::styled(
                    if selected { CURSOR_BAR } else { " " },
                    Style::default().fg(styles::orange()),
                )];
                spans.extend(self.lines[row].spans.clone());
                let line = Line::from(spans);
                // The cursor's tint wins over the band.
                match band(self.rows[row], selected) {
                    Some(bg) => fill(line, width, Style::default().bg(bg)),
                    None => line,
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(rows), area);
    }
}

/// The cursor's tint on the selected row; a quieter band on a thread's head
/// row, which is what divides one thread from the next.
pub fn band(row: Row, selected: bool) -> Option<Color> {
    if selected {
        return Some(styles::highlight());
    }
    matches!(row, Row::Thread { head: true, .. }).then_some(styles::surface())
}

/// Put `chrome` in the columns to the left of `line`, then a single gap.
pub fn indented(chrome: &[Span<'static>], line: Line<'static>) -> Line<'static> {
    let mut spans = chrome.to_vec();
    spans.push(Span::raw(" "));
    spans.extend(line.spans);
    Line::from(spans)
}

/// Pad `line` to `width` and lay `style` under it, so its background reaches
/// the pane edge.  A span with a background of its own keeps it.
pub fn fill(line: Line<'static>, width: usize, style: Style) -> Line<'static> {
    let used: usize = line.spans.iter().map(Span::width).sum();
    let mut spans = line.spans;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    Line::from(spans).style(style)
}

/// Drop the trailing blanks a block renderer leaves, which would open a gap
/// between every note.
pub fn trim_blanks(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    while lines
        .last()
        .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
    {
        lines.pop();
    }
    lines
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
