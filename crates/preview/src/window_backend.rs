//! A desktop window that mirrors the panel, for iterating on animations without
//! hardware. macOS / Linux / Windows via `minifb`.

use minifb::{Key, Window, WindowOptions};
use turzx::{BackendError, DisplayBackend, Frame, Rect, Result};

pub struct WindowBackend {
    window: Window,
    size: (u16, u16),
    scale: usize,
    buf: Vec<u32>,
}

impl WindowBackend {
    pub fn new(title: &str, size: (u16, u16), scale: usize) -> Result<Self> {
        let scale = scale.max(1);
        let (w, h) = (size.0 as usize * scale, size.1 as usize * scale);
        let window = Window::new(title, w, h, WindowOptions::default())
            .map_err(|e| BackendError::Other(e.to_string()))?;
        Ok(Self {
            window,
            size,
            scale,
            buf: vec![0; w * h],
        })
    }
}

impl DisplayBackend for WindowBackend {
    fn size(&self) -> (u16, u16) {
        self.size
    }

    fn present(&mut self, frame: &Frame, _dirty: &[Rect]) -> Result<()> {
        let (pw, ph) = (self.size.0 as usize, self.size.1 as usize);
        let sw = pw * self.scale;
        let src = frame.as_rgba();
        for y in 0..ph {
            for x in 0..pw {
                let s = (y * pw + x) * 4;
                let px = 0xff00_0000
                    | (src[s] as u32) << 16
                    | (src[s + 1] as u32) << 8
                    | (src[s + 2] as u32);
                for dy in 0..self.scale {
                    let row = (y * self.scale + dy) * sw + x * self.scale;
                    self.buf[row..row + self.scale].fill(px);
                }
            }
        }
        self.window
            .update_with_buffer(&self.buf, sw, ph * self.scale)
            .map_err(|e| BackendError::Other(e.to_string()))
    }

    fn should_close(&self) -> bool {
        !self.window.is_open() || self.window.is_key_down(Key::Escape)
    }
}
