//! The color math the theme derivation and the code highlighter share.
//!
//! Everything here works in whole sRGB channels and knows nothing of themes or
//! widgets, so the contrast rules live in one place rather than once per module
//! that needs to keep text readable.

use ratatui::style::Color;

/// An opaque color, as the math works in whole channels.
pub type Rgb = (u8, u8, u8);

/// The contrast ratio body text should clear against its background.
pub const TEXT_CONTRAST: f64 = 4.5;

/// The contrast ratio dimmed, secondary text should clear.
pub const DIM_CONTRAST: f64 = 3.0;

/// A channel triple as the ratatui color the widgets take.
pub const fn color((r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}

/// The channels behind a ratatui color, black for the named and indexed
/// variants the themes never produce.
pub const fn channels(c: Color) -> Rgb {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
}

/// Linearly blend `a` toward `b` by `t` in `[0, 1]`, per channel.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let lerp = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

/// Push `c` further from the middle — brighter on a dark theme, darker on a
/// light one — for the text that should stand out from the body.
pub fn shift(c: Rgb, dark: bool, t: f64) -> Rgb {
    mix(c, if dark { (255, 255, 255) } else { (0, 0, 0) }, t)
}

/// One sRGB channel's contribution to relative luminance, per the WCAG formula.
fn channel_luminance(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The WCAG relative luminance of a color, in `[0, 1]`.
pub fn luminance((r, g, b): Rgb) -> f64 {
    0.2126 * channel_luminance(r) + 0.7152 * channel_luminance(g) + 0.0722 * channel_luminance(b)
}

/// The WCAG contrast ratio between two colors, in `[1, 21]`.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Return `fg` unchanged when it already clears `min` contrast against `bg`,
/// otherwise blend it toward white or black — whichever the background is
/// furthest from — just far enough to reach the threshold, so a color stays as
/// close to its intended hue as legibility allows.
pub fn readable(fg: Rgb, bg: Rgb, min: f64) -> Rgb {
    if contrast(fg, bg) >= min {
        return fg;
    }
    let target = if luminance(bg) < 0.5 {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let mut t = 0.0;
    while t < 1.0 {
        t += 0.05;
        let candidate = mix(fg, target, t);
        if contrast(candidate, bg) >= min {
            return candidate;
        }
    }
    target
}

/// Nudge `fg` so it stays at least as legible over `bg` as it is over
/// `reference`, never pushing past the body-text threshold.
///
/// A syntax theme picks its colors to read against its own background; drawing
/// them over anything else — a code block's slightly lifted panel, a tint, a
/// selected row — can only cut their contrast.  This lifts a glyph back toward
/// what the theme intended.  A color the theme keeps dim on purpose stays dim:
/// the target never exceeds the contrast it already had over `reference`.
pub fn legible_over(fg: Rgb, bg: Rgb, reference: Rgb) -> Rgb {
    let target = contrast(fg, reference).min(TEXT_CONTRAST);
    readable(fg, bg, target)
}

/// The shorter way round the color wheel between two hues, in degrees.
pub fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
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
pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Rgb {
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
pub fn rgb_to_hsl((r, g, b): Rgb) -> (f64, f64, f64) {
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

#[cfg(test)]
mod tests {
    use super::{contrast, legible_over, readable};

    #[test]
    fn legible_over_lifts_a_glyph_a_lifted_panel_would_bury_and_leaves_a_clear_one() {
        // TwoDark's comment gray reads on the theme's own background, but the
        // code panel sits between them and cuts its contrast, so it is lifted
        // back toward what it had.  The body text already clears the panel and
        // stays put.
        let background = (40, 44, 52);
        let panel = (45, 49, 56);
        let comment = (92, 99, 112);
        let text = (171, 178, 191);

        let lifted = legible_over(comment, panel, background);
        assert!(
            contrast(lifted, panel) >= contrast(comment, background) - 0.01,
            "the comment was not lifted back to what the theme intended"
        );
        // Dim on purpose stays dim: it is not pushed to full body contrast.
        assert!(contrast(lifted, panel) < 4.5);
        assert_eq!(legible_over(text, panel, background), text);
    }

    #[test]
    fn readable_keeps_a_color_that_already_clears_and_lifts_one_that_does_not() {
        let bg = (0, 0, 0);
        let clear = (255, 255, 255);
        assert_eq!(readable(clear, bg, 4.5), clear);
        let muddy = (40, 40, 40);
        assert!(contrast(readable(muddy, bg, 4.5), bg) >= 4.5);
    }
}
