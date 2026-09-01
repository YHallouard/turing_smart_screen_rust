//! Writes each presented frame to `dir/frame_00000.png`. Useful headless and in
//! tests; `with_stride` thins the output when you only want a filmstrip.

use std::path::PathBuf;

use turzx::{BackendError, DisplayBackend, Frame, Rect, Result};

pub struct PngBackend {
    dir: PathBuf,
    size: (u16, u16),
    count: u32,
    stride: u32,
}

impl PngBackend {
    pub fn new(dir: impl Into<PathBuf>, size: (u16, u16)) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            size,
            count: 0,
            stride: 1,
        })
    }

    /// Only write every `n`-th presented frame.
    pub fn with_stride(mut self, n: u32) -> Self {
        self.stride = n.max(1);
        self
    }

    /// Number of frames received so far (written or skipped).
    pub fn frames_seen(&self) -> u32 {
        self.count
    }
}

impl DisplayBackend for PngBackend {
    fn size(&self) -> (u16, u16) {
        self.size
    }

    fn present(&mut self, frame: &Frame, _dirty: &[Rect]) -> Result<()> {
        let i = self.count;
        self.count += 1;
        if i % self.stride != 0 {
            return Ok(());
        }
        let path = self.dir.join(format!("frame_{i:05}.png"));
        image::save_buffer(
            &path,
            frame.as_rgba(),
            frame.width() as u32,
            frame.height() as u32,
            image::ColorType::Rgba8,
        )
        .map_err(|e| BackendError::Other(format!("{}: {e}", path.display())))?;
        Ok(())
    }
}
