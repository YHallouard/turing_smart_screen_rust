//! TURZX 3.5" (320x480) secondary-display support.
//!
//! This crate is deliberately small: a backend-agnostic [`Frame`] type, a
//! [`DisplayBackend`] trait, dirty-region tracking, and — behind the `serial`
//! feature — a driver for the USB CDC-ACM panel.
//!
//! The renderer, preview windows and the daemon all talk to the panel only
//! through [`DisplayBackend`], so swapping in the 5" panel or a different bus
//! later means adding one impl, nothing else.

mod backend;
mod dirty;
mod frame;

pub use backend::{BackendError, DisplayBackend, Result};
pub use dirty::DirtyTracker;
pub use frame::{Frame, Rect};

/// Native (physical) resolution of the 3.5" panel. The pixel array is always
/// addressed as 320 wide x 480 tall on the wire, whatever way the panel is
/// mounted; [`Orientation`] only changes the *logical* canvas scenes use.
pub const PANEL_WIDTH: u16 = 320;
/// Native (physical) resolution of the 3.5" panel.
pub const PANEL_HEIGHT: u16 = 480;

/// How the panel is physically mounted. There is no orientation sensor on this
/// hardware, so this is a static choice (config value, read at startup), not a
/// live signal — the daemon selects the matching scene variant and logical size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// 320x480, sent to the panel 1:1.
    #[default]
    Portrait,
    /// 480x320 logical canvas; the serial backend maps it onto the physical
    /// 320x480 array (or, once confirmed, a panel-side orientation command).
    Landscape,
}

impl Orientation {
    /// Logical canvas size that scenes for this orientation are authored against.
    pub fn logical_size(self) -> (u16, u16) {
        match self {
            Orientation::Portrait => (PANEL_WIDTH, PANEL_HEIGHT),
            Orientation::Landscape => (PANEL_HEIGHT, PANEL_WIDTH),
        }
    }

    /// Lowercase tag used for scene filenames (`boot.<tag>.toml`) and logs.
    pub fn tag(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }
}

impl std::fmt::Display for Orientation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.tag())
    }
}

impl std::str::FromStr for Orientation {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "portrait" | "p" => Ok(Orientation::Portrait),
            "landscape" | "l" => Ok(Orientation::Landscape),
            other => Err(format!(
                "unknown orientation '{other}' (expected portrait|landscape)"
            )),
        }
    }
}

/// USB identifiers reported by the 3.5" panel (QinHeng CH34x CDC-ACM).
pub const TURZX_VID: u16 = 0x1a86;
/// USB identifiers reported by the 3.5" panel (QinHeng CH34x CDC-ACM).
pub const TURZX_PID: u16 = 0x5722;

#[cfg(feature = "serial")]
mod protocol;
#[cfg(feature = "serial")]
mod serial;
#[cfg(feature = "serial")]
pub use serial::SerialTurzx;
