//! TURZX / Turing 3.5" wire protocol ("revision A").
//!
//! **Status: confirmed** against the panel that reports USB serial number
//! `USB35INCHIPSV2` (VID `0x1a86` / PID `0x5722`) — the 3.5" 320x480 dalle that
//! shows *"PLEASE RUN THE APP WWW.TURZX.COM"* when idle. The framing matches
//! `turing-smart-screen-python`'s `lcd_comm_rev_a` driver; see
//! `docs/PROTOCOL.md` for the byte-level notes.
//!
//! Framing: every command is a 6-byte packet — five bytes packing up to four
//! 10-bit coordinates, then a one-byte opcode. [`Command::SetOrientation`]
//! carries five extra payload bytes and is padded to 16. Bitmap pixel data
//! (RGB565, little-endian) is streamed straight after a [`Command::DrawBitmap`]
//! packet with no further framing.

/// A command sent to the panel. [`Command::encode`] produces the on-wire bytes;
/// any pixel payload is written separately by the caller.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Command {
    /// Handshake / wake: six repeated opcode bytes. The panel replies with a
    /// 6-byte sub-revision code (or nothing, on some firmware).
    Hello,
    /// Backlight level, `0..=255` (`0` = off). The panel's own scale runs the
    /// other way, so [`Command::encode`] inverts it.
    SetBrightness(u8),
    /// Tell the panel which way it is mounted and what logical size to expect.
    SetOrientation {
        landscape: bool,
        width: u16,
        height: u16,
    },
    /// Repaint the inclusive rectangle `(x, y)..=(ex, ey)`. RGB565 little-endian
    /// pixel data for `(ex - x + 1) * (ey - y + 1)` pixels follows.
    DrawBitmap { x: u16, y: u16, ex: u16, ey: u16 },
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum Opcode {
    Hello = 69,
    SetBrightness = 110,
    SetOrientation = 121,
    DrawBitmap = 197,
}

/// Pack up to four 10-bit coordinates into the 5-byte prefix rev A uses.
fn pack_coords(x: u16, y: u16, ex: u16, ey: u16) -> [u8; 5] {
    debug_assert!(
        x < 1024 && y < 1024 && ex < 1024 && ey < 1024,
        "coordinate exceeds the 10-bit wire field: ({x},{y})..=({ex},{ey})"
    );
    [
        (x >> 2) as u8,
        (((x & 0x3) << 6) | (y >> 4)) as u8,
        (((y & 0xf) << 4) | (ex >> 6)) as u8,
        (((ex & 0x3f) << 2) | (ey >> 8)) as u8,
        (ey & 0xff) as u8,
    ]
}

impl Command {
    /// Encode the command packet. Pixel data, when there is any, is the caller's
    /// job to write immediately afterwards.
    pub(crate) fn encode(&self) -> Vec<u8> {
        match *self {
            Command::Hello => vec![Opcode::Hello as u8; 6],

            Command::SetBrightness(level) => {
                // Panel scale is inverted: 0 = brightest, 255 = fully off. The
                // value rides in the "x" coordinate slot.
                let inv = 255 - level as u16;
                let c = pack_coords(inv, 0, 0, 0);
                vec![c[0], c[1], c[2], c[3], c[4], Opcode::SetBrightness as u8]
            }

            Command::SetOrientation {
                landscape,
                width,
                height,
            } => {
                let c = pack_coords(0, 0, 0, 0);
                let mut buf = vec![0u8; 16];
                buf[..5].copy_from_slice(&c);
                buf[5] = Opcode::SetOrientation as u8;
                // rev A: PORTRAIT = 0, LANDSCAPE = 2, offset by 100 on the wire.
                buf[6] = if landscape { 100 + 2 } else { 100 };
                buf[7] = (width >> 8) as u8;
                buf[8] = (width & 0xff) as u8;
                buf[9] = (height >> 8) as u8;
                buf[10] = (height & 0xff) as u8;
                buf
            }

            Command::DrawBitmap { x, y, ex, ey } => {
                let c = pack_coords(x, y, ex, ey);
                vec![c[0], c[1], c[2], c[3], c[4], Opcode::DrawBitmap as u8]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_is_six_repeated_opcodes() {
        assert_eq!(Command::Hello.encode(), vec![69, 69, 69, 69, 69, 69]);
    }

    #[test]
    fn brightness_is_inverted_and_packed_into_x() {
        // level 255 -> inv 0 -> all-zero coords, opcode 110.
        assert_eq!(
            Command::SetBrightness(255).encode(),
            vec![0, 0, 0, 0, 0, 110]
        );
        // level 0 -> inv 255 -> x = 255 -> b0 = 63, b1 = (3 << 6) = 192.
        assert_eq!(
            Command::SetBrightness(0).encode(),
            vec![63, 192, 0, 0, 0, 110]
        );
    }

    #[test]
    fn orientation_packet_is_16_bytes_with_size() {
        let buf = Command::SetOrientation {
            landscape: false,
            width: 320,
            height: 480,
        }
        .encode();
        assert_eq!(buf.len(), 16);
        assert_eq!(buf[5], 121);
        assert_eq!(buf[6], 100);
        assert_eq!([buf[7], buf[8]], [0x01, 0x40]); // 320
        assert_eq!([buf[9], buf[10]], [0x01, 0xe0]); // 480
        assert_eq!(
            Command::SetOrientation {
                landscape: true,
                width: 480,
                height: 320,
            }
            .encode()[6],
            102
        );
    }

    #[test]
    fn draw_bitmap_matches_reference_coord_packing() {
        // Full 320x480 frame: x=0 y=0 ex=319 ey=479. Hand-computed against the
        // rev A bit layout: b2 = 319 >> 6, b3 = ((319 & 63) << 2) | (479 >> 8),
        // b4 = 479 & 255.
        let buf = Command::DrawBitmap {
            x: 0,
            y: 0,
            ex: 319,
            ey: 479,
        }
        .encode();
        assert_eq!(buf, vec![0, 0, 4, 253, 223, 197]);
    }
}
