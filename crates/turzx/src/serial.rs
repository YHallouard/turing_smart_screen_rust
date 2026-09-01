//! USB CDC-ACM driver for the 3.5" panel. Enabled by the `serial` feature.
//!
//! Works on Linux (`/dev/ttyACM*`) and macOS (`/dev/cu.usbmodem*`) — the panel
//! enumerates as a plain serial device, so you can drive it straight from a Mac
//! for animation work without Steam or a BC-250 in the loop.

use std::borrow::Cow;
use std::io::Write;
use std::time::Duration;

use crate::protocol::Command;
use crate::{
    BackendError, DisplayBackend, Frame, Orientation, Rect, Result, PANEL_HEIGHT, PANEL_WIDTH,
    TURZX_PID, TURZX_VID,
};

pub struct SerialTurzx {
    port: Box<dyn serialport::SerialPort>,
    /// Physical pixel array, always 320x480 on the wire.
    size: (u16, u16),
    orientation: Orientation,
    warned: bool,
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

    /// Open a specific device node.
    pub fn open_path(path: &str) -> Result<Self> {
        let port = serialport::new(path, 1_152_000)
            .timeout(Duration::from_millis(1_000))
            .open()
            .map_err(|e| BackendError::Other(format!("{path}: {e}")))?;
        let mut this = Self {
            port,
            size: (PANEL_WIDTH, PANEL_HEIGHT),
            orientation: Orientation::Portrait,
            warned: false,
        };
        this.send(Command::Hello, &[])?;
        Ok(this)
    }

    /// Set the mounting orientation. In `Landscape` the backend rotates each
    /// 480x320 logical frame onto the physical 320x480 array before sending.
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    fn send(&mut self, cmd: Command, data: &[u8]) -> Result<()> {
        self.port.write_all(&cmd.header())?;
        if !data.is_empty() {
            self.port.write_all(data)?;
        }
        Ok(())
    }
}

impl DisplayBackend for SerialTurzx {
    fn size(&self) -> (u16, u16) {
        self.size
    }

    fn present(&mut self, frame: &Frame, dirty: &[Rect]) -> Result<()> {
        if !self.warned {
            log::warn!(
                "SerialTurzx uses an UNVERIFIED protocol skeleton; expect garbage \
                 output until it is confirmed against hardware (see protocol.rs)"
            );
            self.warned = true;
        }

        let (w, h) = self.size;

        // In landscape the dirty rects are in logical (480x320) space and no
        // longer apply once rotated, so fall back to a full-frame send.
        let (frame, regions): (Cow<Frame>, Vec<Rect>) = match self.orientation {
            Orientation::Portrait if !dirty.is_empty() => (Cow::Borrowed(frame), dirty.to_vec()),
            Orientation::Portrait => (Cow::Borrowed(frame), vec![Rect::new(0, 0, w, h)]),
            Orientation::Landscape => (Cow::Owned(frame.rotated_cw()), vec![Rect::new(0, 0, w, h)]),
        };

        let rgb565 = frame.to_rgb565();
        for r in &regions {
            let mut buf = Vec::with_capacity(r.area() as usize * 2);
            for row in r.y..r.y + r.h {
                for col in r.x..r.x + r.w {
                    let idx = row as usize * w as usize + col as usize;
                    buf.extend_from_slice(&rgb565[idx].to_be_bytes());
                }
            }
            self.send(
                Command::DrawBitmap {
                    x: r.x,
                    y: r.y,
                    ex: r.x + r.w - 1,
                    ey: r.y + r.h - 1,
                },
                &buf,
            )?;
        }
        Ok(())
    }

    fn set_brightness(&mut self, level: u8) -> Result<()> {
        self.send(Command::SetBrightness(level), &[])
    }
}
