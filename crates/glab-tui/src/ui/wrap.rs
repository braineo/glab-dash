//! Soft-wrapping styled text to a column width.
//!
//! [`Paragraph`](ratatui::widgets::Paragraph)'s own wrapping cannot serve a
//! view whose rows carry chrome.  It reflows a logical line into however many
//! screen rows it needs and knows nothing of the indent or gutter leading that
//! line, so every row after the first starts back at column zero and a wrapped
//! reply becomes indistinguishable from a root comment.  Wrapping here instead
//! makes one [`Line`] mean one screen row, which keeps the chrome on every row
//! and lets a caller address rows by index.
//!
//! Widths are display columns, measured over grapheme clusters, so text that is
//! not plain ASCII lines up: a CJK glyph claims the two columns it draws in.

use std::mem;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The display width of `s` in terminal columns.
///
/// A multi-codepoint emoji is measured as the sum of its parts, which overstates
/// the one or two columns a terminal that ligates it actually draws; terminals
/// disagree here and no measurement satisfies all of them.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Cut `s` down to at most `width` display columns, marking a cut with an
/// ellipsis so a shortened cell reads as shortened rather than as the whole of
/// it.  A `width` of zero leaves nothing to draw in.
pub fn truncate(s: &str, width: usize) -> String {
    if self::width(s) <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for g in s.graphemes(true) {
        let gw = UnicodeWidthStr::width(g);
        if used + gw > width - 1 {
            break;
        }
        out.push_str(g);
        used += gw;
    }
    out.push('\u{2026}');
    out
}

/// One grapheme cluster, the style it carries, and the columns it draws in.
struct Cell {
    text: String,
    style: Style,
    width: usize,
}

impl Cell {
    fn is_space(&self) -> bool {
        self.text.chars().all(char::is_whitespace)
    }
}

/// Break `body` into rows no wider than `width` columns, preferring a break at
/// whitespace and hard-splitting a word too wide to fit a row of its own.  Each
/// row keeps the styles of the spans it came from.  Whitespace a break lands on
/// is dropped, so no row starts or ends on it; leading whitespace on the first
/// row survives, being content rather than a break.
///
/// A `width` of zero means the target width is not known yet, so `body` comes
/// back as a single unwrapped row.
pub fn wrap_spans(body: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let cells = flatten(body);
    if width == 0 || cells.iter().map(|c| c.width).sum::<usize>() <= width {
        return vec![body.to_vec()];
    }

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut row_width = 0;
    // Whitespace between two words, held back until a word follows it: if the
    // row breaks here instead, the break consumes it.
    let mut gap: Vec<Cell> = Vec::new();
    let mut gap_width = 0;

    for run in runs(cells) {
        let run_width: usize = run.iter().map(|c| c.width).sum();
        if run.first().is_some_and(Cell::is_space) {
            gap = run;
            gap_width = run_width;
            continue;
        }

        // The word does not fit after the gap, so the row ends before both.
        if !row.is_empty() && row_width + gap_width + run_width > width {
            rows.push(regroup(mem::take(&mut row)));
            row_width = 0;
            gap.clear();
            gap_width = 0;
        }
        // A row opened by a break drops the gap; the document's own first row
        // keeps it, where it is the text's own leading whitespace.
        if row.is_empty() && !rows.is_empty() {
            gap.clear();
            gap_width = 0;
        }
        row.append(&mut gap);
        row_width += mem::take(&mut gap_width);

        // Still wider than a whole row, so the word itself has to be split.
        for cell in run {
            if !row.is_empty() && row_width + cell.width > width {
                rows.push(regroup(mem::take(&mut row)));
                row_width = 0;
            }
            row_width += cell.width;
            row.push(cell);
        }
    }
    rows.push(regroup(row));
    rows
}

