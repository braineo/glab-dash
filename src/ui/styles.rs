use std::collections::HashMap;

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
pub const TEXT_DIM: Color = Color::Rgb(115, 125, 165);
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

/// Type alias for label name → hex color map (e.g. "#428BCA").
pub type LabelColors = HashMap<String, String>;

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

// ── Color Helpers ──

/// Powerline right-arrow separator (requires Nerd Font / Powerline-patched font).
const PL: &str = "\u{E0B0}";

/// Curated palette of (fg, bg) chip colors tuned for the Tokyo Night theme.
/// Hand-picked for readability (WCAG AA), visual harmony, and distinctness.
type ChipColor = ((u8, u8, u8), (u8, u8, u8));
const CHIP_PALETTE: &[ChipColor] = &[
    ((148, 200, 240), (25, 45, 70)), // cerulean
    ((170, 220, 195), (22, 52, 42)), // jade
    ((195, 175, 230), (42, 32, 65)), // wisteria
    ((235, 185, 165), (60, 35, 28)), // apricot
    ((180, 210, 155), (34, 50, 25)), // fern
    ((235, 200, 150), (58, 46, 24)), // marigold
    ((160, 195, 220), (26, 42, 58)), // glacier
    ((215, 175, 200), (52, 28, 42)), // orchid
    ((155, 215, 210), (22, 50, 48)), // seafoam
    ((195, 185, 225), (40, 34, 60)), // periwinkle
    ((220, 200, 165), (50, 44, 26)), // wheat
    ((175, 215, 190), (28, 48, 38)), // eucalyptus
    ((200, 195, 150), (45, 42, 25)), // lichen
    ((180, 200, 230), (30, 42, 62)), // cornflower
    ((210, 180, 185), (50, 30, 34)), // dusty rose
    ((165, 210, 180), (25, 48, 34)), // sage
];

fn djb2(text: &str) -> u32 {
    text.bytes().fold(5381u32, |h, b| {
        h.wrapping_mul(33).wrapping_add(u32::from(b))
    })
}

/// Select a (fg, bg) pair from the curated palette using a deterministic hash.
fn palette_color(text: &str) -> (Color, Color) {
    let idx = djb2(text) as usize % CHIP_PALETTE.len();
    let ((fr, fg, fb), (br, bg, bb)) = CHIP_PALETTE[idx];
    (Color::Rgb(fr, fg, fb), Color::Rgb(br, bg, bb))
}

fn hue_to_rgb(p: f64, q: f64, t: f64) -> f64 {
    let mut t = t;
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

#[allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h = h / 360.0;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

#[allow(clippy::many_single_char_names)]
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = f64::from(r) / 255.0;
    let g = f64::from(g) / 255.0;
    let b = f64::from(b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = f64::midpoint(max, min);
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f64::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f64::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

/// Parse "#FF0000" → (255, 0, 0).
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Derive a (fg, bg) chip pair from a server-provided hex label color.
#[allow(clippy::many_single_char_names)]
fn color_pair_from_hex(hex: &str) -> Option<(Color, Color)> {
    let (r, g, b) = parse_hex_color(hex)?;
    let (h, s, _) = rgb_to_hsl(r, g, b);
    let (br, bg, bb) = hsl_to_rgb(h, s.min(0.40), 0.20);
    let (fr, fg, fb) = hsl_to_rgb(h, s.min(0.50), 0.82);
    Some((Color::Rgb(fr, fg, fb), Color::Rgb(br, bg, bb)))
}

// ── Label Rendering ──

/// Resolve the (fg, bg) for each segment of a label.
/// First segment uses server color when available; rest use the curated palette.
fn segment_colors(segments: &[&str], server_color: Option<&str>) -> Vec<(Color, Color)> {
    segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            if i == 0 {
                server_color
                    .and_then(color_pair_from_hex)
                    .unwrap_or_else(|| palette_color(seg))
            } else {
                palette_color(seg)
            }
        })
        .collect()
}

