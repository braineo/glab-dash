use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{Arena, Options, parse_document};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::{styles, wrap};

/// The gutter a comment body is drawn behind, on every one of its rows.
const COMMENT_GUTTER: &str = "  \u{2502} ";

/// Render a markdown string into styled ratatui Lines, each one screen row wide
/// at most, wrapped to `width` columns behind `indent`.  A `width` of zero means
/// the target width is not known yet, so nothing wraps.
pub fn render(text: &str, indent: &str, width: usize) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, text, &opts);
    let mut lines = Vec::new();
    render_node(root, &mut lines, indent, &mut InlineCtx::default(), width);
    lines
}

/// Render markdown for a comment body, behind the gutter that marks it as one.
/// The body wraps into the room the gutter leaves, so every row it takes keeps
/// the gutter and a wrapped comment still reads as one block.
pub fn render_comment(text: &str, width: usize) -> Vec<Line<'static>> {
    let gutter_width = wrap::width(COMMENT_GUTTER);
    let body = render(text, "", width.saturating_sub(gutter_width));

    let gutter = Span::styled(COMMENT_GUTTER, styles::help_desc_style());
    body.into_iter()
        .map(|line| {
            let mut spans = vec![gutter.clone()];
            spans.extend(line.spans);
            Line::from(spans)
        })
        .collect()
}

fn options() -> Options<'static> {
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.extension.footnotes = true;
    opts
}

#[derive(Default, Clone)]
#[allow(clippy::struct_excessive_bools)]
struct InlineCtx {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    code: bool,
}

impl InlineCtx {
    fn style(&self) -> Style {
        let mut s = Style::default().fg(styles::TEXT);
        if self.code {
            s = s.fg(styles::ORANGE).bg(Color::Rgb(35, 38, 52));
        }
        if self.bold {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if self.strikethrough {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        s
    }
}

fn render_node<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    ctx: &mut InlineCtx,
    width: usize,
) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            let mut body = Vec::new();
            collect_inline(node, &mut body, ctx);
            lines.extend(wrap::hanging(
                &[Span::raw(indent.to_string())],
                &body,
                width,
            ));
            lines.push(Line::from(""));
        }
        NodeValue::Heading(h) => {
            let level = h.level as usize;
            let prefix = "#".repeat(level);
            let mut body = Vec::new();
            collect_inline(node, &mut body, ctx);
            lines.extend(wrap::hanging(
                &[
                    Span::raw(indent.to_string()),
                    Span::styled(
                        format!("{prefix} "),
                        Style::default()
                            .fg(styles::MAGENTA)
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                &body,
                width,
            ));
            lines.push(Line::from(""));
        }
        // Code keeps its source rows: reflowing it would move the line breaks
        // its meaning rests on, so a row wider than the pane is left to clip.
        NodeValue::CodeBlock(cb) => {
            let code_bg = Color::Rgb(35, 38, 52);
            if cb.info.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled("╭───", Style::default().fg(styles::BORDER)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled(
                        format!("╭─ {} ", cb.info),
                        Style::default().fg(styles::BORDER),
                    ),
                ]));
            }
            for code_line in cb.literal.trim_end().lines() {
                let expanded = code_line.replace('\t', "    ");
                lines.push(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled("│ ", Style::default().fg(styles::BORDER)),
                    Span::styled(expanded, Style::default().fg(styles::ORANGE).bg(code_bg)),
                ]));
            }
            lines.push(Line::from(vec![
                Span::raw(indent.to_string()),
                Span::styled("╰───", Style::default().fg(styles::BORDER)),
            ]));
            lines.push(Line::from(""));
        }
        NodeValue::List(list) => {
            let mut item_num = list.start;
            for child in node.children() {
                render_list_item(child, lines, indent, ctx, list.list_type, item_num, width);
                if list.list_type == ListType::Ordered {
                    item_num += 1;
                }
            }
            lines.push(Line::from(""));
        }
        // The quote bar leads every row, wrapped ones included, so the body is
        // rendered into the room it leaves and the bar is laid over each row.
        NodeValue::BlockQuote => {
            // Dashed, so a quote inside a comment body cannot be mistaken for
            // the solid rail the conversation draws down a thread.
            let bar = "\u{2506} ";
            let inner = width
                .saturating_sub(wrap::width(indent))
                .saturating_sub(wrap::width(bar));
            let mut sub_lines = Vec::new();
            for child in node.children() {
                render_node(child, &mut sub_lines, "", ctx, inner);
            }
            for line in sub_lines {
                let mut spans = vec![
                    Span::raw(indent.to_string()),
                    Span::styled(
                        bar,
                        Style::default()
                            .fg(styles::BORDER_ACTIVE)
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                for span in line.spans {
                    spans.push(Span::styled(
                        span.content.to_string(),
                        span.style.fg(styles::TEXT_DIM),
                    ));
                }
                lines.push(Line::from(spans));
            }
        }
        NodeValue::ThematicBreak => {
            lines.push(Line::from(vec![
                Span::raw(indent.to_string()),
                Span::styled(
                    "────────────────────────────────",
                    Style::default().fg(styles::BORDER),
                ),
            ]));
            lines.push(Line::from(""));
        }
        NodeValue::Table(..) => {
            render_table(node, lines, indent, ctx);
            lines.push(Line::from(""));
        }
        NodeValue::HtmlBlock(hb) => {
            for line in hb.literal.lines() {
                lines.extend(wrap::hanging(
                    &[Span::raw(indent.to_string())],
                    &[Span::styled(
                        line.to_string(),
                        Style::default().fg(styles::TEXT_DIM),
                    )],
                    width,
                ));
            }
        }
        _ => {
            for child in node.children() {
                render_node(child, lines, indent, ctx, width);
            }
        }
    }
}

