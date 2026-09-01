//! Thin CPU drawing helpers over a [`turzx::Frame`]: colours, alpha blending,
//! rectangles, bitmap text and image blits. No GPU, no dependencies — the panel
//! is tiny (320x480) so this is comfortably fast enough.

use turzx::Frame;

use crate::font;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const WHITE: Rgba = Rgba(255, 255, 255, 255);
    pub const BLACK: Rgba = Rgba(0, 0, 0, 255);

    /// Parse `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Option<Rgba> {
        let h = s.strip_prefix('#')?;
        let nib = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok();
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            3 => Some(Rgba(nib(0)? * 17, nib(1)? * 17, nib(2)? * 17, 255)),
            6 => Some(Rgba(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// Scale the alpha channel by `o` in `0..=1`.
    pub fn with_opacity(self, o: f32) -> Rgba {
        Rgba(
            self.0,
            self.1,
            self.2,
            (self.3 as f32 * o.clamp(0.0, 1.0)).round() as u8,
        )
    }

    fn scaled(self, factor: u8) -> Rgba {
        let s = |c: u8| ((c as u16 * factor as u16) / 255) as u8;
        Rgba(s(self.0), s(self.1), s(self.2), self.3)
    }
}

pub struct Canvas<'a> {
    frame: &'a mut Frame,
}

impl<'a> Canvas<'a> {
    pub fn new(frame: &'a mut Frame) -> Self {
        Self { frame }
    }

    pub fn width(&self) -> i32 {
        self.frame.width() as i32
    }

    pub fn height(&self) -> i32 {
        self.frame.height() as i32
    }

    /// Fill the whole frame with an opaque colour.
    pub fn clear(&mut self, c: Rgba) {
        for px in self.frame.as_rgba_mut().chunks_exact_mut(4) {
            px.copy_from_slice(&[c.0, c.1, c.2, 255]);
        }
    }

    /// Source-over blend a single pixel.
    pub fn blend(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.width() || y >= self.height() || c.3 == 0 {
            return;
        }
        let w = self.frame.width() as usize;
        let idx = (y as usize * w + x as usize) * 4;
        let buf = self.frame.as_rgba_mut();
        let a = c.3 as u32;
        let ia = 255 - a;
        let src = [c.0, c.1, c.2];
        for k in 0..3 {
            let out = (src[k] as u32 * a + buf[idx + k] as u32 * ia + 127) / 255;
            buf[idx + k] = out as u8;
        }
        buf[idx + 3] = 255;
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        if c.3 == 0 {
            return;
        }
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, c);
            }
        }
    }

    /// Draw a 1px outline just inside `(x, y, w, h)`.
    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgba) {
        self.fill_rect(x, y, w, 1, c);
        self.fill_rect(x, y + h - 1, w, 1, c);
        self.fill_rect(x, y, 1, h, c);
        self.fill_rect(x + w - 1, y, 1, h, c);
    }

    /// Draw uppercase bitmap text with the top-left of the first glyph at
    /// `(x, y)`. `scale` is an integer pixel multiplier.
    pub fn draw_text(&mut self, x: i32, y: i32, text: &str, scale: i32, c: Rgba) {
        let scale = scale.max(1);
        let mut cx = x;
        for ch in text.chars() {
            let rows = font::glyph(ch);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..font::GLYPH_W {
                    if bits & (0x10 >> col) != 0 {
                        self.fill_rect(cx + col * scale, y + row as i32 * scale, scale, scale, c);
                    }
                }
            }
            cx += (font::GLYPH_W + font::TRACKING) * scale;
        }
    }

    /// A framed progress bar: dim track, bright fill to `progress` in `0..=1`.
    pub fn progress_bar(&mut self, x: i32, y: i32, w: i32, h: i32, progress: f32, c: Rgba) {
        self.fill_rect(x, y, w, h, c.scaled(60));
        let fill = (w as f32 * progress.clamp(0.0, 1.0)).round() as i32;
        self.fill_rect(x, y, fill, h, c);
        self.stroke_rect(x, y, w, h, c.scaled(160));
    }

    /// Blit an RGBA8 image (`w * h * 4` bytes) with top-left at `(x, y)`,
    /// modulated by `opacity`.
    pub fn blit_rgba(&mut self, x: i32, y: i32, w: i32, h: i32, data: &[u8], opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        for row in 0..h {
            for col in 0..w {
                let s = (row * w + col) as usize * 4;
                let Some(px) = data.get(s..s + 4) else {
                    continue;
                };
                let a = (px[3] as f32 * opacity).round() as u8;
                self.blend(x + col, y + row, Rgba(px[0], px[1], px[2], a));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(Rgba::parse("#fff"), Some(Rgba(255, 255, 255, 255)));
        assert_eq!(Rgba::parse("#00ff9c"), Some(Rgba(0, 255, 156, 255)));
        assert_eq!(Rgba::parse("#00000080"), Some(Rgba(0, 0, 0, 128)));
        assert_eq!(Rgba::parse("nope"), None);
    }

    #[test]
    fn blend_is_opaque_over_opaque() {
        let mut f = Frame::new(2, 2);
        let mut c = Canvas::new(&mut f);
        c.clear(Rgba::BLACK);
        c.blend(0, 0, Rgba(255, 255, 255, 255));
        assert_eq!(&f.as_rgba()[0..4], &[255, 255, 255, 255]);
    }
}
