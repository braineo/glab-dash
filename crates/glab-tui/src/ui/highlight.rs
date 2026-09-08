//! Syntax coloring for the fenced code blocks in a markdown body.
//!
//! The chrome around a code block comes from the active [`Palette`], so the
//! syntax theme is not free to pick its own background: each palette names the
//! bat theme whose token colors sit closest to it, and only the foregrounds
//! are taken from that theme.  A fence naming no language, or one syntect has
//! no grammar for, is left to the caller to paint flat.
//!
//! [`Palette`]: crate::ui::styles::Palette

use std::sync::LazyLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::SyntaxSet;
use two_face::theme::LazyThemeSet;

/// bat's expanded grammar collection by way of two-face, built once and
/// shared.  It reaches languages syntect's own defaults omit — TOML, TypeScript
/// and friends — which is most of what turns up in a merge request.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_no_newlines);

/// bat's curated themes, each decompressed on first use, so naming one does
/// not pay for the whole collection.
static THEMES: LazyLock<LazyThemeSet> =
    LazyLock::new(|| LazyThemeSet::from(two_face::theme::extra()));

/// The bundled themes that are built for true color, sorted by name.
///
/// two-face carries a few themes whose settings hold ANSI palette indices in
/// place of RGB, marked by a non-opaque background.  The whole interface is
/// derived from RGB, so those are left out rather than rendered as near-black.
pub fn theme_names() -> Vec<String> {
    let mut names: Vec<String> = THEMES
        .theme_names()
        .filter(|name| THEMES.get(name).is_some_and(is_truecolor))
        .map(String::from)
        .collect();
    names.sort();
    names
}

/// The bundled true-color theme `name`, or `None` when no such theme exists.
pub fn theme(name: &str) -> Option<&'static Theme> {
    THEMES.get(name).filter(|t| is_truecolor(t))
}

/// Whether `theme` names its colors in RGB rather than as ANSI palette indices.
fn is_truecolor(theme: &Theme) -> bool {
    theme.settings.background.is_none_or(|c| c.a == 255)
}

/// Color `code` as the language `info` names, painted in theme `theme_name`
/// over `bg`, one span list per source line.  `None` when the fence names
/// nothing syntect can parse.
///
/// Tabs are expanded here rather than by the caller, since a tab inside a
/// string literal has to be widened before the grammar sees the row to keep the
/// spans lined up with the text.
pub fn code_lines(
    info: &str,
    code: &str,
    theme_name: &str,
    bg: Color,
) -> Option<Vec<Vec<Span<'static>>>> {
    let token = info.split_whitespace().next()?;
    let syntax = SYNTAXES.find_syntax_by_token(token)?;
    let theme = theme(theme_name)?;
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut rows = Vec::new();
    for line in code.lines() {
        let expanded = line.replace('\t', "    ");
        // A grammar that trips on one row loses only that row's coloring; the
        // highlighter carries its own state forward, so the rest still parses.
        let row = match highlighter.highlight_line(&expanded, &SYNTAXES) {
            Ok(ranges) => ranges
                .into_iter()
                .map(|(style, text)| span(style, text, bg))
                .collect(),
            Err(_) => vec![Span::styled(expanded, Style::default().bg(bg))],
        };
        rows.push(row);
    }
    Some(rows)
}

/// One syntect-styled run as a ratatui span: the theme's foreground and its
/// emphasis over the palette's own code background.
fn span(style: syntect::highlighting::Style, text: &str, bg: Color) -> Span<'static> {
    let fg = style.foreground;
    let mut out = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)).bg(bg);
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    Span::styled(text.to_string(), out)
}

#[cfg(test)]
mod tests {
    use super::{code_lines, theme, theme_names};
    use ratatui::style::Color;

    const BG: Color = Color::Rgb(0, 0, 0);

    #[test]
    fn only_true_color_themes_are_offered() {
        let names = theme_names();
        assert!(
            names.len() >= 20,
            "expected bat's collection, got {names:?}"
        );
        // The ANSI-indexed themes cannot be rendered from RGB and are dropped.
        for dropped in ["ansi", "base16", "base16-256"] {
            assert!(
                !names.contains(&dropped.to_string()),
                "{dropped} slipped in"
            );
            assert!(theme(dropped).is_none());
        }
        assert!(names.iter().all(|n| theme(n).is_some()));
    }

    #[test]
    fn a_tagged_fence_colors_its_tokens_and_an_untagged_one_is_left_alone() {
        let rows =
            code_lines("rust", "let x = 1;\nfn f() {}", "TwoDark", BG).expect("rust is known");
        assert_eq!(rows.len(), 2);
        // `let` is a keyword and `x` an identifier, so the row cannot be one
        // run of a single color.
        assert!(rows[0].len() > 1, "row was not tokenized: {:?}", rows[0]);

        assert!(code_lines("", "let x = 1;", "TwoDark", BG).is_none());
        assert!(code_lines("no-such-language", "let x = 1;", "TwoDark", BG).is_none());
        assert!(code_lines("rust", "let x = 1;", "no such theme", BG).is_none());
    }

    /// The point of naming the syntax theme: the code has to recolor with it.
    #[test]
    fn a_different_theme_colors_the_same_code_differently() {
        let colors = |name: &str| {
            code_lines("rust", "let x = 1;", name, BG)
                .expect("rust is known")
                .swap_remove(0)
                .into_iter()
                .map(|span| span.style)
                .collect::<Vec<_>>()
        };
        assert_ne!(colors("TwoDark"), colors("Solarized (light)"));
    }
}