fn render_list_item<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    ctx: &mut InlineCtx,
    list_type: ListType,
    num: usize,
    width: usize,
) {
    let bullet = match list_type {
        ListType::Bullet => "  • ".to_string(),
        ListType::Ordered => format!("  {num}. "),
    };

    // Check if the first child is a TaskItem
    let mut children = node.children().peekable();
    let (prefix, prefix_style) = if let Some(first) = children.peek() {
        if let NodeValue::TaskItem(task) = &first.data.borrow().value {
            let checked = task.symbol.is_some();
            let p = if checked {
                format!("{indent}{bullet}✓ ")
            } else {
                format!("{indent}{bullet}○ ")
            };
            let s = if checked {
                Style::default().fg(styles::GREEN)
            } else {
                Style::default().fg(styles::TEXT_DIM)
            };
            // Skip the TaskItem node itself
            let _ = children.next();
            (p, s)
        } else {
            (
                format!("{indent}{bullet}"),
                match list_type {
                    ListType::Bullet => Style::default().fg(styles::CYAN),
                    ListType::Ordered => Style::default().fg(styles::BLUE),
                },
            )
        }
    } else {
        (
            format!("{indent}{bullet}"),
            Style::default().fg(styles::CYAN),
        )
    };

    let sub_indent = format!("{indent}    ");
    let mut first = true;
    for child in children {
        if first {
            first = false;
            let mut body = Vec::new();
            collect_inline(child, &mut body, ctx);
            lines.extend(wrap::hanging(
                &[Span::styled(prefix.clone(), prefix_style)],
                &body,
                width,
            ));
        } else {
            render_node(child, lines, &sub_indent, ctx, width);
        }
    }
}

