use std::sync::LazyLock;

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::SyntaxSet;
use two_face::theme::LazyThemeSet;

use palette::Srgb;

use crate::ui::color::{self, Rgb, legible_over};

pub fn opaque(c: syntect::highlighting::Color) -> Rgb {
    Srgb::new(c.r, c.g, c.b).into_format()
}

/// bat's grammars, which reach languages syntect's defaults omit.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_no_newlines);

/// Each decompressed on first use, so naming one does not pay for the set.
static THEMES: LazyLock<LazyThemeSet> =
    LazyLock::new(|| LazyThemeSet::from(two_face::theme::extra()));

pub fn theme_names() -> Vec<String> {
    let mut names: Vec<String> = THEMES
        .theme_names()
        .filter(|name| THEMES.get(name).is_some_and(is_truecolor))
        .map(String::from)
        .collect();
    names.sort();
    names
}

pub fn theme(name: &str) -> Option<&'static Theme> {
    THEMES.get(name).filter(|t| is_truecolor(t))
}

/// two-face carries a few themes whose settings hold ANSI palette indices in
/// place of RGB, marked by a non-opaque background; the interface is derived
/// from RGB, so those are left out.
fn is_truecolor(theme: &Theme) -> bool {
    theme.settings.background.is_none_or(|c| c.a == 255)
}

/// One span list per source line; `None` when the fence names nothing syntect
/// can parse.
///
/// Tabs are expanded before the grammar sees the row, so a tab inside a string
/// literal keeps the spans lined up with the text.
pub fn code_lines(
    info: &str,
    code: &str,
    theme_name: &str,
    bg: Rgb,
) -> Option<Vec<Vec<Span<'static>>>> {
    let token = info.split_whitespace().next()?;
    let syntax = SYNTAXES.find_syntax_by_token(token)?;
    let theme = theme(theme_name)?;
    let mut highlighter = HighlightLines::new(syntax, theme);
    let reference = theme.settings.background.map_or(bg, opaque);

    let mut rows = Vec::new();
    for line in code.lines() {
        let expanded = line.replace('\t', "    ");
        // The highlighter carries its state forward, so a row that trips the
        // grammar loses only its own coloring.
        let row = match highlighter.highlight_line(&expanded, &SYNTAXES) {
            Ok(ranges) => ranges
                .into_iter()
                .map(|(style, text)| span(style, text, bg, reference))
                .collect(),
            Err(_) => vec![Span::styled(
                expanded,
                Style::default().bg(color::color(bg)),
            )],
        };
        rows.push(row);
    }
    Some(rows)
}

fn span(style: syntect::highlighting::Style, text: &str, bg: Rgb, reference: Rgb) -> Span<'static> {
    let fg = legible_over(opaque(style.foreground), bg, reference);
    let mut out = Style::default().fg(color::color(fg)).bg(color::color(bg));
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
    use crate::ui::color::Rgb;
    use palette::Srgb;

    const BG: Rgb = Srgb::new(0.0, 0.0, 0.0);

    #[test]
    fn only_true_color_themes_are_offered() {
        let names = theme_names();
        assert!(
            names.len() >= 20,
            "expected bat's collection, got {names:?}"
        );
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
        assert!(rows[0].len() > 1, "row was not tokenized: {:?}", rows[0]);

        assert!(code_lines("", "let x = 1;", "TwoDark", BG).is_none());
        assert!(code_lines("no-such-language", "let x = 1;", "TwoDark", BG).is_none());
        assert!(code_lines("rust", "let x = 1;", "no such theme", BG).is_none());
    }

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
