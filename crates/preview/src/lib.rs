//! [`turzx::DisplayBackend`] implementations that need no panel: a PNG frame
//! dump (headless / CI) and — behind the `window` feature — a desktop preview
//! window for iterating on animations without hardware.

mod png_backend;
pub use png_backend::PngBackend;

#[cfg(feature = "window")]
mod window_backend;
#[cfg(feature = "window")]
pub use window_backend::WindowBackend;