fn collect_inline<'a>(node: &'a AstNode<'a>, spans: &mut Vec<Span<'static>>, ctx: &mut InlineCtx) {
    match &node.data.borrow().value {
        NodeValue::Text(text) => {
            spans.push(Span::styled(text.to_string(), ctx.style()));
        }
        NodeValue::Code(code) => {
            let mut c = ctx.clone();
            c.code = true;
            spans.push(Span::styled(format!(" {} ", code.literal), c.style()));
        }
        NodeValue::Emph => {
            let prev = ctx.italic;
            ctx.italic = true;
            for child in node.children() {
                collect_inline(child, spans, ctx);
            }
            ctx.italic = prev;
        }
        NodeValue::Strong => {
            let prev = ctx.bold;
            ctx.bold = true;
            for child in node.children() {
                collect_inline(child, spans, ctx);
            }
            ctx.bold = prev;
        }
        NodeValue::Strikethrough => {
            let prev = ctx.strikethrough;
            ctx.strikethrough = true;
            for child in node.children() {
                collect_inline(child, spans, ctx);
            }
            ctx.strikethrough = prev;
        }
        NodeValue::Link(link) => {
            let mut text_spans = Vec::new();
            for child in node.children() {
                collect_inline(child, &mut text_spans, ctx);
            }
            let text: String = text_spans.iter().map(|s| s.content.as_ref()).collect();
            if text == link.url || text.is_empty() {
                spans.push(Span::styled(
                    link.url.clone(),
                    Style::default()
                        .fg(styles::BLUE)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            } else {
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(styles::BLUE)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                spans.push(Span::styled(
                    format!(" ({})", link.url),
                    Style::default().fg(styles::TEXT_DIM),
                ));
            }
        }
        NodeValue::Image(_link) => {
            let mut text_spans = Vec::new();
            for child in node.children() {
                collect_inline(child, &mut text_spans, ctx);
            }
            let alt: String = text_spans.iter().map(|s| s.content.as_ref()).collect();
            let label = if alt.is_empty() {
                "image".to_string()
            } else {
                alt
            };
            spans.push(Span::styled(
                format!("[{label}]"),
                Style::default()
                    .fg(styles::TEXT_DIM)
                    .add_modifier(Modifier::ITALIC),
            ));
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            spans.push(Span::raw(" "));
        }
        NodeValue::HtmlInline(html) => {
            spans.push(Span::styled(
                html.clone(),
                Style::default().fg(styles::TEXT_DIM),
            ));
        }
        _ => {
            for child in node.children() {
                collect_inline(child, spans, ctx);
            }
        }
    }
}

fn render_table<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    ctx: &mut InlineCtx,
) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut is_header = Vec::new();
    for row_node in node.children() {
        if let NodeValue::TableRow(header) = &row_node.data.borrow().value {
            is_header.push(*header);
        } else {
            is_header.push(false);
        }
        let mut row = Vec::new();
        for cell_node in row_node.children() {
            let mut spans = Vec::new();
            collect_inline(cell_node, &mut spans, ctx);
            let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
            row.push(text);
        }
        rows.push(row);
    }

    // Columns are sized and padded in display columns, so a cell that is not
    // plain ASCII still lines its separator up with the rest.
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; col_count];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                widths[i] = widths[i].max(wrap::width(cell));
            }
        }
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let mut spans = vec![Span::raw(format!("{indent}  "))];
        for (i, cell) in row.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            let pad = " ".repeat(w.saturating_sub(wrap::width(cell)));
            let style = if is_header.get(row_idx) == Some(&true) {
                Style::default()
                    .fg(styles::BLUE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::TEXT)
            };
            spans.push(Span::styled(format!("{cell}{pad}"), style));
            if i + 1 < row.len() {
                spans.push(Span::styled(" │ ", Style::default().fg(styles::BORDER)));
            }
        }
        lines.push(Line::from(spans));

        if is_header.get(row_idx) == Some(&true) {
            let sep: String = widths
                .iter()
                .map(|w| "─".repeat(*w))
                .collect::<Vec<_>>()
                .join("─┼─");
            lines.push(Line::from(vec![
                Span::raw(format!("{indent}  ")),
                Span::styled(sep, Style::default().fg(styles::BORDER)),
            ]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render, render_comment};

    /// The plain text of each rendered row.
    fn rows(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    /// A body longer than the pane keeps the comment gutter on every row it
    /// takes, which is what `Paragraph`'s own wrapping cannot do.
    #[test]
    fn a_wrapped_comment_keeps_its_gutter_on_every_row() {
        let body = "The pipeline failed because the runner ran out of disk space.";
        let rendered = rows(&render_comment(body, 30));
        assert!(rendered.len() > 1, "{rendered:?} should have wrapped");
        for row in &rendered {
            assert!(row.starts_with("  \u{2502} "), "{row:?} lost the gutter");
            assert!(super::wrap::width(row) <= 30, "{row:?} overflows the pane");
        }
    }

    /// A bullet's text holds the bullet's column when it wraps, rather than
    /// falling back to the left edge.
    #[test]
    fn a_wrapped_bullet_holds_its_column() {
        let rendered = rows(&render(
            "- the first bullet is long enough that it has to wrap\n- short",
            "  ",
            32,
        ));
        assert_eq!(rendered[0], "    \u{2022} the first bullet is long");
        assert_eq!(rendered[1], "      enough that it has to wrap");
    }

    /// Widths are display columns, so the separator of a table holding CJK text
    /// lands in the same column on every row.
    #[test]
    fn a_table_aligns_columns_holding_wide_glyphs() {
        let md = "| team | note |\n|---|---|\n| 統合制御 | ok |\n| controls | ok |\n";
        let rendered = rows(&render(md, "", 40));
        let bars: Vec<Option<usize>> = rendered
            .iter()
            .filter(|row| row.contains('\u{2502}'))
            .map(|row| {
                row.char_indices()
                    .find(|&(_, c)| c == '\u{2502}')
                    .map(|(i, _)| super::wrap::width(&row[..i]))
            })
            .collect();
        assert!(bars.len() >= 3, "{rendered:?}");
        assert!(
            bars.windows(2).all(|w| w[0] == w[1]),
            "separators misaligned: {bars:?} in {rendered:?}"
        );
    }

    /// A zero width means the caller does not know the pane yet, so nothing
    /// wraps and the row stays whole.
    #[test]
    fn a_zero_width_leaves_a_paragraph_unwrapped() {
        let body = "a fairly long single paragraph that would certainly wrap somewhere";
        assert_eq!(rows(&render(body, "", 0))[0], body.to_string());
    }
}
