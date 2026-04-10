use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{Arena, Options, parse_document};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::styles;

/// Render a markdown string into styled ratatui Lines.
pub fn render(text: &str, indent: &str) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, text, &opts);
    let mut lines = Vec::new();
    render_node(root, &mut lines, indent, &mut InlineCtx::default());
    lines
}

/// Render markdown for a comment body (with gutter prefix).
pub fn render_comment(text: &str) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, text, &opts);
    let mut lines = Vec::new();
    render_node(root, &mut lines, "", &mut InlineCtx::default());

    let gutter = Span::styled("  │ ", styles::help_desc_style());
    lines
        .into_iter()
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
) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            let mut spans = vec![Span::raw(indent.to_string())];
            collect_inline(node, &mut spans, ctx);
            lines.push(Line::from(spans));
            lines.push(Line::from(""));
        }
        NodeValue::Heading(h) => {
            let level = h.level as usize;
            let prefix = "#".repeat(level);
            let mut spans = vec![
                Span::raw(indent.to_string()),
                Span::styled(
                    format!("{prefix} "),
                    Style::default()
                        .fg(styles::MAGENTA)
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            collect_inline(node, &mut spans, ctx);
            lines.push(Line::from(spans));
            lines.push(Line::from(""));
        }
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
                render_list_item(child, lines, indent, ctx, list.list_type, item_num);
                if list.list_type == ListType::Ordered {
                    item_num += 1;
                }
            }
            lines.push(Line::from(""));
        }
        NodeValue::BlockQuote => {
            let mut sub_lines = Vec::new();
            for child in node.children() {
                render_node(child, &mut sub_lines, "", ctx);
            }
            for line in sub_lines {
                let mut spans = vec![
                    Span::raw(indent.to_string()),
                    Span::styled(
                        "▎ ",
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
                lines.push(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled(line.to_string(), Style::default().fg(styles::TEXT_DIM)),
                ]));
            }
        }
        _ => {
            for child in node.children() {
                render_node(child, lines, indent, ctx);
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
            let mut spans = vec![Span::styled(prefix.clone(), prefix_style)];
            collect_inline(child, &mut spans, ctx);
            lines.push(Line::from(spans));
        } else {
            render_node(child, lines, &sub_indent, ctx);
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

    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; col_count];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let mut spans = vec![Span::raw(format!("{indent}  "))];
        for (i, cell) in row.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            let padded = format!("{cell:<w$}");
            let style = if is_header.get(row_idx) == Some(&true) {
                Style::default()
                    .fg(styles::BLUE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::TEXT)
            };
            spans.push(Span::styled(padded, style));
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
