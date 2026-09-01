//! The CPU-side frame buffer shared between the renderer and every backend.

/// An axis-aligned rectangle in pixel coordinates. Origin is top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    /// Smallest rectangle covering both `self` and `other`.
    pub fn union(self, other: Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    pub fn area(self) -> u32 {
        self.w as u32 * self.h as u32
    }
}

/// A CPU-side RGBA8 frame: row-major, top-left origin, 4 bytes per pixel.
///
/// Rendering happens in RGBA8 for convenience; conversion to the panel's RGB565
/// happens once, at the backend boundary, via [`Frame::to_rgb565`].
#[derive(Debug, Clone)]
pub struct Frame {
    width: u16,
    height: u16,
    pixels: Vec<u8>,
}

impl Frame {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn as_rgba(&self) -> &[u8] {
        &self.pixels
    }

    pub fn as_rgba_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Rotate 90° clockwise into a new frame with width/height swapped.
    ///
    /// Used by the serial backend to map a landscape-authored frame (480x320)
    /// onto the physical 320x480 pixel array. The rotation *direction* still
    /// needs confirming against hardware (see `docs/PROTOCOL.md`).
    pub fn rotated_cw(&self) -> Frame {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut out = Frame::new(self.height, self.width);
        let dst = out.as_rgba_mut();
        for y in 0..h {
            for x in 0..w {
                let s = (y * w + x) * 4;
                // (x, y) in a w-wide buffer -> (h-1-y, x) in an h-wide buffer
                let d = (x * h + (h - 1 - y)) * 4;
                dst[d..d + 4].copy_from_slice(&self.pixels[s..s + 4]);
            }
        }
        out
    }

    /// Pack into RGB565, one `u16` per pixel, in reading order.
    pub fn to_rgb565(&self) -> Vec<u16> {
        self.pixels
            .chunks_exact(4)
            .map(|p| {
                let r = (p[0] as u16 >> 3) & 0x1f;
                let g = (p[1] as u16 >> 2) & 0x3f;
                let b = (p[2] as u16 >> 3) & 0x1f;
                (r << 11) | (g << 5) | b
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_cw_swaps_dims_and_moves_corner() {
        let mut f = Frame::new(2, 3); // 2 wide, 3 tall
        f.as_rgba_mut()[0..4].copy_from_slice(&[9, 9, 9, 9]); // top-left (0,0)
        let r = f.rotated_cw();
        assert_eq!((r.width(), r.height()), (3, 2));
        // top-left of a w-wide buffer lands at top-right of the rotated one
        let top_right = ((r.width() as usize - 1) * 4)..((r.width() as usize) * 4);
        assert_eq!(&r.as_rgba()[top_right], &[9, 9, 9, 9]);
    }
}
