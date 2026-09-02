//! Minimal SVG path (`d`) parser — the subset the Horus eye glyph uses:
//! `M m L l H h V v C c Z z`. Produces a `tiny_skia::Path`.

use tiny_skia::{Path, PathBuilder, PathSegment, Transform};

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Lexer {
            b: s.as_bytes(),
            i: 0,
        }
    }

    fn skip_sep(&mut self) {
        while matches!(
            self.b.get(self.i),
            Some(b' ' | b',' | b'\t' | b'\n' | b'\r')
        ) {
            self.i += 1;
        }
    }

    fn eof(&mut self) -> bool {
        self.skip_sep();
        self.i >= self.b.len()
    }

    fn peek_cmd(&mut self) -> Option<u8> {
        self.skip_sep();
        match self.b.get(self.i) {
            Some(c) if c.is_ascii_alphabetic() => Some(*c),
            _ => None,
        }
    }

    fn take_cmd(&mut self) -> Option<u8> {
        let c = self.peek_cmd()?;
        self.i += 1;
        Some(c)
    }

    fn num(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let mut seen_digit = false;
        let mut seen_dot = false;
        while let Some(&c) = self.b.get(self.i) {
            match c {
                b'0'..=b'9' => {
                    seen_digit = true;
                    self.i += 1;
                }
                b'.' if !seen_dot => {
                    seen_dot = true;
                    self.i += 1;
                }
                b'e' | b'E' => {
                    self.i += 1;
                    if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                        self.i += 1;
                    }
                }
                _ => break,
            }
        }
        if !seen_digit {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

/// Parse `d`. Returns `None` on an unsupported command or malformed numbers.
pub fn parse_path(d: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let mut lx = Lexer::new(d);
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut cmd = 0u8;
    let mut any = false;

    while !lx.eof() {
        if let Some(c) = lx.peek_cmd() {
            lx.take_cmd();
            cmd = c;
        } else if cmd == b'M' {
            cmd = b'L';
        } else if cmd == b'm' {
            cmd = b'l';
        }

        match cmd {
            b'M' | b'm' => {
                let (mut x, mut y) = (lx.num()?, lx.num()?);
                if cmd == b'm' {
                    x += cx;
                    y += cy;
                }
                cx = x;
                cy = y;
                sx = x;
                sy = y;
                pb.move_to(x, y);
                any = true;
            }
            b'L' | b'l' => {
                let (mut x, mut y) = (lx.num()?, lx.num()?);
                if cmd == b'l' {
                    x += cx;
                    y += cy;
                }
                cx = x;
                cy = y;
                pb.line_to(x, y);
            }
            b'H' | b'h' => {
                let mut x = lx.num()?;
                if cmd == b'h' {
                    x += cx;
                }
                cx = x;
                pb.line_to(x, cy);
            }
            b'V' | b'v' => {
                let mut y = lx.num()?;
                if cmd == b'v' {
                    y += cy;
                }
                cy = y;
                pb.line_to(cx, y);
            }
            b'C' | b'c' => {
                let mut p = [0.0f32; 6];
                for slot in &mut p {
                    *slot = lx.num()?;
                }
                if cmd == b'c' {
                    for k in 0..3 {
                        p[k * 2] += cx;
                        p[k * 2 + 1] += cy;
                    }
                }
                pb.cubic_to(p[0], p[1], p[2], p[3], p[4], p[5]);
                cx = p[4];
                cy = p[5];
            }
            b'Z' | b'z' => {
                pb.close();
                cx = sx;
                cy = sy;
            }
            _ => return None,
        }
    }

    if !any {
        return None;
    }
    pb.finish()
}

/// Approximate outline length (cubics flattened to `STEPS` chords). Needed to
/// turn a `0..1` trace fraction into a stroke dash length.
pub fn path_length(path: &Path) -> f32 {
    const STEPS: usize = 24;
    let mut total = 0.0f32;
    let mut cur = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                cur = (p.x, p.y);
                start = cur;
            }
            PathSegment::LineTo(p) => {
                total += dist(cur, (p.x, p.y));
                cur = (p.x, p.y);
            }
            PathSegment::QuadTo(c, p) => {
                let mut prev = cur;
                for k in 1..=STEPS {
                    let t = k as f32 / STEPS as f32;
                    let pt = quad(cur, (c.x, c.y), (p.x, p.y), t);
                    total += dist(prev, pt);
                    prev = pt;
                }
                cur = (p.x, p.y);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let mut prev = cur;
                for k in 1..=STEPS {
                    let t = k as f32 / STEPS as f32;
                    let pt = cubic(cur, (c1.x, c1.y), (c2.x, c2.y), (p.x, p.y), t);
                    total += dist(prev, pt);
                    prev = pt;
                }
                cur = (p.x, p.y);
            }
            PathSegment::Close => {
                total += dist(cur, start);
                cur = start;
            }
        }
    }
    total
}

/// Transform matching the design's `EyeMark`:
/// `translate(cx,cy) rotate(deg) scale(k) translate(-cxp,-cyp)`.
pub fn glyph_transform(cx: f32, cy: f32, k: f32, deg: f32, cxp: f32, cyp: f32) -> Transform {
    Transform::from_translate(cx, cy)
        .pre_rotate(deg)
        .pre_scale(k, k)
        .pre_translate(-cxp, -cyp)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn quad(a: (f32, f32), c: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    (
        u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
        u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
    )
}

fn cubic(a: (f32, f32), c1: (f32, f32), c2: (f32, f32), b: (f32, f32), t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    (
        w[0] * a.0 + w[1] * c1.0 + w[2] * c2.0 + w[3] * b.0,
        w[0] * a.1 + w[1] * c1.1 + w[2] * c2.1 + w[3] * b.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_horus_eye() {
        let p = parse_path(crate::HORUS_EYE_PATH).expect("eye path parses");
        let b = p.bounds();
        // The glyph lives roughly within its 0..666 viewBox.
        assert!(b.left() > -5.0 && b.right() < 672.0);
        assert!(b.top() > -5.0 && b.bottom() < 672.0);
        assert!(path_length(&p) > 1000.0);
    }

    #[test]
    fn rejects_unsupported_command() {
        assert!(parse_path("M0 0 A 1 1 0 0 1 10 10").is_none());
    }

    #[test]
    fn relative_and_implicit_lineto() {
        let p = parse_path("M10 10 20 20 l5 0 z").expect("parses");
        assert!(p.bounds().right() >= 25.0);
    }
}