/// Wrap `body` to `width` columns behind `prefix`, which leads the first row
/// while every row the body wraps onto is led by blanks as wide as it.  This is
/// the shape of a bullet, a heading marker or an indent: chrome that introduces
/// a block once and holds its column for the rest.
pub fn hanging(
    prefix: &[Span<'static>],
    body: &[Span<'static>],
    width: usize,
) -> Vec<Line<'static>> {
    let lead: usize = prefix.iter().map(Span::width).sum();
    let rows = wrap_spans(body, width.saturating_sub(lead));
    let pad = Span::raw(" ".repeat(lead));
    rows.into_iter()
        .enumerate()
        .map(|(i, spans)| {
            let mut out = if i == 0 {
                prefix.to_vec()
            } else {
                vec![pad.clone()]
            };
            out.extend(spans);
            Line::from(out)
        })
        .collect()
}

/// Explode the spans into one [`Cell`] per grapheme cluster, so a break can
/// land anywhere and [`regroup`] can put the spans back together after.
fn flatten(spans: &[Span<'static>]) -> Vec<Cell> {
    spans
        .iter()
        .flat_map(|span| {
            span.content.graphemes(true).map(|g| Cell {
                text: g.to_string(),
                style: span.style,
                width: width(g),
            })
        })
        .collect()
}

/// Group cells into alternating runs of whitespace and non-whitespace, the
/// words and gaps wrapping decides between.
fn runs(cells: Vec<Cell>) -> Vec<Vec<Cell>> {
    let mut out: Vec<Vec<Cell>> = Vec::new();
    for cell in cells {
        match out.last_mut() {
            Some(last) if last[0].is_space() == cell.is_space() => last.push(cell),
            _ => out.push(vec![cell]),
        }
    }
    out
}

/// Rebuild spans from cells, merging neighbors that share a style back into one.
fn regroup(cells: Vec<Cell>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for cell in cells {
        match spans.last_mut() {
            Some(last) if last.style == cell.style => last.content.to_mut().push_str(&cell.text),
            _ => spans.push(Span::styled(cell.text, cell.style)),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    use super::{hanging, wrap_spans};

    /// The plain text of each wrapped row, so a test reads break points without
    /// span noise.
    fn texts(rows: &[Vec<Span<'static>>]) -> Vec<String> {
        rows.iter()
            .map(|row| row.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// The plain text of each line, for the prefixed forms.
    fn lines(rendered: &[Line<'static>]) -> Vec<String> {
        rendered
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn spans(text: &str) -> Vec<Span<'static>> {
        vec![Span::raw(text.to_string())]
    }

    #[test]
    fn text_within_the_width_comes_back_whole() {
        assert_eq!(
            texts(&wrap_spans(&spans("short enough"), 20)),
            ["short enough"]
        );
    }

    #[test]
    fn a_zero_width_leaves_the_text_unwrapped() {
        assert_eq!(
            texts(&wrap_spans(&spans("anything at all here"), 0)),
            ["anything at all here"]
        );
    }

    #[test]
    fn breaks_land_on_spaces_and_consume_them() {
        assert_eq!(
            texts(&wrap_spans(&spans("the quick brown lazy fox"), 10)),
            ["the quick", "brown lazy", "fox"]
        );
    }

    #[test]
    fn a_word_wider_than_a_row_is_split_hard() {
        assert_eq!(
            texts(&wrap_spans(&spans("a supercalifragilistic word"), 8)),
            ["a", "supercal", "ifragili", "stic", "word"]
        );
    }

    /// The whole point of measuring in columns: a run of CJK breaks at half as
    /// many glyphs as an ASCII run of the same column width, because each glyph
    /// draws in two columns.
    #[test]
    fn wide_glyphs_count_the_two_columns_they_draw_in() {
        assert_eq!(
            texts(&wrap_spans(&spans("統合制御 統合制御 統合制御"), 10)),
            ["統合制御", "統合制御", "統合制御"]
        );
    }

    #[test]
    fn each_wrapped_row_keeps_the_styles_of_its_spans() {
        let red = Style::default().fg(Color::Red);
        let blue = Style::default().fg(Color::Blue);
        let body = vec![
            Span::styled("hello", red),
            Span::styled(" ", red),
            Span::styled("world", blue),
        ];
        let rows = wrap_spans(&body, 5);
        let styled: Vec<Vec<(String, Style)>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|s| (s.content.to_string(), s.style))
                    .collect()
            })
            .collect();
        assert_eq!(
            styled,
            [
                vec![("hello".to_string(), red)],
                vec![("world".to_string(), blue)],
            ]
        );
    }

    /// The bug this module exists for: a wrapped row holds the chrome's column
    /// instead of falling back to zero.
    #[test]
    fn a_hanging_prefix_holds_its_column_on_every_wrapped_row() {
        assert_eq!(
            lines(&hanging(
                &spans("  \u{2022} "),
                &spans("the first bullet is long enough that it wraps"),
                24
            )),
            [
                "  \u{2022} the first bullet is",
                "    long enough that it",
                "    wraps"
            ]
        );
    }

    #[test]
    fn a_prefix_wider_than_the_target_leaves_the_body_unwrapped() {
        assert_eq!(
            lines(&hanging(&spans("        "), &spans("body text here"), 4)),
            ["        body text here"]
        );
    }
}
