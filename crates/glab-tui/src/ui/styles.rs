use palette::color_difference::Wcag21RelativeContrast;
use palette::{FromColor, Mix, Oklch, Srgb};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};
use syntect::highlighting::Highlighter;
use syntect::parsing::ScopeStack;

use crate::ui::color::{
    BLACK, DIM_CONTRAST, Rgb, TEXT_CONTRAST, WHITE, color, fit_gamut, hue_distance, readable,
};
use crate::ui::highlight;

pub struct Theme {
    pub name: String,
    pub dark: bool,
    // Layered: base < surface < overlay.
    pub base: Rgb,
    pub surface: Rgb,
    pub overlay: Rgb,
    pub highlight: Rgb,
    pub text: Rgb,
    pub text_dim: Rgb,
    pub text_bright: Rgb,
    /// Carries WCAG AA against `overlay`, which `text` does not.
    pub overlay_text: Rgb,
    pub overlay_text_dim: Rgb,
    pub blue: Rgb,
    pub cyan: Rgb,
    pub green: Rgb,
    pub red: Rgb,
    pub yellow: Rgb,
    pub magenta: Rgb,
    pub orange: Rgb,
    pub teal: Rgb,
    pub border: Rgb,
    pub border_active: Rgb,
    pub filter_chip_bg: Rgb,
    pub sort_chip_bg: Rgb,
    pub row_alt_bg: Rgb,
    pub code_bg: Rgb,
    pub chord_dim: Rgb,
}

pub const DEFAULT_THEME: &str = "TwoDark";

/// Derived up front (~11ms for the set): the picker previews a theme on every
/// cursor move, so deriving lazily would put the work on the keystroke.
static THEMES: LazyLock<Vec<Theme>> = LazyLock::new(|| {
    highlight::theme_names()
        .into_iter()
        .filter_map(|name| {
            let syntax = highlight::theme(&name)?;
            Some(Theme::derive(name, syntax))
        })
        .collect()
});

static ACTIVE: LazyLock<AtomicUsize> = LazyLock::new(|| {
    let idx = THEMES.iter().position(|t| t.name == DEFAULT_THEME);
    AtomicUsize::new(idx.unwrap_or(0))
});

pub fn theme() -> &'static Theme {
    &THEMES[ACTIVE.load(Ordering::Relaxed)]
}

