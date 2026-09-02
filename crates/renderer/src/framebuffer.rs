//! [`Canvas`] — a thin wrapper over a `tiny_skia::Pixmap` with the drawing
//! operations the scene engine needs: rects, paths, circles, transformed pixmap
//! blits, CPU-sampled glyph coverage, a separable blur for the glow, scanlines,
//! and the 5x7 bitmap fallback font.

use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Rect,
    Shader, Stroke, StrokeDash, Transform,
};
use turzx::Frame;

use crate::font;

#[derive(Debug, Clone, Copy, PartialEq)]
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

    /// Linear per-channel blend towards `other` by `p` in `0..=1`.
    pub fn lerp(self, other: Rgba, p: f32) -> Rgba {
        let p = p.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * p).round() as u8;
        Rgba(
            m(self.0, other.0),
            m(self.1, other.1),
            m(self.2, other.2),
            m(self.3, other.3),
        )
    }

    pub fn to_color(self) -> Color {
        Color::from_rgba8(self.0, self.1, self.2, self.3)
    }
}

pub struct Canvas {
    pm: Pixmap,
}

impl Canvas {
    pub fn new(width: u16, height: u16) -> Self {
        Canvas {
            pm: Pixmap::new(width.max(1) as u32, height.max(1) as u32).expect("pixmap alloc"),
        }
    }

    pub fn from_pixmap(pm: Pixmap) -> Self {
        Canvas { pm }
    }

    pub fn width(&self) -> f32 {
        self.pm.width() as f32
    }

    pub fn height(&self) -> f32 {
        self.pm.height() as f32
    }

    pub fn pixmap(&self) -> &Pixmap {
        &self.pm
    }

    pub fn clear(&mut self, c: Rgba) {
        self.pm.fill(c.to_color());
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Rect> {
        Rect::from_xywh(x, y, w.max(0.0), h.max(0.0))
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, paint: &Paint) {
        // Clip to the canvas: tiny-skia's hairline path can assert on
        // sub-pixel rects that straddle an edge. The shader is in canvas
        // space, so clipping the drawn area doesn't shift the gradient.
        let (cw, ch) = (self.width(), self.height());
        let x0 = x.max(0.0);
        let y0 = y.max(0.0);
        let x1 = (x + w).min(cw);
        let y1 = (y + h).min(ch);
        if x1 - x0 < 0.01 || y1 - y0 < 0.01 {
            return;
        }
        // Fill via a path, not `Pixmap::fill_rect`: the latter routes thin rects
        // through a hairline rasteriser that can assert on sub-pixel sizes.
        if let Some(r) = Rect::from_xywh(x0, y0, x1 - x0, y1 - y0) {
            let mut pb = PathBuilder::new();
            pb.push_rect(r);
            if let Some(path) = pb.finish() {
                self.pm
                    .fill_path(&path, paint, FillRule::Winding, Transform::identity(), None);
            }
        }
    }

    /// Fill the whole canvas with `paint` (used for a gradient background).
    pub fn fill_all(&mut self, paint: &Paint) {
        let (w, h) = (self.width(), self.height());
        self.fill_rect(0.0, 0.0, w, h, paint);
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, paint: &Paint) {
        let mut pb = PathBuilder::new();
        pb.push_rect(
            Self::rect(x, y, w, h).unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
        );
        if let Some(p) = pb.finish() {
            let stroke = Stroke {
                width,
                ..Stroke::default()
            };
            self.pm
                .stroke_path(&p, paint, &stroke, Transform::identity(), None);
        }
    }

    pub fn fill_path(&mut self, path: &tiny_skia::Path, paint: &Paint, ts: Transform) {
        self.pm.fill_path(path, paint, FillRule::Winding, ts, None);
    }

    /// Stroke `path`; `dash` (on, off) draws only a leading fraction (trace).
    pub fn stroke_path(
        &mut self,
        path: &tiny_skia::Path,
        paint: &Paint,
        width: f32,
        ts: Transform,
        dash: Option<(f32, f32)>,
    ) {
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            dash: dash
                .and_then(|(on, off)| StrokeDash::new(vec![on.max(0.01), off.max(0.01)], 0.0)),
            ..Stroke::default()
        };
        self.pm.stroke_path(path, paint, &stroke, ts, None);
    }

