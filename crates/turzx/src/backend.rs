//! The [`DisplayBackend`] trait: the single seam between the render engine and
//! anything that shows pixels (real panel, desktop preview window, PNG dump).

use crate::{Frame, Rect};

pub type Result<T> = std::result::Result<T, BackendError>;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("display backend I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("no TURZX device found (looked for VID {vid:#06x} PID {pid:#06x})")]
    DeviceNotFound { vid: u16, pid: u16 },

    #[error("TURZX protocol error: {0}")]
    Protocol(String),

    #[error("{0}")]
    Other(String),
}

/// A sink for rendered frames.
///
/// Implemented by the real serial panel driver, the windowed preview and the
/// PNG dump used in tests. The engine never depends on which one it holds.
pub trait DisplayBackend {
    /// Physical panel size in pixels.
    fn size(&self) -> (u16, u16);

    /// Push a frame.
    ///
    /// `dirty` lists the regions that changed since the previous `present`. A
    /// backend may repaint everything and ignore it, or use it to minimise bus
    /// traffic. An empty slice means "nothing changed since last time".
    fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<()>;

    /// Set panel brightness, `0..=255`. No-op for backends without a panel.
    fn set_brightness(&mut self, _level: u8) -> Result<()> {
        Ok(())
    }

    /// `true` once the user closed the preview window. Always `false` for a
    /// real panel, so the daemon's loop condition works everywhere.
    fn should_close(&self) -> bool {
        false
    }
}
