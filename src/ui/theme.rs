//! Colour, type, spacing and motion. Everything the app paints comes from here, so swapping the
//! palette is one line.

use rune::{Color, Family, TextStyle, oklch, oklcha};

use crate::fonts;

/// A colour held as OKLCH numbers, so it can be measured as well as painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ok(pub f32, pub f32, pub f32);

impl Ok {
    pub fn color(self) -> Color {
        oklch(self.0, self.1, self.2)
    }

    pub fn alpha(self, a: f32) -> Color {
        oklcha(self.0, self.1, self.2, a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub page: Ok,
    pub panel: Ok,
    pub ink: Ok,
    /// Counts, footers and other quiet text.
    pub faint: Ok,
    /// Selection, active tabs, the hover tint.
    pub accent: Ok,
    /// Paragraph marks, and the dot that says a target has left its original.
    pub marker: Ok,
    /// Opacity of ink on faded text.
    pub ghost: f32,
    /// Opacity of ink on the page outside a target, while its panel is open.
    pub dimmed: f32,
}

/// Midnight blue with lamp-warm ink. The closest to a late-night desk.
pub const LANTERN: Theme = Theme {
    name: "Lantern",
    page: Ok(0.185, 0.028, 262.0),
    panel: Ok(0.225, 0.03, 262.0),
    ink: Ok(0.87, 0.045, 78.0),
    faint: Ok(0.62, 0.03, 262.0),
    accent: Ok(0.73, 0.1, 205.0),
    marker: Ok(0.74, 0.15, 48.0),
    ghost: 0.14,
    dimmed: 0.34,
};

/// Warm charcoal with a green-gold accent, like graphite on a dark board.
pub const GRAPHITE: Theme = Theme {
    name: "Graphite",
    page: Ok(0.19, 0.01, 70.0),
    panel: Ok(0.235, 0.012, 70.0),
    ink: Ok(0.88, 0.028, 88.0),
    faint: Ok(0.62, 0.016, 75.0),
    accent: Ok(0.78, 0.13, 115.0),
    marker: Ok(0.72, 0.16, 35.0),
    ghost: 0.14,
    dimmed: 0.34,
};

/// Deep sea green with cream ink and a soft lilac accent.
pub const KELP: Theme = Theme {
    name: "Kelp",
    page: Ok(0.19, 0.03, 185.0),
    panel: Ok(0.235, 0.032, 185.0),
    ink: Ok(0.89, 0.035, 100.0),
    faint: Ok(0.63, 0.03, 180.0),
    accent: Ok(0.76, 0.09, 300.0),
    marker: Ok(0.75, 0.14, 62.0),
    ghost: 0.14,
    dimmed: 0.34,
};

pub const THEMES: [Theme; 3] = [LANTERN, GRAPHITE, KELP];

/// The palette in use.
pub const THEME: Theme = LANTERN;

pub const COLUMN: f32 = 640.0;
pub const GUTTER: f32 = 40.0;
pub const SWAP_FADE: f32 = 0.16;

pub fn body() -> TextStyle {
    TextStyle::new(16.0, 400.0).lh(28.0).family(Family::Mono)
}

pub fn small() -> TextStyle {
    TextStyle::new(12.0, 400.0).lh(16.0).family(Family::Mono)
}

pub fn hand() -> TextStyle {
    TextStyle::new(24.0, 400.0).lh(30.0).family(Family::Named(fonts::HAND))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear(c: Ok) -> [f64; 3] {
        let (l, a, b) = (c.0 as f64, c.1 as f64 * (c.2 as f64).to_radians().cos(), c.1 as f64 * (c.2 as f64).to_radians().sin());
        let (m, s, t) = (l + 0.3963377774 * a + 0.2158037573 * b, l - 0.1055613458 * a - 0.0638541728 * b, l - 0.0894841775 * a - 1.2914855480 * b);
        let (m, s, t) = (m.powi(3), s.powi(3), t.powi(3));
        [4.0767416621 * m - 3.3077115913 * s + 0.2309699292 * t, -1.2684380046 * m + 2.6097574011 * s - 0.3413193965 * t, -0.0041960863 * m - 0.7034186147 * s + 1.7076147010 * t]
            .map(|v| v.clamp(0.0, 1.0))
    }

    fn encode(v: f64) -> f64 {
        if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
    }

    fn decode(v: f64) -> f64 {
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    fn luminance(rgb: [f64; 3]) -> f64 {
        0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
    }

    /// `fg` at opacity `a` over `bg`, blended in sRGB as the renderer does for text.
    fn over(fg: Ok, a: f64, bg: Ok) -> [f64; 3] {
        let (f, b) = (linear(fg).map(encode), linear(bg).map(encode));
        [0, 1, 2].map(|i| decode(f[i] * a + b[i] * (1.0 - a)))
    }

    fn contrast(a: [f64; 3], b: [f64; 3]) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn body_text_reads_comfortably_at_length() {
        for t in THEMES {
            let c = contrast(linear(t.ink), linear(t.page));
            assert!(c >= 9.0, "{}: ink contrast {c:.1}", t.name);
        }
    }

    #[test]
    fn quiet_text_stays_legible() {
        for t in THEMES {
            let c = contrast(linear(t.faint), linear(t.page));
            assert!(c >= 4.5, "{}: faint contrast {c:.1}", t.name);
        }
    }

    #[test]
    fn marks_stand_out_from_the_page() {
        for t in THEMES {
            for (name, c) in [("accent", t.accent), ("marker", t.marker)] {
                let c2 = contrast(linear(c), linear(t.page));
                assert!(c2 >= 4.5, "{}: {name} contrast {c2:.1}", t.name);
            }
        }
    }

    #[test]
    fn faded_text_is_faint_but_still_there() {
        for t in THEMES {
            let c = contrast(over(t.ink, t.ghost as f64, t.page), linear(t.page));
            assert!((1.25..2.2).contains(&c), "{}: ghost contrast {c:.2}", t.name);
        }
    }

    #[test]
    fn dimmed_text_sits_behind_the_target() {
        for t in THEMES {
            let dim = contrast(over(t.ink, t.dimmed as f64, t.page), linear(t.page));
            let full = contrast(linear(t.ink), linear(t.page));
            assert!(dim >= 2.0 && dim < full / 2.0, "{}: dimmed contrast {dim:.2} of {full:.2}", t.name);
        }
    }
}
