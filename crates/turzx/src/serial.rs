//! USB CDC-ACM driver for the 3.5" panel. Enabled by the `serial` feature.
//!
//! Works on Linux (`/dev/ttyACM*`) and macOS (`/dev/cu.usbmodem*`) — the panel
//! enumerates as a plain serial device, so you can drive it straight from a
//! desktop for animation work without Steam or a BC-250 in the loop.
//!
//! The wire protocol is the "revision A" one in [`crate::protocol`], confirmed
//! against a panel reporting serial `USB35INCHIPSV2`.

use std::borrow::Cow;
use std::io::{Read, Write};
use std::time::Duration;

use serialport::{ClearBuffer, FlowControl};

use crate::protocol::Command;
use crate::{
    BackendError, DisplayBackend, Frame, Orientation, Rect, Result, PANEL_HEIGHT, PANEL_WIDTH,
    TURZX_PID, TURZX_VID,
};

/// Panel baud rate (fixed in firmware). Matches `turing-smart-screen-python`.
const BAUD: u32 = 115_200;
/// Pixel-stream write granularity, in bytes: `PANEL_WIDTH * 8`, exactly the
/// reference driver's `chunked(data, width * 8)`. Hardware flow control paces
/// the panel; this is just how much we hand the OS per `write`.
const CHUNK_BYTES: usize = PANEL_WIDTH as usize * 8;

pub struct SerialTurzx {
    port: Box<dyn serialport::SerialPort>,
    /// Physical pixel array, always 320x480 on the wire.
    size: (u16, u16),
    orientation: Orientation,
    /// Cleared until the first `present`, which always sends a full frame.
    first_frame_done: bool,
}

impl SerialTurzx {
    /// Open the first serial port whose USB VID/PID matches the panel.
    pub fn open() -> Result<Self> {
        let ports =
            serialport::available_ports().map_err(|e| BackendError::Other(e.to_string()))?;
        let path = ports
            .into_iter()
            .find_map(|p| match &p.port_type {
                serialport::SerialPortType::UsbPort(usb)
                    if usb.vid == TURZX_VID && usb.pid == TURZX_PID =>
                {
                    Some(p.port_name)
                }
                _ => None,
            })
            .ok_or(BackendError::DeviceNotFound {
                vid: TURZX_VID,
                pid: TURZX_PID,
            })?;
        Self::open_path(&path)
    }

    /// Open a specific device node (e.g. `/dev/ttyACM0`).
    pub fn open_path(path: &str) -> Result<Self> {
        let mut port = serialport::new(path, BAUD)
            .timeout(Duration::from_millis(1_000))
            .flow_control(FlowControl::Hardware)
            .open()
            .map_err(|e| BackendError::Other(format!("{path}: {e}")))?;

        // Handshake: six HELLO bytes; the panel answers with a 6-byte
        // sub-revision code (matches the reference driver's `serial_read(6)`).
        // Best-effort: some firmware revisions stay silent but still accept the
        // command stream, so a timeout here is not fatal.
        port.write_all(&Command::Hello.encode())?;
        let mut code = [0u8; 6];
        match port.read(&mut code) {
            Ok(n) => log::debug!("panel hello -> {:02x?}", &code[..n]),
            Err(e) => log::debug!("panel hello: no response ({e})"),
        }
        let _ = port.clear(ClearBuffer::Input);

        // The panel is always driven as portrait 320x480; a landscape logical
        // frame is rotated in software before sending. Set this once, up front,
        // in the same order the reference driver uses (hello -> orientation ->
        // brightness -> bitmap).
        port.write_all(
            &Command::SetOrientation {
                landscape: false,
                width: PANEL_WIDTH,
                height: PANEL_HEIGHT,
            }
            .encode(),
        )?;

        Ok(Self {
            port,
            size: (PANEL_WIDTH, PANEL_HEIGHT),
            orientation: Orientation::Portrait,
            first_frame_done: false,
        })
    }

    /// Set the mounting orientation. In `Landscape` the backend rotates each
    /// 480x320 logical frame onto the physical 320x480 array before sending;
    /// the panel itself is always driven as portrait.
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    fn send(&mut self, cmd: Command) -> Result<()> {
        self.port.write_all(&cmd.encode())?;
        Ok(())
    }
}

impl DisplayBackend for SerialTurzx {
    fn size(&self) -> (u16, u16) {
        self.size
    }

    fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<()> {
        let (w, h) = self.size;
        let full = [Rect::new(0, 0, w, h)];

        // Landscape: rotate the 480x320 logical frame onto the physical array
        // and resend in full (rotated dirty rects don't map back to logical
        // space). Portrait: send dirty rects, except force a full repaint on
        // the first frame or when nothing was flagged.
        let (frame, regions): (Cow<Frame>, &[Rect]) = match self.orientation {
            Orientation::Landscape => (Cow::Owned(frame.rotated_cw()), &full),
            Orientation::Portrait if self.first_frame_done && !dirty.is_empty() => {
                (Cow::Borrowed(frame), dirty)
            }
            Orientation::Portrait => (Cow::Borrowed(frame), &full),
        };

        let rgb565 = frame.to_rgb565();
        for r in regions {
            // Clamp to the frame: a stray dirty rect must never index past
            // `rgb565` or overflow the 10-bit coordinate fields.
            let rx = r.x.min(w);
            let ry = r.y.min(h);
            let rw = r.w.min(w - rx);
            let rh = r.h.min(h - ry);
            if rw == 0 || rh == 0 {
                continue;
            }
            self.send(Command::DrawBitmap {
                x: rx,
                y: ry,
                ex: rx + rw - 1,
                ey: ry + rh - 1,
            })?;

            let mut buf = Vec::with_capacity(rw as usize * rh as usize * 2);
            for row in ry..ry + rh {
                let base = row as usize * w as usize;
                for col in rx..rx + rw {
                    // rev A expects RGB565 little-endian.
                    buf.extend_from_slice(&rgb565[base + col as usize].to_le_bytes());
                }
            }
            for chunk in buf.chunks(CHUNK_BYTES) {
                self.port.write_all(chunk)?;
            }
        }
        self.port.flush()?;

        // Only now is a full baseline guaranteed on the panel; a failure above
        // returns early and leaves this clear so the next call resends in full.
        self.first_frame_done = true;
        Ok(())
    }

    fn set_brightness(&mut self, level: u8) -> Result<()> {
        self.send(Command::SetBrightness(level))
    }
}
