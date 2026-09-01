# bc250-dashboard

A native Rust daemon for Bazzite / SteamOS that drives a **TURZX 3.5" (320×480)**
USB-C secondary screen: a boot animation, event-driven scenes, automatic Steam
game detection, and a permanent GPU/CPU/FPS dashboard — with negligible impact on
games (dirty-region rendering, mostly-idle event loop).

See [`infos.md`](infos.md) for the full design brief and [`docs/PROTOCOL.md`](docs/PROTOCOL.md)
for the panel protocol notes.

## Status

Phase 1–2 of the plan:

| Crate | What it does | State |
|-------|--------------|-------|
| `crates/turzx` | `Frame`, `DisplayBackend` trait, dirty-region tracking, USB serial driver (`serial` feature) | trait + preview solid; **serial protocol is an unverified skeleton** |
| `crates/renderer` | TOML scene / widget / keyframe-animation engine, CPU framebuffer, 5×7 bitmap font | working |
| `crates/preview` | non-hardware backends: PNG dump + desktop window (`window` feature) | working |
| `daemon` (`bc250-dashboard`) | loads a scene, plays it through a backend | working |

Not yet started: Steam detection, sensors, the event engine, the scene library.

## Build & test

```sh
make check      # fmt + clippy + build + test
cargo test --workspace
```

The default workspace build has **no system dependencies** (PNG backend only).
`--features window` pulls in `minifb`; `--features serial` pulls in `serialport`
(needs `libudev` on Linux).

## Run

Render the boot scene to `target/frames/*.png`:

```sh
cargo run -p bc250-dashboard -- --backend png
```

Live preview window — the way to iterate on animations on a Mac/desktop, no
hardware needed:

```sh
cargo run -p bc250-dashboard --features window -- --backend window --loop
```

Drive the real panel over USB serial (works from macOS too — the panel is just a
serial device at `/dev/cu.usbmodem*`). The protocol is unverified, so expect
garbage until Phase 1 is done:

```sh
cargo run -p bc250-dashboard --features serial -- --backend serial
# or point at a specific device:
cargo run -p bc250-dashboard --features serial -- --backend serial --port /dev/cu.usbmodem1101
```

macOS needs nothing extra. On Linux the `serial` feature links `libudev`, so
install `pkg-config` + `libudev-dev` (`libudev-devel` on Fedora/Bazzite) first.

## Orientation

The panel has no orientation sensor, so it is a static choice in `config.toml`
(read once at startup — change it, then restart):

```toml
[panel]
orientation = "portrait"   # 320x480   |   "landscape" -> 480x320
```

`--orientation portrait|landscape` overrides it for a run (handy for previewing).

Scenes are authored **per orientation**, not rotated: for `--scene .../boot.toml`
the daemon loads `boot.<orientation>.toml` next to it if present, else `boot.toml`.
The logical canvas passed to the engine is 320x480 or 480x320 accordingly; only
the serial backend maps a landscape frame onto the physical 320x480 array.

## Scene format

Scenes are TOML (`assets/scenes/*.toml`): a `[scene]` block (`name`, `duration`,
`background`, optional `orientation`) plus `[[layer]]`s of type `text`, `rect`,
`image` or `progress_bar`, each with optional `[[layer.anim]]` keyframes on
`opacity` / `x` / `y` / `scale` / `value`. Text and numeric fields interpolate
`{{ context.vars }}` supplied by the daemon. See `assets/scenes/boot.portrait.toml`
and `assets/scenes/boot.landscape.toml`.
