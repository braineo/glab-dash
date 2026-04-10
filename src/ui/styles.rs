use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

// ── Tokyo Night Color Palette ──

// Backgrounds (layered: base < surface < overlay)
pub const BASE: Color = Color::Rgb(26, 27, 38);
pub const SURFACE: Color = Color::Rgb(36, 40, 59);
pub const OVERLAY: Color = Color::Rgb(52, 59, 88);
pub const HIGHLIGHT: Color = Color::Rgb(41, 46, 66);

// Foregrounds
pub const TEXT: Color = Color::Rgb(169, 177, 214);
pub const TEXT_DIM: Color = Color::Rgb(86, 95, 137);
pub const TEXT_BRIGHT: Color = Color::Rgb(200, 211, 245);

// Overlay-specific foregrounds (WCAG AA 4.5:1 against OVERLAY bg #343b58)
pub const OVERLAY_TEXT: Color = Color::Rgb(192, 202, 233);
pub const OVERLAY_TEXT_DIM: Color = Color::Rgb(148, 160, 197);

// Accents
pub const BLUE: Color = Color::Rgb(122, 162, 247);
pub const CYAN: Color = Color::Rgb(125, 207, 255);
pub const GREEN: Color = Color::Rgb(158, 206, 106);
pub const RED: Color = Color::Rgb(247, 118, 142);
pub const YELLOW: Color = Color::Rgb(224, 175, 104);
pub const MAGENTA: Color = Color::Rgb(187, 154, 247);
pub const ORANGE: Color = Color::Rgb(255, 158, 100);
pub const TEAL: Color = Color::Rgb(115, 218, 202);

// Borders
pub const BORDER: Color = Color::Rgb(59, 66, 97);
pub const BORDER_ACTIVE: Color = Color::Rgb(122, 162, 247);

// Scoped label colors
pub const LABEL_SCOPE: Color = Color::Rgb(137, 180, 250);
pub const LABEL_SCOPE_SEP: Color = Color::Rgb(86, 95, 137);
pub const LABEL_VALUE: Color = Color::Rgb(115, 218, 202);

// ── Icons ──

pub const ICON_OPEN: &str = "●";
pub const ICON_CLOSED: &str = "✗";
pub const ICON_MERGED: &str = "◆";
pub const ICON_DRAFT: &str = "◌";
pub const ICON_PIPELINE_OK: &str = "✓";
pub const ICON_PIPELINE_FAIL: &str = "✗";
pub const ICON_PIPELINE_RUN: &str = "⟳";
pub const ICON_PIPELINE_WAIT: &str = "◷";
pub const ICON_SELECTOR: &str = " ▸ ";
pub const ICON_SEPARATOR: &str = " │ ";
pub const ICON_SECTION: &str = "◆";
pub const ICON_ARROW: &str = "→";
pub const ICON_CHECK: &str = "✓";
pub const ICON_UNCHECK: &str = "○";

// ── Block Helpers ──

pub fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD))
}

pub fn overlay_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_ACTIVE))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(OVERLAY))
}

// ── Label Rendering ──

/// Render a label as styled spans. Scoped labels (containing `::`) get
/// the scope in LABEL_SCOPE and value in LABEL_VALUE with a dimmed `::`.
/// Regular labels use TEAL.
pub fn label_spans(label: &str) -> Vec<Span<'static>> {
    if let Some((scope, value)) = label.split_once("::") {
        vec![
            Span::styled(
                scope.to_string(),
                Style::default().fg(LABEL_SCOPE).add_modifier(Modifier::BOLD),
            ),
            Span::styled("::", Style::default().fg(LABEL_SCOPE_SEP)),
            Span::styled(value.to_string(), Style::default().fg(LABEL_VALUE)),
        ]
    } else {
        vec![Span::styled(label.to_string(), Style::default().fg(TEAL))]
    }
}

/// Render a list of labels into a single Line with comma separators.
pub fn labels_line(labels: &[String]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(", ", Style::default().fg(TEXT_DIM)));
        }
        spans.extend(label_spans(label));
    }
    Line::from(spans)
}

/// Render labels compactly for table cells: truncate to fit width.
pub fn labels_compact(labels: &[String], max_width: usize) -> String {
    if labels.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    let mut remaining = labels.len();
    for (i, label) in labels.iter().enumerate() {
        let sep = if i > 0 { ", " } else { "" };
        let candidate = format!("{sep}{label}");
        remaining -= 1;
        // Reserve space for "+N" suffix if needed
        let suffix_len = if remaining > 0 {
            format!("+{remaining}").len() + 1
        } else {
            0
        };
        if result.len() + candidate.len() + suffix_len > max_width && i > 0 {
            result.push_str(&format!(" +{}", labels.len() - i));
            return result;
        }
        result.push_str(&candidate);
    }
    result
}

