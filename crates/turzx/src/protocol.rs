//! TURZX 3.5" wire protocol.
//!
//! **Status: unverified skeleton.** The command framing here follows the shape
//! used by `turing-smart-screen-python` and `big-screen-monitor-display` for the
//! 3.5"/5" "revision B" panels (6-byte command header: 5 payload bytes + 1
//! opcode, RGB565 pixel data), but the exact opcodes and coordinate packing have
//! NOT been checked against real hardware yet.
//!
//! Phase 1 of the project is to confirm these bytes with the panel in hand
//! (`lsusb -v`, a capture of the vendor tool, or a port from the Python project)
//! and write the result up in `docs/PROTOCOL.md`. Until then the `serial`
//! backend logs a warning on first use.

/// A command sent to the panel as a fixed 6-byte header, optionally followed by
/// pixel data.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Command {
    /// Handshake / wake.
    Hello,
    /// Backlight level, `0..=255`.
    SetBrightness(u8),
    /// Update the inclusive rectangle `(x, y)..=(ex, ey)`; RGB565 data follows.
    DrawBitmap { x: u16, y: u16, ex: u16, ey: u16 },
}

#[repr(u8)]
enum Opcode {
    Hello = 0xca,
    Brightness = 0xcb,
    DrawBitmap = 0xcc,
}

impl Command {
    /// Encode the 6-byte command header.
    pub(crate) fn header(&self) -> [u8; 6] {
        match *self {
            Command::Hello => [0, 0, 0, 0, 0, Opcode::Hello as u8],
            Command::SetBrightness(level) => [level, 0, 0, 0, 0, Opcode::Brightness as u8],
            Command::DrawBitmap { x, y, ex, ey } => {
                // Four 10-bit coordinates packed big-endian into 5 bytes.
                let b0 = (x >> 2) as u8;
                let b1 = (((x & 0x3) << 6) | (y >> 4)) as u8;
                let b2 = (((y & 0xf) << 4) | (ex >> 6)) as u8;
                let b3 = (((ex & 0x3f) << 2) | (ey >> 8)) as u8;
                let b4 = (ey & 0xff) as u8;
                [b0, b1, b2, b3, b4, Opcode::DrawBitmap as u8]
            }
        }
    }
}
