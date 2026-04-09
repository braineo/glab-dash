use ratatui::style::{Color, Modifier, Style};
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