pub fn set_theme(name: &str) -> bool {
    match THEMES.iter().position(|t| t.name == name) {
        Some(idx) => {
            ACTIVE.store(idx, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

pub fn theme_name() -> &'static str {
    &theme().name
}

pub fn theme_names() -> Vec<String> {
    THEMES.iter().map(|t| t.name.clone()).collect()
}

impl Theme {
    fn derive(name: String, syntax: &syntect::highlighting::Theme) -> Self {
        let set = &syntax.settings;
        let base = set
            .background
            .map_or_else(|| hex(0x1A_1B_26), highlight::opaque);
        // Alpha on a tmTheme background means "blend me into the page"; on a
        // foreground it means nothing, and compositing it muddies the text.
        let text = set
            .foreground
            .map_or_else(|| hex(0xA9_B1_D6), highlight::opaque);
        let dark = base.relative_luminance().luma < 0.5;

        let accents = Accents::of(syntax, base);
        // Blends of the two anchors, not the theme's named selection color: a
        // tmTheme picks that to sit behind a few words, and a vivid one reads
        // as a highlighter pen behind a whole modal.
        let surface = lift(base, text, 0.07);
        let overlay = lift(base, text, 0.16);
        // A vivid selection is still refused: the row keeps its own
        // foreground, which a loud wash would bury.
        let selection = set
            .selection
            .or(set.line_highlight)
            .map(|c| composite(c, base))
            .filter(|c| c.relative_contrast(base) <= 2.5);
        let dim = text.mix(base, 0.45);

        Self {
            dark,
            base,
            surface,
            overlay,
            highlight: selection.unwrap_or_else(|| lift(base, text, 0.12)),
            text: readable(text, base, TEXT_CONTRAST),
            text_dim: readable(dim, base, DIM_CONTRAST),
            text_bright: text.mix(if dark { WHITE } else { BLACK }, 0.35),
            overlay_text: readable(text, overlay, TEXT_CONTRAST),
            overlay_text_dim: readable(dim, overlay, DIM_CONTRAST),
            blue: accents.blue,
            cyan: accents.cyan,
            green: accents.green,
            red: accents.red,
            yellow: accents.yellow,
            magenta: accents.magenta,
            orange: accents.orange,
            teal: accents.teal,
            border: lift(base, text, 0.28),
            border_active: accents.blue,
            filter_chip_bg: base.mix(accents.yellow, 0.20),
            sort_chip_bg: base.mix(accents.magenta, 0.20),
            row_alt_bg: lift(base, text, 0.05),
            code_bg: lift(base, text, 0.08),
            chord_dim: readable(text.mix(overlay, 0.5), overlay, DIM_CONTRAST),
            name,
        }
    }
}

/// A rung on the background ladder, as a blend of the two anchors scaled so it
/// does not depend on how far apart this theme put them.
///
/// Blending by a fixed ratio lifts the row stripe 0.158 in Oklch lightness on
/// DarkNeon against 0.019 on Solarized, an eightfold spread.  Stepping by a
/// fixed Oklch lightness instead is not representable: from `#000000` the
/// smallest 8-bit step, `#010101`, is already 0.067, so the three finest rungs
/// quantize back onto the base.  So the blend stays, normalized by the gap.
fn lift(base: Rgb, text: Rgb, ratio: f64) -> Rgb {
    /// The median gap between the two anchors over the bundled themes; a theme
    /// at exactly this gap keeps the ratio it is given.
    const MEDIAN_GAP: f64 = 0.48;
    let gap = (Oklch::from_color(text).l - Oklch::from_color(base).l).abs();
    base.mix(text, (ratio * MEDIAN_GAP / gap.max(0.1)).clamp(0.0, 1.0))
}

struct Accents {
    blue: Rgb,
    cyan: Rgb,
    green: Rgb,
    red: Rgb,
    yellow: Rgb,
    magenta: Rgb,
    orange: Rgb,
    teal: Rgb,
}

const ACCENT_SCOPES: &[&str] = &[
    "keyword",
    "keyword.control",
    "keyword.operator",
    "string",
    "string.quoted",
    "constant.numeric",
    "constant.language",
    "constant.character.escape",
    "comment",
    "entity.name.function",
    "entity.name.type",
    "entity.name.tag",
    "entity.other.attribute-name",
    "variable",
    "variable.parameter",
    "support.function",
    "support.type",
    "support.class",
    "invalid",
    "markup.inserted",
    "markup.deleted",
];

/// Oklch hue in degrees, each named color's sRGB angle taken at a mid-tone.
/// These are the one thing not taken from the theme, because the interface
/// spends them on meaning: `red` is a closed issue and a failed pipeline.
const ACCENT_HUES: [f64; 8] = [
    24.0,  // red
    54.0,  // orange
    95.0,  // yellow
    143.0, // green
    183.0, // teal
    217.0, // cyan
    257.0, // blue
    320.0, // magenta
];

fn hex(rgb: u32) -> Rgb {
    Srgb::<u8>::from(rgb).into_format()
}

impl Accents {
    fn of(syntax: &syntect::highlighting::Theme, base: Rgb) -> Self {
        /// Past this the fill rate stops improving, and a window would reach
        /// beyond the nearest neighbouring target, 30 degrees away.
        const HUE_TOLERANCE: f64 = 30.0;
        /// A theme's grays top out at 0.048; the one real accent this costs is
        /// Nord's `#81a1c1` at 0.059.
        ///
        /// ponytail: slots rank on hue distance alone, so a washed-out
        /// candidate inside the window beats a vivid one — at 0.05 gruvbox's
        /// near-white `#fbf1c7` took the yellow slot from its `#fabd2f`.
        /// Weight the distance by chroma to lower this floor.
        const MIN_CHROMA: f64 = 0.06;
        /// Only GitHub trips this: its `#003300` diff marker is a legible green
        /// that reads as black beside the `#4f824a` the theme actually spends.
        const MIN_LIGHTNESS: f64 = 0.35;

        let highlighter = Highlighter::new(syntax);
        let mut candidates: Vec<(f64, Rgb)> = ACCENT_SCOPES
            .iter()
            .filter_map(|name| {
                let stack = ScopeStack::from_str(name).ok()?;
                let fg = highlighter.style_for_stack(stack.as_slice()).foreground;
                // A scope the theme gives no rule answers with the body
                // foreground, which is not one of its accents.
                if syntax.settings.foreground == Some(fg) {
                    return None;
                }
                let rgb = composite(fg, base);
                let oklch = Oklch::from_color(rgb);
                (oklch.chroma >= MIN_CHROMA && oklch.l >= MIN_LIGHTNESS)
                    .then_some((oklch.hue.into_positive_degrees(), rgb))
            })
            .collect();
        // Several scopes commonly share one color, and two slots must not
        // claim it through different scopes.
        let rendered = |rgb: &Rgb| rgb.into_format::<u8>().into_components();
        candidates.sort_unstable_by_key(|(_, rgb)| rendered(rgb));
        candidates.dedup_by_key(|(_, rgb)| rendered(rgb));

        // Best fit first, not slot order: many themes carry one blue-green
        // that both the cyan and blue targets sit within, and the earlier slot
        // taking it leaves the other on a fallback.
        let mut pairings: Vec<(f64, usize, usize)> = ACCENT_HUES
            .iter()
            .enumerate()
            .flat_map(|(slot, target)| {
                candidates
                    .iter()
                    .enumerate()
                    .filter_map(move |(i, (h, _))| {
                        let d = hue_distance(*h, *target);
                        (d <= HUE_TOLERANCE).then_some((d, slot, i))
                    })
            })
            .collect();
        pairings.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

        let mut chosen: [Option<f64>; ACCENT_HUES.len()] = [None; ACCENT_HUES.len()];
        let mut taken = vec![false; candidates.len()];
        for (_, slot, cand) in pairings {
            if chosen[slot].is_none() && !taken[cand] {
                chosen[slot] = Some(candidates[cand].0);
                taken[cand] = true;
            }
        }

        // Only the hue is the theme's.  A tmTheme never meant these eight as a
        // set, so taking their lightness and chroma too gives eight unrelated
        // colors; at one register they read as a family and still carry the
        // theme's own angles — Solarized's green stays olive.
        let (lightness, chroma) = register(candidates.iter().map(|(_, c)| *c));
        let at = |slot: usize| {
            let hue = chosen[slot].unwrap_or(ACCENT_HUES[slot]);
            readable(
                fit_gamut(Oklch::new(lightness, chroma, hue)),
                base,
                TEXT_CONTRAST,
            )
        };
        Self {
            red: at(0),
            orange: at(1),
            yellow: at(2),
            green: at(3),
            teal: at(4),
            cyan: at(5),
            blue: at(6),
            magenta: at(7),
        }
    }
}

/// In a tmTheme, alpha means a wash laid over the background.
fn composite(c: syntect::highlighting::Color, bg: Rgb) -> Rgb {
    bg.mix(highlight::opaque(c), f64::from(c.a) / 255.0)
}

macro_rules! color_accessors {
    ($($name:ident),* $(,)?) => {
        $(pub fn $name() -> Color { color(theme().$name) })*
    };
}

color_accessors!(
    base,
    surface,
    overlay,
    highlight,
    text,
    text_dim,
    text_bright,
    overlay_text,
    overlay_text_dim,
    blue,
    cyan,
    green,
    red,
    yellow,
    magenta,
    orange,
    teal,
    border,
    border_active,
    filter_chip_bg,
    sort_chip_bg,
    row_alt_bg,
    code_bg,
    chord_dim,
);

/// Label name → GitLab's own hex color.
pub type LabelColors = HashMap<String, String>;

pub const ICON_OPEN: &str = "●";
pub const ICON_CLOSED: &str = "✗";
pub const ICON_MERGED: &str = "◆";
pub const ICON_DRAFT: &str = "◌";

pub const ICON_PIPELINE_OK: &str = "✓";
pub const ICON_PIPELINE_FAIL: &str = "✗";
pub const ICON_PIPELINE_RUN: &str = "⟳";
pub const ICON_PIPELINE_WAIT: &str = "◷";

pub const ICON_REVIEW: &str = "◉";
pub const ICON_BLOCKED: &str = "⊘";
pub const ICON_PROGRESS: &str = "▶";

pub const ICON_DASHBOARD: &str = "◈";
pub const ICON_ISSUES: &str = "◉";
pub const ICON_MRS: &str = "⑂";
pub const ICON_PLANNING: &str = "▦";

pub const ICON_SELECTOR: &str = " ▸ ";
pub const ICON_SEPARATOR: &str = " │ ";
pub const ICON_SECTION: &str = "◆";
pub const ICON_ARROW: &str = "→";
pub const ICON_CHECK: &str = "✓";
pub const ICON_UNCHECK: &str = "○";
pub const ICON_LOADING: &str = "⟳";

pub fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border()))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(cyan()).add_modifier(Modifier::BOLD))
}

