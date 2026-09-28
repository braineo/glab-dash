use palette::color_difference::Wcag21RelativeContrast;
use palette::convert::FromColorUnclamped;
use palette::{Clamp, IsWithinBounds, LinSrgb, Mix, Oklch, Srgb};
use ratatui::style::Color;

/// Floats all the way to `color`, which quantizes once: rounding at every
/// blend compounds through the accent derivation.
pub type Rgb = Srgb<f64>;

pub const WHITE: Rgb = Srgb::new(1.0, 1.0, 1.0);
pub const BLACK: Rgb = Srgb::new(0.0, 0.0, 0.0);

/// WCAG AA: 4.5 for body text, 3.0 for large or secondary text.
pub const TEXT_CONTRAST: f64 = 4.5;
pub const DIM_CONTRAST: f64 = 3.0;

pub fn color(c: Rgb) -> Color {
    let c = c.into_format::<u8>();
    Color::Rgb(c.red, c.green, c.blue)
}

pub fn readable(fg: Rgb, bg: Rgb, min: f64) -> Rgb {
    if fg.relative_contrast(bg) >= min {
        return fg;
    }
    let target = if bg.relative_luminance().luma < 0.5 {
        WHITE
    } else {
        BLACK
    };
    let mut t = 0.0;
    while t < 1.0 {
        t += 0.05;
        let candidate = fg.mix(target, t);
        if candidate.relative_contrast(bg) >= min {
            return candidate;
        }
    }
    target
}

pub fn legible_over(fg: Rgb, bg: Rgb, reference: Rgb) -> Rgb {
    let target = fg.relative_contrast(reference).min(TEXT_CONTRAST);
    readable(fg, bg, target)
}

pub fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    d.min(360.0 - d)
}

/// Most of the Oklch cylinder is outside sRGB, and clamping the channels
/// shifts the hue, so chroma is walked down until the color fits.
///
/// ponytail: `Okhsl` measures saturation against the gamut boundary and would
/// answer in one step, but palette 0.7 returns a saturation of 0 for a chroma
/// past that boundary, which paints half the hues gray.
pub fn fit_gamut(mut c: Oklch<f64>) -> Rgb {
    loop {
        // `from_color` would clamp into the cube, leaving nothing to detect.
        let lin = LinSrgb::<f64>::from_color_unclamped(c);
        if lin.is_within_bounds() || c.chroma <= 0.0 {
            return Srgb::from_linear(lin.clamp());
        }
        c.chroma = (c.chroma - 0.005).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{Rgb, fit_gamut, legible_over, readable};
    use palette::color_difference::Wcag21RelativeContrast;
    use palette::{FromColor, Oklch, Srgb};

    fn rgb(r: u8, g: u8, b: u8) -> Rgb {
        Srgb::new(r, g, b).into_format()
    }

    #[test]
    fn legible_over_lifts_a_glyph_a_lifted_panel_would_bury_and_leaves_a_clear_one() {
        let background = rgb(40, 44, 52);
        let panel = rgb(45, 49, 56);
        let comment = rgb(92, 99, 112);
        let text = rgb(171, 178, 191);

        let lifted = legible_over(comment, panel, background);
        assert!(
            lifted.relative_contrast(panel) >= comment.relative_contrast(background) - 0.01,
            "the comment was not lifted back to what the theme intended"
        );
        assert!(lifted.relative_contrast(panel) < 4.5);
        assert_eq!(legible_over(text, panel, background), text);
    }

    #[test]
    fn readable_keeps_a_color_that_already_clears_and_lifts_one_that_does_not() {
        let bg = rgb(0, 0, 0);
        let clear = rgb(255, 255, 255);
        assert_eq!(readable(clear, bg, 4.5), clear);
        let muddy = rgb(40, 40, 40);
        assert!(readable(muddy, bg, 4.5).relative_contrast(bg) >= 4.5);
    }

    #[test]
    fn an_out_of_gamut_chroma_keeps_its_lightness_and_stays_colored() {
        for hue in (0..360).step_by(15).map(f64::from) {
            let fitted = Oklch::from_color(fit_gamut(Oklch::new(0.65, 0.4, hue)));
            // Loose: walking chroma back moves the lightness a little too.
            assert!(
                (fitted.l - 0.65).abs() < 0.07,
                "lightness drifted at {hue}: {}",
                fitted.l
            );
            assert!(
                fitted.chroma > 0.05,
                "chroma collapsed to gray at {hue}: {}",
                fitted.chroma
            );
        }
    }
}
