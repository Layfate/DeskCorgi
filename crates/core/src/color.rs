//! 8-bit RGBA colours with hex parsing and simple mixing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba { r, g, b, a: 255 }
}

pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    Rgba { r, g, b, a }
}

pub const WHITE: Rgba = rgb(255, 255, 255);
pub const BLACK: Rgba = rgb(0, 0, 0);

impl Rgba {
    /// Parses `#RRGGBB` or `#RRGGBBAA`.
    pub fn parse_hex(s: &str) -> Option<Rgba> {
        let hex = s.trim().strip_prefix('#')?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: if hex.len() == 8 { byte(6)? } else { 255 } })
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    /// Multiplies the alpha by `k` (0..1).
    pub fn fade(self, k: f32) -> Rgba {
        let a = (self.a as f32 * k.clamp(0.0, 1.0)).round() as u8;
        Rgba { a, ..self }
    }

    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba { r: m(self.r, other.r), g: m(self.g, other.g), b: m(self.b, other.b), a: m(self.a, other.a) }
    }

    /// Darkens towards black by `k` (0 = unchanged, 1 = black), keeping alpha.
    pub fn darken(self, k: f32) -> Rgba {
        let a = self.a;
        Rgba { a, ..self.mix(rgba(0, 0, 0, a), k) }
    }

    /// Lightens towards white by `k`, keeping alpha.
    pub fn lighten(self, k: f32) -> Rgba {
        let a = self.a;
        Rgba { a, ..self.mix(rgba(255, 255, 255, a), k) }
    }

    /// Relative luminance 0..1 (sRGB coefficients, no gamma).
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        for s in ["#000000", "#FFFFFF", "#E8913A", "#12345678"] {
            assert_eq!(Rgba::parse_hex(s).unwrap().to_hex(), s);
        }
        assert_eq!(Rgba::parse_hex("#abcdef"), Some(rgb(0xAB, 0xCD, 0xEF)));
    }

    #[test]
    fn rejects_bad_hex() {
        for s in ["", "pink", "#FFF", "#GGGGGG", "FFFFFF", "#1234567", "#FFFFFFFFF", "#ÿÿÿÿÿÿ"] {
            assert_eq!(Rgba::parse_hex(s), None, "{s}");
        }
    }
}