pub fn overlay_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_active()))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(cyan()).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(overlay()))
}

/// Needs a Powerline-patched font.
const PL: &str = "\u{E0B0}";

/// Not a `DefaultHasher`: its seed changes between runs, so a label would not
/// keep its color across restarts.
fn djb2(text: &str) -> u32 {
    text.bytes().fold(5381u32, |h, b| {
        h.wrapping_mul(33).wrapping_add(u32::from(b))
    })
}

/// Past twelve at one lightness the hues stop telling apart.
const CHIP_COUNT: usize = 12;

fn accents() -> [Rgb; 8] {
    let t = theme();
    [
        t.red, t.orange, t.yellow, t.green, t.teal, t.cyan, t.blue, t.magenta,
    ]
}

/// Oklch lightness and chroma, as medians: a theme's yellow is far lighter than
/// its blue, and a set built on one color's numbers stops looking like a set.
fn register(colors: impl IntoIterator<Item = Rgb>) -> (f64, f64) {
    let mut ls = Vec::new();
    let mut cs = Vec::new();
    for c in colors {
        let oklch = Oklch::from_color(c);
        ls.push(oklch.l);
        cs.push(oklch.chroma);
    }
    let median = |mut v: Vec<f64>| match v.len() {
        0 => 0.0,
        n => {
            v.sort_unstable_by(f64::total_cmp);
            if n % 2 == 0 {
                f64::midpoint(v[n / 2 - 1], v[n / 2])
            } else {
                v[n / 2]
            }
        }
    };
    (median(ls), median(cs))
}

