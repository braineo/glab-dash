use ratatui::style::{Color, Modifier, Style};

// ── Color Palette ──
pub const CYAN: Color = Color::Cyan;
pub const GREEN: Color = Color::Green;
pub const YELLOW: Color = Color::Yellow;
pub const RED: Color = Color::Red;
pub const MAGENTA: Color = Color::Magenta;
pub const BLUE: Color = Color::Blue;
pub const GRAY: Color = Color::DarkGray;
pub const WHITE: Color = Color::White;

// ── Styles ──

pub fn title_style() -> Style {
    Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(Color::Rgb(40, 40, 60))
        .add_modifier(Modifier::BOLD)
}

pub fn header_style() -> Style {
    Style::default()
        .fg(CYAN)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

pub fn status_bar_style() -> Style {
    Style::default().bg(Color::Rgb(30, 30, 50)).fg(WHITE)
}

pub fn filter_chip_style() -> Style {
    Style::default().fg(YELLOW).bg(Color::Rgb(50, 50, 30))
}

pub fn filter_chip_selected_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(YELLOW)
        .add_modifier(Modifier::BOLD)
}

pub fn state_style(state: &str) -> Style {
    match state {
        "opened" => Style::default().fg(GREEN),
        "closed" => Style::default().fg(RED),
        "merged" => Style::default().fg(MAGENTA),
        "locked" => Style::default().fg(GRAY),
        _ => Style::default().fg(WHITE),
    }
}

pub fn label_style() -> Style {
    Style::default().fg(BLUE)
}

pub fn draft_style() -> Style {
    Style::default().fg(GRAY).add_modifier(Modifier::ITALIC)
}

pub fn error_style() -> Style {
    Style::default().fg(RED).add_modifier(Modifier::BOLD)
}

pub fn help_key_style() -> Style {
    Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
}

pub fn help_desc_style() -> Style {
    Style::default().fg(GRAY)
}

pub fn source_tracking_style() -> Style {
    Style::default().fg(GREEN)
}

pub fn source_external_style() -> Style {
    Style::default().fg(YELLOW)
}

pub fn pipeline_style(status: &str) -> Style {
    match status {
        "success" | "passed" => Style::default().fg(GREEN),
        "failed" => Style::default().fg(RED),
        "running" | "pending" => Style::default().fg(YELLOW),
        "canceled" | "skipped" => Style::default().fg(GRAY),
        _ => Style::default().fg(WHITE),
    }
}
