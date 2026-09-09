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

/// One sRGB channel undone back to light, the form every color model below
/// does its arithmetic in.
fn linearize(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// One linear channel encoded back to an sRGB byte, clamped to the display.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn encode(c: f64) -> u8 {
    let c = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The WCAG relative luminance of a color, in `[0, 1]`.
pub fn luminance((r, g, b): Rgb) -> f64 {
    0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
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

/// A color's hue in degrees, saturation and lightness.  Kept for the accent
/// harvest, which sorts a theme's syntax colors by the hue names everyone
/// spells in HSL — Oklch's red sits at 29 degrees, not zero.
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

// ── Oklch ──
//
// HSL lies about lightness: a yellow and a blue at the same `l` are nowhere
// near equally bright, and blending two hues through sRGB dips through a
// washed-out middle.  Oklab is perceptually uniform, so its polar form —
// lightness, chroma, hue — is the space to build a set of colors in that has
// to look like a set: hold L and C, step the hue, and every color comes out a
// sibling of the others.  Everything is Björn Ottosson's Oklab.

/// A color as perceptual (lightness in `[0, 1]`, chroma, hue in degrees).
pub type Oklch = (f64, f64, f64);

/// A color's perceptual lightness, chroma and hue.
#[allow(clippy::many_single_char_names)]
pub fn rgb_to_oklch((r, g, b): Rgb) -> Oklch {
    let (r, g, b) = (linearize(r), linearize(g), linearize(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();

    let lightness = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
    let a = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let b = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    (
        lightness,
        a.hypot(b),
        b.atan2(a).to_degrees().rem_euclid(360.0),
    )
}

/// The linear-light channels for an Oklab color, which may fall outside the
/// `[0, 1]` cube a display can actually show.
#[allow(clippy::many_single_char_names)]
fn oklab_to_linear(lightness: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    (
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    )
}

/// The sRGB color at perceptual `(lightness, chroma, hue)`, desaturated only
/// as far as the display forces.
///
/// Most of the Oklch cylinder is outside sRGB — a fully chromatic yellow at
/// mid lightness simply does not exist on a monitor — and letting the channels
/// clamp on their own shifts the hue.  Walking the chroma down instead keeps
/// the hue and the lightness, which are the two the palette depends on, and
/// gives up only the saturation that was never displayable.
pub fn oklch_to_rgb((lightness, chroma, hue): Oklch) -> Rgb {
    let (sin, cos) = hue.to_radians().sin_cos();
    let mut c = chroma;
    loop {
        let (r, g, b) = oklab_to_linear(lightness, c * cos, c * sin);
        let inside = [r, g, b].iter().all(|v| (-0.001..=1.001).contains(v));
        if inside || c <= 0.0 {
            return (encode(r), encode(g), encode(b));
        }
        c = (c - 0.005).max(0.0);
    }
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