/// Even steps in Oklch, so they are even to the eye as sRGB and HSL are not.
/// Starting at the theme's own blue, so two themes sharing a register still
/// get different chips.
fn chip_palette() -> [Rgb; CHIP_COUNT] {
    let (l, c) = register(accents());
    let start = Oklch::from_color(theme().blue).hue;
    #[allow(clippy::cast_precision_loss)]
    std::array::from_fn(|i| {
        fit_gamut(Oklch::new(
            l,
            c,
            start + 360.0 * i as f64 / CHIP_COUNT as f64,
        ))
    })
}

fn chip(accent: Rgb) -> (Color, Color) {
    let bg = theme().base.mix(accent, 0.22);
    (color(readable(accent, bg, TEXT_CONTRAST)), color(bg))
}

fn palette_color(text: &str) -> (Color, Color) {
    chip(chip_palette()[djb2(text) as usize % CHIP_COUNT])
}

fn color_pair_from_hex(hex: &str) -> Option<(Color, Color)> {
    let server = Oklch::from_color(Rgb::from_str(hex).ok()?);
    // A gray label has no hue worth keeping, and re-registering one invents it.
    if server.chroma < 0.03 {
        return Some(chip(theme().text_dim));
    }
    let (l, c) = register(accents());
    Some(chip(fit_gamut(Oklch::new(l, c, server.hue))))
}

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

/// A scoped label (`a::b::c`) becomes one colored segment per part.
pub fn label_spans(label: &str, server_color: Option<&str>) -> Vec<Span<'static>> {
    let segments: Vec<&str> = glab_core::label::segments(label).collect();
    let colors = segment_colors(&segments, server_color);

    if segments.len() == 1 {
        let (fg, bg) = colors[0];
        return vec![
            Span::styled(label.to_string(), Style::default().fg(fg).bg(bg)),
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
            let next_bg = colors[i + 1].1;
            spans.push(Span::styled(PL, Style::default().fg(bg).bg(next_bg)));
        } else {
            spans.push(Span::styled(PL, Style::default().fg(bg)));
        }
    }
    spans
}

/// Separators included.
fn label_chip_width(label: &str) -> usize {
    let n: Vec<&str> = glab_core::label::segments(label).collect();
    let text: usize = n.iter().map(|s| s.len()).sum();
    text + n.len()
}

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
                Style::default().fg(text_dim()),
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

pub fn title_style() -> Style {
    Style::default().fg(cyan()).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(highlight())
        .add_modifier(Modifier::BOLD)
}

pub fn header_style() -> Style {
    Style::default().fg(blue()).add_modifier(Modifier::BOLD)
}

