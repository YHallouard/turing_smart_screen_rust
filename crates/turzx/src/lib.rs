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

/// Native resolution of the 3.5" panel.
pub const PANEL_WIDTH: u16 = 320;
/// Native resolution of the 3.5" panel.
pub const PANEL_HEIGHT: u16 = 480;

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