// ── Styles ──

pub fn title_style() -> Style {
    Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(HIGHLIGHT)
        .fg(TEXT_BRIGHT)
        .add_modifier(Modifier::BOLD)
}

pub fn header_style() -> Style {
    Style::default()
        .fg(BLUE)
        .add_modifier(Modifier::BOLD)
}

pub fn status_bar_style() -> Style {
    Style::default().bg(SURFACE).fg(TEXT)
}

pub fn filter_chip_style() -> Style {
    Style::default()
        .fg(YELLOW)
        .bg(Color::Rgb(56, 52, 34))
}

pub fn filter_chip_selected_style() -> Style {
    Style::default()
        .fg(BASE)
        .bg(YELLOW)
        .add_modifier(Modifier::BOLD)
}

pub fn state_style(state: &str) -> Style {
    match state {
        "opened" => Style::default().fg(GREEN),
        "closed" => Style::default().fg(RED),
        "merged" => Style::default().fg(MAGENTA),
        "locked" => Style::default().fg(TEXT_DIM),
        _ => Style::default().fg(TEXT),
    }
}

/// Style for work item custom status names.
/// Style for work item custom status names.
/// Colors are chosen based on the status color returned from GitLab when
/// available; otherwise we fall back to keyword matching.
pub fn status_style_from_color(color: Option<&str>) -> Option<Style> {
    let hex = color?.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Style::default().fg(Color::Rgb(r, g, b)))
}

pub fn status_style(status: &str) -> Style {
    let lower = status.to_lowercase();
    if lower.contains("done") {
        Style::default().fg(GREEN)
    } else if lower.contains("progress") {
        Style::default().fg(BLUE)
    } else if lower.contains("won't do") || lower.contains("wont do") {
        Style::default().fg(RED)
    } else if lower.contains("duplicate") {
        Style::default().fg(TEXT_DIM)
    } else if lower.contains("todo") || lower.contains("to do") {
        Style::default().fg(CYAN)
    } else if lower.contains("backlog") {
        Style::default().fg(TEXT_DIM)
    } else if lower.contains("draft") {
        Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC)
    } else if lower.contains("block") {
        Style::default().fg(ORANGE)
    } else if lower.contains("review") || lower.contains("await") {
        Style::default().fg(MAGENTA)
    } else {
        Style::default().fg(YELLOW)
    }
}

/// Icon for work item custom status.
pub fn status_icon(status: &str) -> &'static str {
    let lower = status.to_lowercase();
    if lower.contains("done") {
        ICON_CHECK
    } else if lower.contains("progress") {
        ICON_PIPELINE_RUN
    } else if lower.contains("won't do") || lower.contains("wont do") {
        ICON_CLOSED
    } else if lower.contains("duplicate") {
        ICON_CLOSED
    } else if lower.contains("block") {
        "⊘"
    } else if lower.contains("review") || lower.contains("await") {
        ICON_PIPELINE_WAIT
    } else if lower.contains("draft") {
        ICON_DRAFT
    } else {
        ICON_OPEN
    }
}

pub fn label_style() -> Style {
    Style::default().fg(TEAL)
}

pub fn draft_style() -> Style {
    Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC)
}

pub fn error_style() -> Style {
    Style::default().fg(RED).add_modifier(Modifier::BOLD)
}

pub fn help_key_style() -> Style {
    Style::default().fg(BLUE).add_modifier(Modifier::BOLD)
}

pub fn help_desc_style() -> Style {
    Style::default().fg(TEXT_DIM)
}

// Overlay-specific help styles (higher contrast for WCAG AA on OVERLAY bg)
pub fn overlay_key_style() -> Style {
    Style::default()
        .fg(CYAN)
        .add_modifier(Modifier::BOLD)
}

pub fn overlay_desc_style() -> Style {
    Style::default().fg(OVERLAY_TEXT_DIM)
}

pub fn overlay_text_style() -> Style {
    Style::default().fg(OVERLAY_TEXT)
}

pub fn source_tracking_style() -> Style {
    Style::default().fg(GREEN)
}

pub fn source_external_style() -> Style {
    Style::default().fg(ORANGE)
}

pub fn pipeline_style(status: &str) -> Style {
    match status {
        "success" | "passed" => Style::default().fg(GREEN),
        "failed" => Style::default().fg(RED),
        "running" => Style::default().fg(BLUE),
        "pending" => Style::default().fg(YELLOW),
        "canceled" | "skipped" => Style::default().fg(TEXT_DIM),
        _ => Style::default().fg(TEXT),
    }
}

pub fn row_alt_style() -> Style {
    Style::default().bg(Color::Rgb(30, 32, 45))
}

pub fn section_header_style() -> Style {
    Style::default().fg(MAGENTA).add_modifier(Modifier::BOLD)
}