pub fn status_bar_style() -> Style {
    Style::default().bg(surface()).fg(text())
}

pub fn filter_chip_style() -> Style {
    Style::default().fg(yellow()).bg(filter_chip_bg())
}

pub fn filter_chip_selected_style() -> Style {
    Style::default()
        .fg(base())
        .bg(yellow())
        .add_modifier(Modifier::BOLD)
}

pub fn sort_chip_style() -> Style {
    Style::default().fg(magenta()).bg(sort_chip_bg())
}

pub fn state_style(state: &str) -> Style {
    match state {
        "opened" => Style::default().fg(green()),
        "closed" => Style::default().fg(red()),
        "merged" => Style::default().fg(magenta()),
        "locked" => Style::default().fg(text_dim()),
        _ => Style::default().fg(text()),
    }
}

pub fn status_style(status: &str) -> Style {
    let lower = status.to_lowercase();
    if lower.contains("done") {
        Style::default().fg(green())
    } else if lower.contains("progress") {
        Style::default().fg(blue())
    } else if lower.contains("won't do") || lower.contains("wont do") {
        Style::default().fg(red())
    } else if lower.contains("duplicate") {
        Style::default()
            .fg(text_dim())
            .add_modifier(Modifier::ITALIC)
    } else if lower.contains("todo") || lower.contains("to do") {
        Style::default().fg(cyan())
    } else if lower.contains("backlog") {
        Style::default().fg(teal())
    } else if lower.contains("draft") {
        Style::default().fg(yellow()).add_modifier(Modifier::ITALIC)
    } else if lower.contains("block") {
        Style::default().fg(orange())
    } else if lower.contains("review") || lower.contains("await") {
        Style::default().fg(magenta())
    } else {
        Style::default().fg(yellow())
    }
}

pub fn status_icon(status: &str) -> &'static str {
    let lower = status.to_lowercase();
    if lower.contains("done") {
        ICON_CHECK
    } else if lower.contains("progress") {
        ICON_PROGRESS
    } else if lower.contains("won't do") || lower.contains("wont do") || lower.contains("duplicate")
    {
        ICON_CLOSED
    } else if lower.contains("block") {
        ICON_BLOCKED
    } else if lower.contains("review") || lower.contains("await") {
        ICON_REVIEW
    } else if lower.contains("draft") {
        ICON_DRAFT
    } else {
        ICON_OPEN
    }
}

pub fn draft_style() -> Style {
    Style::default()
        .fg(text_dim())
        .add_modifier(Modifier::ITALIC)
}

pub fn error_style() -> Style {
    Style::default().fg(red()).add_modifier(Modifier::BOLD)
}

pub fn help_key_style() -> Style {
    Style::default().fg(blue()).add_modifier(Modifier::BOLD)
}

pub fn help_desc_style() -> Style {
    Style::default().fg(text_dim())
}

pub fn overlay_key_style() -> Style {
    Style::default().fg(cyan()).add_modifier(Modifier::BOLD)
}

pub fn overlay_desc_style() -> Style {
    Style::default().fg(overlay_text_dim())
}

pub fn overlay_text_style() -> Style {
    Style::default().fg(overlay_text())
}

pub fn source_tracking_style() -> Style {
    Style::default().fg(green())
}

pub fn source_external_style() -> Style {
    Style::default().fg(orange())
}

pub fn pipeline_style(status: &str) -> Style {
    match status {
        "success" | "passed" => Style::default().fg(green()),
        "failed" => Style::default().fg(red()),
        "running" => Style::default().fg(blue()),
        "pending" => Style::default().fg(yellow()),
        "canceled" | "skipped" => Style::default().fg(text_dim()),
        _ => Style::default().fg(text()),
    }
}

pub fn row_alt_style() -> Style {
    Style::default().bg(row_alt_bg())
}

pub fn chip_sep() -> Span<'static> {
    Span::styled("  \u{00B7}  ", Style::default().fg(border()))
}

pub fn section_header_style() -> Style {
    Style::default().fg(magenta()).add_modifier(Modifier::BOLD)
}

