# TURZX 3.5" USB protocol — working notes

**Panel:** TURZX / Turing 3.5", 320×480 IPS, USB-C, idle screen
*"PLEASE RUN THE APP WWW.TURZX.COM"*.
Enumerates as USB CDC-ACM (serial): `VID 0x1a86`, `PID 0x5722` (QinHeng CH34x),
USB serial number `USB35INCHIPSV2`.
- Linux: `/dev/ttyACM*`
- macOS: `/dev/cu.usbmodem*`

## Status

**Confirmed** against `turing-smart-screen-python`'s `lcd_comm_rev_a` driver
(the "revision A" 3.5" panel). Implemented in `crates/turzx/src/protocol.rs` and
`crates/turzx/src/serial.rs`.

## Link parameters

| Parameter | Value |
|-----------|-------|
| Baud rate | 115 200 |
| Flow control | RTS/CTS (hardware) |
| Read timeout | 1 s |

## Command framing

Every command is a **6-byte packet**: five bytes packing up to four 10-bit
coordinates, then a one-byte opcode.

```
b0 = x  >> 2
b1 = ((x  & 0x03) << 6) | (y  >> 4)
b2 = ((y  & 0x0f) << 4) | (ex >> 6)
b3 = ((ex & 0x3f) << 2) | (ey >> 8)
b4 =   ey & 0xff
b5 = opcode
```

| Opcode | Value | Notes |
|--------|-------|-------|
| `HELLO` | `69` | Sent as **six** `69` bytes (not the coord form). Panel replies with a 6-byte hardware-id string; may also stay silent. |
| `SET_BRIGHTNESS` | `110` | Level `0..=255` rides in the `x` slot, **inverted**: wire value = `255 - level` (0 = brightest, 255 = off). |
| `SET_ORIENTATION` | `121` | 16-byte packet, see below. |
| `DISPLAY_BITMAP` | `197` | `(x, y)..=(ex, ey)` inclusive; RGB565-LE pixel data follows. |

Other rev A opcodes not yet used here: `RESET=101`, `CLEAR=102`, `TO_BLACK=103`,
`SCREEN_OFF=108`, `SCREEN_ON=109`, `SET_MIRROR=122`, `DISPLAY_PIXELS=195`.

### SET_ORIENTATION

16 bytes: coord prefix all-zero, `b5 = 121`, then

```
b6  = orientation + 100      # PORTRAIT = 0, LANDSCAPE = 2  -> 100 / 102
b7  = width  >> 8
b8  = width  & 0xff
b9  = height >> 8
b10 = height & 0xff
b11..b15 = 0
```

We always drive the panel as **portrait 320×480**; a landscape logical frame is
rotated 90° CW in software (`Frame::rotated_cw`) before sending. Whether the
panel-side landscape mode is preferable (and its exact rotation direction) is
still untested.

### DISPLAY_BITMAP payload

`RGB565`, **little-endian**, one `u16` per pixel, row-major, `(ex-x+1)*(ey-y+1)`
pixels. Streamed straight after the command packet with no framing. We hand it
to the OS in `PANEL_WIDTH * 8` = 2560-byte writes (the reference driver's
`chunked(rgb565_bytes, width * 8)`) and let RTS/CTS pace the panel; the chunk
size is only a write granularity, not a wire frame. Partial rectangles work and
are used for dirty-region updates in portrait.

## Handshake / init sequence

All in `SerialTurzx::open_path`, except the frames:

1. Open at 115 200, RTS/CTS.
2. Write `HELLO` (`[69; 6]`); read up to 6 bytes (a sub-revision code — panel
   may also stay silent), then flush input.
3. `SET_ORIENTATION` (portrait, 320, 480). We always drive the panel portrait
   and rotate landscape frames in software.
4. `SET_BRIGHTNESS` (daemon sends `255` right after opening the backend).
5. First `present`: `DISPLAY_BITMAP` for the full frame; subsequent presents
   send per-frame dirty rects (full frame again in landscape).

## References

- `turing-smart-screen-python` — `library/lcd/lcd_comm_rev_a.py`. Linux support
  for the TURZX/Turing 3.5" `USB35INCHIPSV2`.
- `big-screen-monitor-display` — same panel family over CDC-ACM.
