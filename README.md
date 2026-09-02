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
| `crates/turzx` | `Frame`, `DisplayBackend` trait, `Orientation`, dirty-region tracking, USB serial driver (`serial` feature) | trait + preview solid; **serial protocol is an unverified skeleton** |
| `crates/renderer` | TOML scene engine on `tiny_skia` (AA paths, gradients, transforms, additive blend, glow) + `fontdue` TTF text; multi-keyframe animation | working |
| `crates/preview` | non-hardware backends: PNG dump + desktop window (`window` feature) | working |
| `daemon` (`bc250-dashboard`) | loads the orientation-specific scene, plays it through a backend | working |

The reference **"Boot Horus I"** animation (10 s, both orientations — power sweep,
eye traced then ignited in gold, wordmark, seal→badge handoff, GPU/CPU/VRAM/FPS
dashboard) is implemented in `assets/scenes/boot.{portrait,landscape}.toml`.

Not yet started: Steam detection, sensors, the event engine, a wider scene library.

## Build & test

```sh
make check      # fmt + clippy + build + test
cargo test --workspace
```

The default workspace build has **no system dependencies** (PNG backend only;
`tiny-skia` and `fontdue` are pure Rust). `--features window` pulls in `minifb`;
`--features serial` pulls in `serialport` (needs `libudev` on Linux).

Fonts (Cinzel, Rajdhani, JetBrains Mono — all SIL OFL) are vendored under
`assets/fonts/` and embedded in the binary; see `assets/fonts/OFL-*.txt`.

## Run

Render the boot scene to `target/frames/*.png`:

```sh
cargo run -p bc250-dashboard -- --backend png
# deterministic filmstrip (step scene time by 1/fps, ignore the wall clock):
cargo run -p bc250-dashboard -- --backend png --capture --fps 30 --out target/frames
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

Scenes are TOML (`assets/scenes/*.toml`):

- `[scene]` — `name`, `duration`, `background` (`"#rrggbb[aa]"` or
  `"gradient:<name>"`), optional `orientation`.
- `[[gradient]]` — `name`, `kind` (`linear`/`radial`), `axis`
  (`vertical`/`horizontal`), `stops = [{ at, color }]`.
- `[[layer]]` — `type` is `text` (bitmap 5×7 or, with `font =
  "cinzel"|"rajdhani"|"mono"`, TTF at `size`, `letter_spacing`, `align`),
  `rect`, `image`, `progress_bar`, `path` (`d = "@horus_eye"` or inline / an
  `.svg`), `circle` (`radius`), or `scanlines`. Common fields: `x/y/width/height`,
  `scale`, `rotation`, `anchor` (`top_left`/`center`), `fill`/`stroke`
  (paint spec), `stroke_width`, `blend` (`normal`/`add`), `glow` + `glow_radius`,
  `trace` (0..1 outline / letter reveal).
- `[[layer.anim]]` — `property` (`opacity`/`x`/`y`/`width`/`height`/`scale`/
  `rotation`/`trace`/`value`), `easing`, and either `keys = [{ t, v }, …]` or the
  shorthand `from`/`to`/`start`/`end`.

Text and numeric fields interpolate `{{ context.vars }}` from the daemon. Full
worked example: `assets/scenes/boot.portrait.toml` /
`assets/scenes/boot.landscape.toml`.
