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
    self, DIM_CONTRAST, Rgb, TEXT_CONTRAST, color, contrast, hue_distance, luminance, mix,
    readable, shift,
};
use crate::ui::highlight;

// ── Themes ──

/// Every color the interface paints with, derived from one syntax theme.
///
/// Nothing here is hand-authored per theme.  A tmTheme names only a handful of
/// editor colors — the background, the default text, the selection, the line
/// wash — and colors its syntax scopes; everything else the dashboard needs is
/// derived from those, so a theme is a name plus the file bat already ships and
/// adding one costs no code at all.
pub struct Theme {
    /// The name the config file and the theme picker use, which is also the
    /// name of the syntax theme code blocks are colored in.
    pub name: String,
    /// Whether the backgrounds sit darker than the text.  The label chips need
    /// to know to choose their own lightness.
    pub dark: bool,
    // Backgrounds (layered: base < surface < overlay)
    pub base: Color,
    pub surface: Color,
    pub overlay: Color,
    pub highlight: Color,
    // Foregrounds
    pub text: Color,
    pub text_dim: Color,
    pub text_bright: Color,
    /// Foreground carrying WCAG AA contrast against `overlay`, for modal text.
    pub overlay_text: Color,
    pub overlay_text_dim: Color,
    // Accents
    pub blue: Color,
    pub cyan: Color,
    pub green: Color,
    pub red: Color,
    pub yellow: Color,
    pub magenta: Color,
    pub orange: Color,
    pub teal: Color,
    // Borders
    pub border: Color,
    pub border_active: Color,
    // Tinted backgrounds
    pub filter_chip_bg: Color,
    pub sort_chip_bg: Color,
    pub row_alt_bg: Color,
    pub code_bg: Color,
    /// The unbound-key gray in the chord popup.
    pub chord_dim: Color,
}

/// The theme every palette falls back to, and the one the picker opens on.
pub const DEFAULT_THEME: &str = "TwoDark";

/// Every bundled theme, derived once and held for the life of the process.
///
/// All of them are built up front rather than on demand: the picker previews a
/// theme on every cursor move, so deriving lazily would put the work on the
/// keystroke instead of the startup, and the whole set costs about 11ms.
static THEMES: LazyLock<Vec<Theme>> = LazyLock::new(|| {
    highlight::theme_names()
        .into_iter()
        .filter_map(|name| {
            let syntax = highlight::theme(&name)?;
            Some(Theme::derive(name, syntax))
        })
        .collect()
});

/// Which theme every color accessor reads from.  A global rather than a value
/// threaded through render: every widget already reaches for its colors by
/// name, and switching a theme repaints the whole frame anyway.
static ACTIVE: LazyLock<AtomicUsize> = LazyLock::new(|| {
    let idx = THEMES.iter().position(|t| t.name == DEFAULT_THEME);
    AtomicUsize::new(idx.unwrap_or(0))
});

/// The theme in effect.
pub fn theme() -> &'static Theme {
    &THEMES[ACTIVE.load(Ordering::Relaxed)]
}