/// The active palette is process-wide and the harness runs tests in parallel,
/// so a test that switches themes would recolor another mid-assert.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_THEME, Rgb, TEST_LOCK, THEMES, label_spans, set_theme, text, theme, theme_name,
        theme_names,
    };
    use palette::Srgb;
    use palette::color_difference::Wcag21RelativeContrast;

    fn rgb(c: ratatui::style::Color) -> Rgb {
        match c {
            ratatui::style::Color::Rgb(r, g, b) => Srgb::new(r, g, b).into_format(),
            other => panic!("theme colors are always RGB, got {other:?}"),
        }
    }

    #[test]
    fn every_bundled_theme_derives_a_legible_palette() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(
            THEMES.len() >= 20,
            "expected bat's whole collection, got {}",
            THEMES.len()
        );
        for theme in THEMES.iter() {
            let (base, overlay) = (theme.base, theme.overlay);
            let name = &theme.name;

            for (label, color) in [
                ("text", theme.text),
                ("red", theme.red),
                ("green", theme.green),
                ("blue", theme.blue),
                ("yellow", theme.yellow),
                ("magenta", theme.magenta),
                ("cyan", theme.cyan),
                ("orange", theme.orange),
                ("teal", theme.teal),
            ] {
                let ratio = color.relative_contrast(base);
                assert!(ratio >= 4.4, "{name}: {label} is {ratio:.2}:1 on the base");
            }
            let dim = theme.text_dim.relative_contrast(base);
            assert!(dim >= 2.9, "{name}: dim text is {dim:.2}:1 on the base");
            let modal = theme.overlay_text.relative_contrast(overlay);
            assert!(
                modal >= 4.4,
                "{name}: modal text is {modal:.2}:1 on overlay"
            );

            assert!(
                overlay.relative_contrast(base) >= 1.12,
                "{name}: the overlay does not lift off the base"
            );
        }
    }

    #[test]
    fn a_theme_keeps_its_own_accents_and_is_classified_by_its_background() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let light = THEMES.iter().filter(|t| !t.dark).count();
        assert!(light >= 3, "the light themes should be recognized as light");

        assert!(set_theme("Solarized (dark)"));
        let solarized = text();
        assert!(set_theme("Dracula"));
        assert_ne!(solarized, text());

        assert!(set_theme(DEFAULT_THEME));
    }

    #[test]
    fn switching_a_theme_recolors_the_palette_and_the_label_chips() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dark_text = text();
        let dark_chip = label_spans("backend", None)[0].style.bg;

        assert!(set_theme("Catppuccin Latte"));
        assert!(!theme().dark);
        assert_ne!(text(), dark_text);
        assert_ne!(label_spans("backend", None)[0].style.bg, dark_chip);

        assert!(!set_theme("no such theme"));
        assert_eq!(theme_name(), "Catppuccin Latte");

        assert!(set_theme(DEFAULT_THEME));
        assert_eq!(text(), dark_text);
    }

    #[test]
    fn the_chip_ring_is_one_register_and_even_hue_steps() {
        use palette::{FromColor, Oklch};

        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for name in theme_names() {
            assert!(set_theme(&name));
            let ring = super::chip_palette().map(Oklch::<f64>::from_color);
            let hue = |c: Oklch<f64>| c.hue.into_positive_degrees();

            // Chroma may fall short of the register where sRGB cannot show it,
            // but never to gray, or the hue stops reading.
            for chip in &ring {
                assert!(
                    (chip.l - ring[0].l).abs() < 0.02,
                    "{name}: chip lightness {:.3} drifts from {:.3}",
                    chip.l,
                    ring[0].l
                );
                assert!(chip.chroma > 0.02, "{name}: a chip came out gray");
            }

            let step = (hue(ring[1]) - hue(ring[0])).rem_euclid(360.0);
            for pair in ring.windows(2) {
                let gap = (hue(pair[1]) - hue(pair[0])).rem_euclid(360.0);
                assert!(
                    (gap - step).abs() < 8.0,
                    "{name}: hue step {gap:.1}° is not the ring's {step:.1}°"
                );
            }
        }
        assert!(set_theme(DEFAULT_THEME));
    }

    #[test]
    fn label_chips_stay_legible_on_every_theme() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for name in theme_names() {
            assert!(set_theme(&name));
            for (label, color) in [
                ("backend", None),
                ("priority::high", Some("#D9534F")),
                ("stale", Some("#666666")),
            ] {
                let span = &label_spans(label, color)[0];
                let ratio =
                    rgb(span.style.fg.unwrap()).relative_contrast(rgb(span.style.bg.unwrap()));
                assert!(ratio >= 4.4, "{name}: chip {label} is {ratio:.2}:1");
            }
        }
        assert!(set_theme(DEFAULT_THEME));
    }

    /// The theme's own choice is the floor, not a fixed ratio: a comment is
    /// meant to recede, and forcing it to body contrast would be as wrong as
    /// letting it vanish.
    #[test]
    fn no_token_reads_worse_on_the_code_panel_than_the_theme_intended() {
        use crate::ui::highlight::code_lines;

        const SAMPLE: &str = "// a comment\nlet s = \"text\";\nfn f(x: u32) -> u32 { x + 1 }";

        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for name in theme_names() {
            assert!(set_theme(&name));
            let (base, panel) = (theme().base, theme().code_bg);
            let on_base = code_lines("rust", SAMPLE, &name, base).expect("rust is known");
            let on_panel = code_lines("rust", SAMPLE, &name, panel).expect("rust is known");

            for (want, got) in on_base.iter().flatten().zip(on_panel.iter().flatten()) {
                if want.content.trim().is_empty() {
                    continue;
                }
                let intended = rgb(want.style.fg.unwrap()).relative_contrast(base).min(4.5);
                let actual = rgb(got.style.fg.unwrap()).relative_contrast(panel);
                assert!(
                    actual >= intended - 0.05,
                    "{name}: {:?} reads at {actual:.2}:1 on the panel, \
                     against {intended:.2}:1 the theme gave it",
                    want.content,
                );
            }
        }
        assert!(set_theme(DEFAULT_THEME));
    }

    #[test]
    fn the_picker_lists_every_theme_and_the_default_is_one_of_them() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let names = theme_names();
        assert!(names.contains(&DEFAULT_THEME.to_string()));
        assert!(names.iter().all(|n| set_theme(n)));
        let _ = set_theme(DEFAULT_THEME);
    }
}

