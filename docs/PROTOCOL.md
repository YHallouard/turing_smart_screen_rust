# TURZX 3.5" USB protocol — working notes

**Panel:** TURZX 3.5", 320×480 IPS, USB-C, "NO AIDA64" variant.
Enumerates as USB CDC-ACM (serial): `VID 0x1a86`, `PID 0x5722` (QinHeng CH34x).
- Linux: `/dev/ttyACM*`
- macOS: `/dev/cu.usbmodem*`

## Status

The wire format in `crates/turzx/src/protocol.rs` is an **unverified skeleton**
modelled on `turing-smart-screen-python` and `big-screen-monitor-display`. It has
NOT been confirmed against this panel. The `serial` backend logs a warning on
first `present`.

## To confirm (Phase 1)

1. `lsusb -v -d 1a86:5722` — record interface / endpoint layout.
2. Check whether the vendor tool speaks a 6-byte command header (5 payload
   bytes + 1 opcode) as the Python projects do for "revision B".
3. Confirm:
   - baud rate (skeleton assumes 1 152 000)
   - opcodes for hello / brightness / bitmap draw
   - coordinate packing (skeleton assumes 4×10-bit big-endian in 5 bytes)
   - pixel order and endianness of RGB565 (skeleton assumes big-endian)
   - whether partial-rectangle updates are supported (needed for dirty rendering)
   - whether a panel-side **orientation / rotation** opcode exists. If so, prefer
     it over `Frame::rotated_cw()` in `serial.rs` (and confirm the CW vs CCW
     direction — the software rotation currently assumes 90° CW).
4. Update `protocol.rs`, remove the warning in `serial.rs`, and record the
   confirmed values here.

## References

- `turing-smart-screen-python` — Linux support for TURZX 3.5"/5", multiple HW revs.
- `big-screen-monitor-display` — TURZX 3.5" over CDC-ACM, RGB565, dirty tiles,
  adjacent-region merging.