/// Render a label as powerline-style chip spans.
/// Scoped labels (`a::b::c`) become colored segments joined by powerline arrows.
/// Non-scoped labels use server color when available, else curated palette.
pub fn label_spans(label: &str, server_color: Option<&str>) -> Vec<Span<'static>> {
    let segments: Vec<&str> = label.split("::").collect();
    let colors = segment_colors(&segments, server_color);

    if segments.len() == 1 {
        let (fg, bg) = colors[0];
        return vec![
            Span::styled(label.to_string(), Style::default().fg(fg).bg(bg)),
            // Trailing arrow tapers into surrounding background
            Span::styled(PL, Style::default().fg(bg)),
        ];
    }

    let mut spans = Vec::with_capacity(segments.len() * 2 + 1);
    for (i, seg) in segments.iter().enumerate() {
        let (fg, bg) = colors[i];
        let style = if i == 0 {
            Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(fg).bg(bg)
        };
        spans.push(Span::styled((*seg).to_string(), style));

        if i < segments.len() - 1 {
            // Powerline arrow: prev_bg → next_bg
            let next_bg = colors[i + 1].1;
            spans.push(Span::styled(PL, Style::default().fg(bg).bg(next_bg)));
        } else {
            // Trailing arrow tapers into surrounding background
            spans.push(Span::styled(PL, Style::default().fg(bg)));
        }
    }
    spans
}

/// Visual width of a label chip (segments + powerline separators).
fn label_chip_width(label: &str) -> usize {
    let n: Vec<&str> = label.split("::").collect();
    let text: usize = n.iter().map(|s| s.len()).sum();
    // Each segment boundary + trailing arrow
    text + n.len()
}

/// Render labels as chip-style spans for table cells, truncating to fit width.
pub fn labels_compact(labels: &[String], max_width: usize, colors: &LabelColors) -> Line<'static> {
    if labels.is_empty() {
        return Line::from("");
    }
    let mut spans = Vec::new();
    let mut used = 0usize;
    let mut remaining = labels.len();
    for (i, label) in labels.iter().enumerate() {
        let gap = usize::from(i > 0);
        let chip_w = label_chip_width(label);
        remaining -= 1;
        let suffix_len = if remaining > 0 {
            format!("+{remaining}").len() + 1
        } else {
            0
        };
        if used + gap + chip_w + suffix_len > max_width && i > 0 {
            spans.push(Span::styled(
                format!(" +{}", labels.len() - i),
                Style::default().fg(TEXT_DIM),
            ));
            return Line::from(spans);
        }
        if i > 0 {
            spans.push(Span::raw(" "));
            used += 1;
        }
        let color = colors.get(label).map(String::as_str);
        spans.extend(label_spans(label, color));
        used += chip_w;
    }
    Line::from(spans)
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
    Style::default().fg(BLUE).add_modifier(Modifier::BOLD)
}

pub fn status_bar_style() -> Style {
    Style::default().bg(SURFACE).fg(TEXT)
}

pub fn filter_chip_style() -> Style {
    Style::default().fg(YELLOW).bg(Color::Rgb(56, 52, 34))
}

pub fn filter_chip_selected_style() -> Style {
    Style::default()
        .fg(BASE)
        .bg(YELLOW)
        .add_modifier(Modifier::BOLD)
}

pub fn sort_chip_style() -> Style {
    Style::default().fg(MAGENTA).bg(Color::Rgb(48, 36, 56))
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
#[allow(dead_code)]
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
        Style::default().fg(TEXT_DIM).add_modifier(Modifier::ITALIC)
    } else if lower.contains("todo") || lower.contains("to do") {
        Style::default().fg(CYAN)
    } else if lower.contains("backlog") {
        Style::default().fg(TEAL)
    } else if lower.contains("draft") {
        Style::default().fg(YELLOW).add_modifier(Modifier::ITALIC)
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
    } else if lower.contains("won't do") || lower.contains("wont do") || lower.contains("duplicate")
    {
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

#[allow(dead_code)]
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
    Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
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