    /// A circle outline path (for the seal rings).
    pub fn circle_path(cx: f32, cy: f32, r: f32) -> Option<tiny_skia::Path> {
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r.max(0.1));
        pb.finish()
    }

    pub fn draw_pixmap(&mut self, src: &Pixmap, ts: Transform, opacity: f32, blend: BlendMode) {
        let paint = PixmapPaint {
            opacity: opacity.clamp(0.0, 1.0),
            blend_mode: blend,
            quality: tiny_skia::FilterQuality::Bilinear,
        };
        self.pm.draw_pixmap(0, 0, src.as_ref(), &paint, ts, None);
    }

    /// Blit a fontdue coverage bitmap at `(ox, oy)`, colouring each covered
    /// pixel with `color_at(px, py)` and multiplying alpha by `alpha`.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_coverage(
        &mut self,
        ox: i32,
        oy: i32,
        gw: usize,
        gh: usize,
        cov: &[u8],
        alpha: f32,
        color_at: impl Fn(f32, f32) -> Rgba,
    ) {
        if alpha <= 0.0 {
            return;
        }
        let (cw, ch) = (self.pm.width() as i32, self.pm.height() as i32);
        let data = self.pm.data_mut();
        for gy in 0..gh {
            let py = oy + gy as i32;
            if py < 0 || py >= ch {
                continue;
            }
            for gx in 0..gw {
                let px = ox + gx as i32;
                if px < 0 || px >= cw {
                    continue;
                }
                let c = cov[gy * gw + gx] as f32 / 255.0 * alpha.clamp(0.0, 1.0);
                if c <= 0.0 {
                    continue;
                }
                let col = color_at(px as f32 + 0.5, py as f32 + 0.5);
                let sa = c * (col.3 as f32 / 255.0);
                let idx = ((py * cw + px) * 4) as usize;
                // premultiplied source-over
                for k in 0..3 {
                    let src = [col.0, col.1, col.2][k] as f32 * sa;
                    let dst = data[idx + k] as f32;
                    data[idx + k] = (src + dst * (1.0 - sa)).round().clamp(0.0, 255.0) as u8;
                }
                let da = data[idx + 3] as f32 / 255.0;
                data[idx + 3] = ((sa + da * (1.0 - sa)) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// 1px horizontal lines every `period` rows (the "console" scanline look).
    pub fn scanlines(&mut self, period: i32, color: Rgba) {
        if period < 1 {
            return;
        }
        let paint = Paint {
            shader: Shader::SolidColor(color.to_color()),
            anti_alias: false,
            ..Paint::default()
        };
        let w = self.width();
        let mut y = 0;
        let h = self.pm.height() as i32;
        while y < h {
            self.fill_rect(0.0, y as f32, w, 1.0, &paint);
            y += period;
        }
    }

    /// In-place separable box blur (3 passes ~ Gaussian). Premultiplied RGBA.
    pub fn blur(&mut self, sigma: f32) {
        let radius = (sigma.max(0.0)).round() as i32;
        if radius < 1 {
            return;
        }
        let (w, h) = (self.pm.width() as usize, self.pm.height() as usize);
        let data = self.pm.data_mut();
        for _ in 0..3 {
            box_blur_pass(data, w, h, radius as usize, true);
            box_blur_pass(data, w, h, radius as usize, false);
        }
    }

    /// 5x7 bitmap fallback text (used when no TTF `font` is set on a layer).
    pub fn draw_bitmap_text(&mut self, x: f32, y: f32, text: &str, scale: f32, c: Rgba) {
        let s = scale.max(1.0);
        let paint = Paint {
            shader: Shader::SolidColor(c.to_color()),
            anti_alias: false,
            ..Paint::default()
        };
        let mut cx = x;
        for ch in text.chars() {
            let rows = font::glyph(ch);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..font::GLYPH_W {
                    if bits & (0x10 >> col) != 0 {
                        self.fill_rect(cx + col as f32 * s, y + row as f32 * s, s, s, &paint);
                    }
                }
            }
            cx += (font::GLYPH_W + font::TRACKING) as f32 * s;
        }
    }

    /// Unpremultiply into a straight-alpha [`Frame`].
    pub fn into_frame(self) -> Frame {
        let (w, h) = (self.pm.width() as u16, self.pm.height() as u16);
        let mut frame = Frame::new(w, h);
        let src = self.pm.data();
        let dst = frame.as_rgba_mut();
        for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            let a = s[3];
            if a == 0 {
                d.copy_from_slice(&[0, 0, 0, 0]);
            } else if a == 255 {
                d.copy_from_slice(s);
            } else {
                let inv = 255.0 / a as f32;
                d[0] = (s[0] as f32 * inv).round().min(255.0) as u8;
                d[1] = (s[1] as f32 * inv).round().min(255.0) as u8;
                d[2] = (s[2] as f32 * inv).round().min(255.0) as u8;
                d[3] = a;
            }
        }
        frame
    }
}

fn box_blur_pass(data: &mut [u8], w: usize, h: usize, radius: usize, horizontal: bool) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let stride = if horizontal { 4 } else { w * 4 };
    let window = (radius * 2 + 1) as f32;
    let mut scratch = vec![0u8; len * 4];
    for line in 0..lines {
        let base = if horizontal { line * w * 4 } else { line * 4 };
        for k in 0..4 {
            let at = |i: usize| data[base + i * stride + k] as f32;
            let mut sum: f32 = 0.0;
            for i in 0..=radius.min(len - 1) {
                sum += at(i);
            }
            // extend the left edge
            sum += at(0) * radius as f32;
            for i in 0..len {
                scratch[i * 4 + k] = (sum / window).round().clamp(0.0, 255.0) as u8;
                let add = at((i + radius + 1).min(len - 1));
                let sub = if i >= radius { at(i - radius) } else { at(0) };
                sum += add - sub;
            }
        }
        for i in 0..len {
            for k in 0..4 {
                data[base + i * stride + k] = scratch[i * 4 + k];
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
    fn fills_and_unpremultiplies() {
        let mut c = Canvas::new(4, 4);
        c.clear(Rgba::BLACK);
        let paint = Paint {
            shader: Shader::SolidColor(Rgba(255, 255, 255, 255).to_color()),
            ..Paint::default()
        };
        c.fill_rect(0.0, 0.0, 2.0, 2.0, &paint);
        let f = c.into_frame();
        assert_eq!(&f.as_rgba()[0..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn lerp_midpoint() {
        assert_eq!(
            Rgba(0, 0, 0, 255).lerp(Rgba(255, 255, 255, 255), 0.5).0,
            128
        );
    }
}
