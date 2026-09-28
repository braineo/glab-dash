use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::{Arena, Options, parse_document};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::{highlight, styles, wrap};

/// One [`Line`] per screen row.  A `width` of zero wraps nothing.
pub fn render(text: &str, indent: &str, width: usize) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, text, &opts);
    let mut lines = Vec::new();
    render_node(root, &mut lines, indent, &mut InlineCtx::default(), width);
    lines
}

fn options() -> Options<'static> {
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;
    opts.extension.footnotes = true;
    // GitLab renders these too; without them a `[!note]` marker, a literal
    // `:tada:`, `$x$`, a `>>>` quote and front matter all show as source.
    opts.extension.alerts = true;
    opts.extension.multiline_block_quotes = true;
    opts.extension.shortcodes = true;
    opts.extension.math_dollars = true;
    opts.extension.math_code = true;
    opts.extension.front_matter_delimiter = Some("---".to_string());
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
        let mut s = Style::default().fg(styles::text());
        if self.code {
            s = s.fg(styles::orange()).bg(styles::code_bg());
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
                            .fg(styles::magenta())
                            .add_modifier(Modifier::BOLD),
                    ),
                ],
                &body,
                width,
            ));
            lines.push(Line::from(""));
        }
        // Reflowing code would move the line breaks its meaning rests on, so
        // a row wider than the pane is clipped instead.
        NodeValue::CodeBlock(cb) => {
            let code_bg = styles::code_bg();
            let panel = styles::theme().code_bg;
            if cb.info.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled("╭───", Style::default().fg(styles::border())),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(indent.to_string()),
                    Span::styled(
                        format!("╭─ {} ", cb.info),
                        Style::default().fg(styles::border()),
                    ),
                ]));
            }
            let body = cb.literal.trim_end();
            let rail = Span::styled("│ ", Style::default().fg(styles::border()));
            match highlight::code_lines(&cb.info, body, styles::theme_name(), panel) {
                Some(rows) => {
                    for row in rows {
                        let mut spans = vec![Span::raw(indent.to_string()), rail.clone()];
                        spans.extend(row);
                        lines.push(Line::from(spans));
                    }
                }
                None => {
                    for code_line in body.lines() {
                        let expanded = code_line.replace('\t', "    ");
                        lines.push(Line::from(vec![
                            Span::raw(indent.to_string()),
                            rail.clone(),
                            Span::styled(
                                expanded,
                                Style::default().fg(styles::orange()).bg(code_bg),
                            ),
                        ]));
                    }
                }
            }
            lines.push(Line::from(vec![
                Span::raw(indent.to_string()),
                Span::styled("╰───", Style::default().fg(styles::border())),
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
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(..) => {
            render_quote(node, lines, indent, ctx, width, None);
        }
        NodeValue::Alert(alert) => {
            let title = alert
                .title
                .clone()
                .unwrap_or_else(|| alert.alert_type.default_title().to_string());
            render_quote(node, lines, indent, ctx, width, Some(title));
        }
        NodeValue::ThematicBreak => {
            lines.push(Line::from(vec![
                Span::raw(indent.to_string()),
                Span::styled(
                    "────────────────────────────────",
                    Style::default().fg(styles::border()),
                ),
            ]));
            lines.push(Line::from(""));
        }
        NodeValue::Table(..) => {
            render_table(node, lines, indent, ctx, width);
            lines.push(Line::from(""));
        }
        NodeValue::HtmlBlock(hb) => {
            for line in hb.literal.lines() {
                lines.extend(wrap::hanging(
                    &[Span::raw(indent.to_string())],
                    &[Span::styled(
                        line.to_string(),
                        Style::default().fg(styles::text_dim()),
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

/// `title` heads the quote, which is what makes it an alert.
fn render_quote<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    ctx: &mut InlineCtx,
    width: usize,
    title: Option<String>,
) {
    // Dashed, so it is not mistaken for the conversation's solid thread rail.
    let bar = "\u{2506} ";
    let inner = width
        .saturating_sub(wrap::width(indent))
        .saturating_sub(wrap::width(bar));
    let mut sub_lines = Vec::new();
    if let Some(title) = title {
        sub_lines.push(Line::from(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }
    for child in node.children() {
        render_node(child, &mut sub_lines, "", ctx, inner);
    }
    for line in sub_lines {
        let mut spans = vec![
            Span::raw(indent.to_string()),
            Span::styled(
                bar,
                Style::default()
                    .fg(styles::border_active())
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        for span in line.spans {
            spans.push(Span::styled(
                span.content.to_string(),
                span.style.fg(styles::text_dim()),
            ));
        }
        lines.push(Line::from(spans));
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
                Style::default().fg(styles::green())
            } else {
                Style::default().fg(styles::text_dim())
            };
            let _ = children.next();
            (p, s)
        } else {
            (
                format!("{indent}{bullet}"),
                match list_type {
                    ListType::Bullet => Style::default().fg(styles::cyan()),
                    ListType::Ordered => Style::default().fg(styles::blue()),
                },
            )
        }
    } else {
        (
            format!("{indent}{bullet}"),
            Style::default().fg(styles::cyan()),
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
        // Not typeset in a terminal, so it is shown as a code span is.
        NodeValue::Math(math) => {
            let mut c = ctx.clone();
            c.code = true;
            spans.push(Span::styled(format!(" {} ", math.literal), c.style()));
        }
        NodeValue::ShortCode(short) => {
            spans.push(Span::styled(short.emoji.clone(), ctx.style()));
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
                        .fg(styles::blue())
                        .add_modifier(Modifier::UNDERLINED),
                ));
            } else {
                spans.push(Span::styled(
                    text,
                    Style::default()
                        .fg(styles::blue())
                        .add_modifier(Modifier::UNDERLINED),
                ));
                spans.push(Span::styled(
                    format!(" ({})", link.url),
                    Style::default().fg(styles::text_dim()),
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
                    .fg(styles::text_dim())
                    .add_modifier(Modifier::ITALIC),
            ));
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            spans.push(Span::raw(" "));
        }
        NodeValue::HtmlInline(html) => {
            spans.push(Span::styled(
                html.clone(),
                Style::default().fg(styles::text_dim()),
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
    width: usize,
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
                widths[i] = widths[i].max(wrap::width(cell));
            }
        }
    }

    // Clipping at the pane edge would drop whole columns with nothing to show
    // they were there.
    let budget = width
        .saturating_sub(wrap::width(indent) + 2)
        .saturating_sub(3 * col_count.saturating_sub(1));
    while width > 0 && widths.iter().sum::<usize>() > budget {
        let Some(widest) = widths.iter_mut().filter(|w| **w > 1).max() else {
            break;
        };
        *widest -= 1;
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let mut spans = vec![Span::raw(format!("{indent}  "))];
        for (i, cell) in row.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(0);
            let cell = wrap::truncate(cell, w);
            let pad = " ".repeat(w.saturating_sub(wrap::width(&cell)));
            let style = if is_header.get(row_idx) == Some(&true) {
                Style::default()
                    .fg(styles::blue())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(styles::text())
            };
            spans.push(Span::styled(format!("{cell}{pad}"), style));
            if i + 1 < row.len() {
                spans.push(Span::styled(" │ ", Style::default().fg(styles::border())));
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
                Span::styled(sep, Style::default().fg(styles::border())),
            ]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render;

    fn rows(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

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

    #[test]
    fn a_wide_table_shrinks_to_the_pane() {
        let md = "| stage | job | why it failed |\n|---|---|---|\n\
                  | build | compile-release-x86 | ran out of disk space |\n";
        let rendered = rows(&render(md, "", 40));
        assert!(rendered.len() >= 3, "{rendered:?}");
        for row in &rendered {
            assert!(super::wrap::width(row) <= 40, "{row:?} overflows the pane");
        }
        assert!(
            rendered[2].contains('\u{2026}'),
            "{rendered:?} should mark the cells it cut"
        );
    }

    #[test]
    fn gitlab_flavored_syntax_does_not_show_its_source() {
        let cases = [
            ("> [!warning]\n> out of disk.", "Warning", "[!warning]"),
            (">>>\nfenced quote\n>>>", "\u{2506} fenced quote", ">>>"),
            ("shipped :tada: now", "\u{1f389}", ":tada:"),
            ("bound is $O(n)$ here", "O(n)", "$O(n)$"),
            ("inline $`E = mc^2`$", "E = mc^2", "$`"),
            ("---\ntitle: x\n---\n\nbody", "body", "title: x"),
        ];
        for (md, want, unwanted) in cases {
            let rendered = rows(&render(md, "", 44)).join("\n");
            assert!(rendered.contains(want), "{md:?} rendered {rendered:?}");
            assert!(
                !rendered.contains(unwanted),
                "{md:?} leaked its source: {rendered:?}"
            );
        }
    }

    #[test]
    fn a_rule_is_not_mistaken_for_front_matter() {
        let rendered = rows(&render("before\n\n---\n\nafter", "", 20));
        assert!(
            rendered.iter().any(|r| r.contains('\u{2500}')),
            "{rendered:?}"
        );
    }

    #[test]
    fn a_zero_width_leaves_a_paragraph_unwrapped() {
        let body = "a fairly long single paragraph that would certainly wrap somewhere";
        assert_eq!(rows(&render(body, "", 0))[0], body.to_string());
    }
}