#[cfg(test)]
mod dump {
    #[test]
    #[ignore = "a review aid: prints every derived palette for a human to look at"]
    fn palettes() {
        let h = |c: super::Rgb| format!("#{:x}", c.into_format::<u8>());
        for t in super::THEMES.iter() {
            println!(
                "{:<24} {} base={} text={} ovl={} | r={} g={} y={} b={} m={} c={}",
                t.name,
                if t.dark { "dark " } else { "light" },
                h(t.base),
                h(t.text),
                h(t.overlay),
                h(t.red),
                h(t.green),
                h(t.yellow),
                h(t.blue),
                h(t.magenta),
                h(t.cyan),
            );
        }
    }
}

#[cfg(test)]
mod code_contrast {
    use super::THEMES;
    use crate::ui::highlight::opaque;
    use palette::Srgb;
    use palette::color_difference::Wcag21RelativeContrast;
    use std::str::FromStr;
    use syntect::highlighting::Highlighter;
    use syntect::parsing::ScopeStack;

    #[test]
    #[ignore = "probe"]
    fn comment_over_code_bg() {
        let mut worst: Vec<(f64, f64, String)> = Vec::new();
        for t in THEMES.iter() {
            let syntax = crate::ui::highlight::theme(&t.name).unwrap();
            let h = Highlighter::new(syntax);
            let base = syntax
                .settings
                .background
                .map_or_else(|| Srgb::new(0.0, 0.0, 0.0), opaque);
            for scope in ["comment", "string", "keyword"] {
                let st = ScopeStack::from_str(scope).unwrap();
                let fg = opaque(h.style_for_stack(st.as_slice()).foreground);
                let on_theme = fg.relative_contrast(base);
                let on_code = fg.relative_contrast(t.code_bg);
                worst.push((on_code, on_theme, format!("{} {scope}", t.name)));
            }
        }
        worst.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (on_code, on_theme, what) in worst.iter().take(12) {
            println!("{on_code:5.2} on code_bg (was {on_theme:5.2} on theme bg)  {what}");
        }
    }
}

#[cfg(test)]
mod probe7 {
    #[test]
    #[ignore = "probe"]
    fn raw_settings() {
        for name in crate::ui::highlight::theme_names() {
            let s = &crate::ui::highlight::theme(&name).unwrap().settings;
            let f = |c: Option<syntect::highlighting::Color>| match c {
                Some(c) => format!("#{:02x}{:02x}{:02x}/a{:02x}", c.r, c.g, c.b, c.a),
                None => "        NONE  ".to_string(),
            };
            println!(
                "{:<24} bg={} fg={} sel={}",
                name,
                f(s.background),
                f(s.foreground),
                f(s.selection)
            );
        }
    }
}