/// Switch to the theme named `name`, or report `false` when no bundled theme
/// goes by it and leave the current one alone.
pub fn set_theme(name: &str) -> bool {
    match THEMES.iter().position(|t| t.name == name) {
        Some(idx) => {
            ACTIVE.store(idx, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// The name of the theme in effect.
pub fn theme_name() -> &'static str {
    &theme().name
}

/// Every bundled theme's name, for the picker.
pub fn theme_names() -> Vec<String> {
    THEMES.iter().map(|t| t.name.clone()).collect()
}

impl Theme {
    /// Derive the whole interface palette from syntax theme `syntax`.
    ///
    /// The background and text are the theme's own.  The layered backgrounds
    /// are its selection and line wash where it names them and blends of the
    /// background toward the text where it does not, so the layering holds its
    /// shape on a theme that specifies almost nothing.  The accents are taken
    /// from the theme's own syntax colors by hue, so each theme keeps its
    /// character, and every one is lifted to a legible contrast against the
    /// background it will be drawn on.
    fn derive(name: String, syntax: &syntect::highlighting::Theme) -> Self {
        let set = &syntax.settings;
        let base = set.background.map_or((26, 27, 38), opaque);
        // Alpha on a background wash means "blend me into the page" and is
        // composited; alpha on a foreground is noise no renderer honors, and
        // compositing it turned gruvbox's cream text to mud.
        let text = set.foreground.map_or((169, 177, 214), opaque);
        let dark = luminance(base) < 0.5;

        let accents = Accents::of(syntax, base);
        // The layered backgrounds are blends of the theme's own two anchors
        // rather than its named colors.  A tmTheme picks its selection to sit
        // behind a few words, and some pick a vivid one — Sublime Snazzy's is
        // bright cyan — which reads as a highlighter pen when it is stretched
        // behind a whole modal.  Blending keeps every theme's panels calm and
        // in its own hue family.
        let surface = mix(base, text, 0.07);
        let overlay = mix(base, text, 0.16);
        // The selected row is the one place a selection color belongs, since
        // that is what the theme chose it for.  A vivid one is still refused:
        // the row keeps its own foreground, which a loud wash would bury.
        let selection = set
            .selection
            .or(set.line_highlight)
            .map(|c| composite(c, base))
            .filter(|c| contrast(*c, base) <= 2.5);
        let dim = mix(text, base, 0.45);

        Self {
            dark,
            base: color(base),
            surface: color(surface),
            overlay: color(overlay),
            highlight: color(selection.unwrap_or_else(|| mix(base, text, 0.12))),
            text: color(readable(text, base, TEXT_CONTRAST)),
            text_dim: color(readable(dim, base, DIM_CONTRAST)),
            text_bright: color(shift(text, dark, 0.35)),
            overlay_text: color(readable(text, overlay, TEXT_CONTRAST)),
            overlay_text_dim: color(readable(dim, overlay, DIM_CONTRAST)),
            blue: color(accents.blue),
            cyan: color(accents.cyan),
            green: color(accents.green),
            red: color(accents.red),
            yellow: color(accents.yellow),
            magenta: color(accents.magenta),
            orange: color(accents.orange),
            teal: color(accents.teal),
            border: color(mix(base, text, 0.28)),
            border_active: color(accents.blue),
            filter_chip_bg: color(mix(base, accents.yellow, 0.20)),
            sort_chip_bg: color(mix(base, accents.magenta, 0.20)),
            row_alt_bg: color(mix(base, text, 0.05)),
            code_bg: color(mix(base, text, 0.08)),
            chord_dim: color(readable(mix(text, overlay, 0.5), overlay, DIM_CONTRAST)),
            name,
        }
    }
}

/// The eight semantic accents, each carrying a meaning the dashboard relies on:
/// green passes, red fails, magenta is merged.
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

/// The scopes an accent is harvested from.  Between them these cover the colors
/// a tmTheme actually spends its palette on, so a theme's own reds and greens
/// are found rather than approximated.
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

/// The hue each accent aims at, in degrees, with the fallback used when the
/// theme has nothing near it.  The fallbacks are the base16-ocean accents.
const ACCENT_TARGETS: &[(f64, Rgb)] = &[
    (0.0, (191, 97, 106)),    // red
    (25.0, (208, 135, 112)),  // orange
    (48.0, (235, 203, 139)),  // yellow
    (120.0, (163, 190, 140)), // green
    (172.0, (150, 181, 180)), // teal
    (192.0, (125, 207, 255)), // cyan
    (215.0, (143, 161, 179)), // blue
    (290.0, (187, 154, 247)), // magenta
];

impl Accents {
    /// Harvest the theme's syntax colors and sort them into the accent slots by
    /// hue, so a theme's own green becomes the color that means "passing".
    ///
    /// A slot with no candidate within [`HUE_TOLERANCE`] keeps its fallback
    /// hue, and every accent — harvested or not — is lifted until it is legible
    /// on the theme's background.
    fn of(syntax: &syntect::highlighting::Theme, base: Rgb) -> Self {
        /// How far, in degrees, a theme color may sit from an accent's target
        /// hue and still stand in for it.  Wide enough to catch a theme's one
        /// green, narrow enough that its red never becomes that green.
        const HUE_TOLERANCE: f64 = 26.0;
        /// The saturation a candidate needs before it counts as a color rather
        /// than one more shade of gray.
        const MIN_SATURATION: f64 = 0.22;
        /// The lightness band a candidate has to sit in.  A theme's cream body
        /// text is technically a yellow, and gruvbox's very nearly became the
        /// one meaning "draft"; the ceiling keeps a near-white out of a slot
        /// whose whole job is to stand apart from the text.
        const LIGHTNESS: std::ops::RangeInclusive<f64> = 0.12..=0.85;

        let highlighter = Highlighter::new(syntax);
        let mut candidates: Vec<(f64, Rgb)> = ACCENT_SCOPES
            .iter()
            .filter_map(|name| {
                let stack = ScopeStack::from_str(name).ok()?;
                let fg = highlighter.style_for_stack(stack.as_slice()).foreground;
                let rgb = composite(fg, base);
                let (h, s, l) = color::rgb_to_hsl(rgb);
                (s >= MIN_SATURATION && LIGHTNESS.contains(&l)).then_some((h, rgb))
            })
            .collect();
        // Several scopes commonly share one color; keeping duplicates would let
        // two slots claim the same color through different scopes.
        candidates.sort_unstable_by_key(|a| a.1);
        candidates.dedup_by_key(|(_, rgb)| *rgb);

        // Slots and colors are matched best-fit-first rather than in slot
        // order.  Many themes carry one blue-green that both the cyan and blue
        // targets sit within, and letting the earlier slot take it left the
        // theme's signature blue standing in for cyan while blue fell back to a
        // generic slate.  Sorting every pairing by how well it fits and handing
        // out the closest first gives each color to the slot that wants it most.
        let mut pairings: Vec<(f64, usize, usize)> = ACCENT_TARGETS
            .iter()
            .enumerate()
            .flat_map(|(slot, (target, _))| {
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

        let mut chosen: [Option<Rgb>; ACCENT_TARGETS.len()] = [None; ACCENT_TARGETS.len()];
        let mut taken = vec![false; candidates.len()];
        for (_, slot, cand) in pairings {
            if chosen[slot].is_none() && !taken[cand] {
                chosen[slot] = Some(candidates[cand].1);
                taken[cand] = true;
            }
        }

        // A slot the theme has no color for keeps its fallback hue, and every
        // accent is lifted until it is legible on this theme's background.
        let at = |slot: usize| {
            let (_, fallback) = ACCENT_TARGETS[slot];
            readable(chosen[slot].unwrap_or(fallback), base, TEXT_CONTRAST)
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

/// Drop a syntect color's alpha by blending it over `bg`, which is what the
/// alpha means in a tmTheme: a wash laid over the background rather than a
/// color in its own right.
fn composite(c: syntect::highlighting::Color, bg: Rgb) -> Rgb {
    let t = f64::from(c.a) / 255.0;
    mix(bg, (c.r, c.g, c.b), t)
}

/// A syntect color with its alpha ignored.
const fn opaque(c: syntect::highlighting::Color) -> Rgb {
    (c.r, c.g, c.b)
}

/// One accessor per theme color, so a call site names the color it wants and
/// gets it from whichever theme is active.
macro_rules! color_accessors {
    ($($name:ident),* $(,)?) => {
        $(pub fn $name() -> Color { theme().$name })*
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

/// Type alias for label name → hex color map (e.g. "#428BCA").
pub type LabelColors = HashMap<String, String>;

// ── Icons ──

// State
pub const ICON_OPEN: &str = "●";
pub const ICON_CLOSED: &str = "✗";
pub const ICON_MERGED: &str = "◆";
pub const ICON_DRAFT: &str = "◌";

// Pipeline
pub const ICON_PIPELINE_OK: &str = "✓";
pub const ICON_PIPELINE_FAIL: &str = "✗";
pub const ICON_PIPELINE_RUN: &str = "⟳";
pub const ICON_PIPELINE_WAIT: &str = "◷";

// Status (work items)
pub const ICON_REVIEW: &str = "◉";
pub const ICON_BLOCKED: &str = "⊘";
pub const ICON_PROGRESS: &str = "▶";

// Tab / view
pub const ICON_DASHBOARD: &str = "◈";
pub const ICON_ISSUES: &str = "◉";
pub const ICON_MRS: &str = "⑂";
pub const ICON_PLANNING: &str = "▦";

// General
pub const ICON_SELECTOR: &str = " ▸ ";
pub const ICON_SEPARATOR: &str = " │ ";
pub const ICON_SECTION: &str = "◆";
pub const ICON_ARROW: &str = "→";
pub const ICON_CHECK: &str = "✓";
pub const ICON_UNCHECK: &str = "○";
pub const ICON_LOADING: &str = "⟳";

// ── Block Helpers ──

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

// ── Color Helpers ──

/// Powerline right-arrow separator (requires Nerd Font / Powerline-patched font).
const PL: &str = "\u{E0B0}";

fn djb2(text: &str) -> u32 {
    text.bytes().fold(5381u32, |h, b| {
        h.wrapping_mul(33).wrapping_add(u32::from(b))
    })
}

/// How many colors label chips are drawn from.
///
/// Twelve, not the sixteen a curated palette carried: at chip size the eye can
/// only tell so many hues apart, and sixteen at one lightness meant several
/// pairs no one could name apart.  Twelve is the classic wheel — thirty degrees
/// a step — and collisions past it cost little, since the label's own text is
/// what identifies it and the color only groups.
const CHIP_COUNT: usize = 12;

/// The theme's own eight accents.
fn accents() -> [Rgb; 8] {
    let t = theme();
    [
        t.red, t.orange, t.yellow, t.green, t.teal, t.cyan, t.blue, t.magenta,
    ]
    .map(color::channels)
}

/// The perceptual register the theme paints its accents in: the median
/// lightness and chroma of the eight.
///
/// The median rather than any one accent, because a theme's own accents are
/// all over the place — its yellow is far lighter than its blue — and a chip
/// set built on that unevenness is what stops looking like a set.  Taking the
/// middle of the theme's spread puts every chip in the same register the theme
/// already works in, and does it without asking whether the theme is dark: a
/// light theme's accents are simply darker, so its median is too.
fn register() -> (f64, f64) {
    let mut ls = [0.0; 8];
    let mut cs = [0.0; 8];
    for (i, accent) in accents().into_iter().enumerate() {
        let (l, c, _) = color::rgb_to_oklch(accent);
        ls[i] = l;
        cs[i] = c;
    }
    let median = |mut v: [f64; 8]| {
        v.sort_unstable_by(f64::total_cmp);
        f64::midpoint(v[3], v[4])
    };
    (median(ls), median(cs))
}

/// The chip colors: `CHIP_COUNT` hues spaced evenly around the wheel, all at
/// the theme's own lightness and chroma.
///
/// Evenly in Oklch, so the steps are evenly spaced to the eye rather than to
/// the arithmetic — sRGB and HSL crowd several distinct-looking colors into the
/// blues and stretch one green across a third of the wheel.  The ring starts at
/// the theme's own blue, so two themes with the same register still get
/// different chips.
fn chip_palette() -> [Rgb; CHIP_COUNT] {
    let (l, c) = register();
    let start = color::rgb_to_oklch(color::channels(theme().blue)).2;
    #[allow(clippy::cast_precision_loss)]
    std::array::from_fn(|i| {
        color::oklch_to_rgb((l, c, start + 360.0 * i as f64 / CHIP_COUNT as f64))
    })
}

/// The (fg, bg) pair for a chip colored `accent`.
///
/// The chip is the theme's backdrop tinted toward the color, with the color
/// itself as the text, lifted only as far as the tint costs it — the same
/// recipe the filter and sort chips already use, so labels sit in the same
/// visual register as the rest of the interface.
fn chip(accent: Rgb) -> (Color, Color) {
    let bg = mix(color::channels(theme().base), accent, 0.22);
    (color(readable(accent, bg, TEXT_CONTRAST)), color(bg))
}

/// Pick one of the theme's chip colors for `text`, deterministically.
fn palette_color(text: &str) -> (Color, Color) {
    chip(chip_palette()[djb2(text) as usize % CHIP_COUNT])
}

/// Parse "#FF0000" → (255, 0, 0).
fn parse_hex_color(hex: &str) -> Option<Rgb> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Derive a chip from a server-provided hex label color.
///
/// GitLab's hue is kept — a red label stays red — but the lightness and chroma
/// come from the theme's register, so the color arrives as one of the family
/// instead of dropping a raw web color into it.  A label colored gray has no
/// hue worth keeping and becomes a neutral chip.
fn color_pair_from_hex(hex: &str) -> Option<(Color, Color)> {
    let (_, chroma, hue) = color::rgb_to_oklch(parse_hex_color(hex)?);
    if chroma < 0.03 {
        return Some(chip(color::channels(theme().text_dim)));
    }
    let (l, c) = register();
    Some(chip(color::oklch_to_rgb((l, c, hue))))
}

// ── Label Rendering ──

/// Resolve the (fg, bg) for each segment of a label.
/// First segment uses server color when available; rest come from the theme.
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
/// Non-scoped labels use server color when available, else the theme's palette.
pub fn label_spans(label: &str, server_color: Option<&str>) -> Vec<Span<'static>> {
    let segments: Vec<&str> = glab_core::label::segments(label).collect();
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
    let n: Vec<&str> = glab_core::label::segments(label).collect();
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

// ── Styles ──

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

/// Icon for work item custom status.
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

// Overlay-specific help styles (higher contrast for WCAG AA on overlay() bg)
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

/// The separator between two metadata chips in a compact header: dim enough to
/// group the chips without competing with them.
pub fn chip_sep() -> Span<'static> {
    Span::styled("  \u{00B7}  ", Style::default().fg(border()))
}

pub fn section_header_style() -> Style {
    Style::default().fg(magenta()).add_modifier(Modifier::BOLD)
}

/// Serializes the tests that read or flip the active palette.  The palette is
/// process-wide, and the test harness runs a binary's tests in parallel, so
/// without this a test that switches themes could recolor another mid-assert.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_THEME, TEST_LOCK, THEMES, contrast, label_spans, set_theme, text, theme,
        theme_name, theme_names,
    };

    /// Turn a ratatui color back into channels, so a test can do contrast math
    /// on what a theme actually exposes.
    fn rgb(c: ratatui::style::Color) -> (u8, u8, u8) {
        match c {
            ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
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
            let base = rgb(theme.base);
            let overlay = rgb(theme.overlay);
            let name = &theme.name;

            // Body text and every accent have to clear AA on the background
            // they are drawn over, whatever the theme itself specified.
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
                let ratio = contrast(rgb(color), base);
                assert!(ratio >= 4.4, "{name}: {label} is {ratio:.2}:1 on the base");
            }
            let dim = contrast(rgb(theme.text_dim), base);
            assert!(dim >= 2.9, "{name}: dim text is {dim:.2}:1 on the base");
            let modal = contrast(rgb(theme.overlay_text), overlay);
            assert!(
                modal >= 4.4,
                "{name}: modal text is {modal:.2}:1 on overlay"
            );

            // A modal that does not separate from the page is not a modal.
            assert!(
                contrast(overlay, base) >= 1.12,
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

        // Solarized's palette is nothing like Dracula's; deriving from each
        // theme's own syntax colors has to show that.
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
        // The chips are tinted from the theme's own backdrop, so a new theme
        // repaints them.
        assert_ne!(label_spans("backend", None)[0].style.bg, dark_chip);

        assert!(!set_theme("no such theme"));
        assert_eq!(theme_name(), "Catppuccin Latte");

        assert!(set_theme(DEFAULT_THEME));
        assert_eq!(text(), dark_text);
    }

    #[test]
    fn the_chip_ring_is_one_register_and_even_hue_steps() {
        use crate::ui::color::rgb_to_oklch;

        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for name in theme_names() {
            assert!(set_theme(&name));
            let ring = super::chip_palette().map(rgb_to_oklch);

            // The whole point of building the ring in Oklch: every chip is a
            // sibling of the others, at one perceptual lightness.  Chroma may
            // fall short of the register where sRGB cannot show it, but never
            // to gray, or the hue stops reading.
            for &(l, c, _) in &ring {
                assert!(
                    (l - ring[0].0).abs() < 0.02,
                    "{name}: chip lightness {l:.3} drifts from {:.3}",
                    ring[0].0
                );
                assert!(c > 0.02, "{name}: a chip came out gray at {c:.3}");
            }

            // Evenly spaced to the eye, which is what sRGB and HSL cannot do:
            // every step around the ring is the same size.
            let step = (ring[1].2 - ring[0].2).rem_euclid(360.0);
            for pair in ring.windows(2) {
                let gap = (pair[1].2 - pair[0].2).rem_euclid(360.0);
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
            // A hashed label, a saturated server color and a gray one.
            for (label, color) in [
                ("backend", None),
                ("priority::high", Some("#D9534F")),
                ("stale", Some("#666666")),
            ] {
                let span = &label_spans(label, color)[0];
                let ratio = contrast(rgb(span.style.fg.unwrap()), rgb(span.style.bg.unwrap()));
                assert!(ratio >= 4.4, "{name}: chip {label} is {ratio:.2}:1");
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
        let h = |c: ratatui::style::Color| match c {
            ratatui::style::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => unreachable!(),
        };
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
    use super::{THEMES, contrast};
    use std::str::FromStr;
    use syntect::highlighting::Highlighter;
    use syntect::parsing::ScopeStack;

    #[test]
    #[ignore = "probe"]
    fn comment_over_code_bg() {
        let rgb = |c: ratatui::style::Color| match c {
            ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
            _ => unreachable!(),
        };
        let mut worst: Vec<(f64, f64, String)> = Vec::new();
        for t in THEMES.iter() {
            let syntax = crate::ui::highlight::theme(&t.name).unwrap();
            let h = Highlighter::new(syntax);
            let base = syntax
                .settings
                .background
                .map_or((0, 0, 0), |c| (c.r, c.g, c.b));
            for scope in ["comment", "string", "keyword"] {
                let st = ScopeStack::from_str(scope).unwrap();
                let fg = h.style_for_stack(st.as_slice()).foreground;
                let fg = (fg.r, fg.g, fg.b);
                let on_theme = contrast(fg, base);
                let on_code = contrast(fg, rgb(t.code_bg));
                worst.push((on_code, on_theme, format!("{} {scope}", t.name)));
            }
        }
        worst.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (on_code, on_theme, what) in worst.iter().take(12) {
            println!("{on_code:5.2} on code_bg (was {on_theme:5.2} on theme bg)  {what}");
        }
    }
}
