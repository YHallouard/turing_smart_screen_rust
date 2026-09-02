# Boot Horus I — implementation notes

Port of the Claude Design project *"Animation de boot Horus I"*
(`Boot Horus I.dc.html` → `boot-horus.jsx`) into `crates/renderer`.

## Choreography

10 s, 30 fps, authored natively per orientation (never rotated). The design's
cue table (`OM_SCENES` + per-section `nat`) resolves to **absolute** seconds,
which is what the TOML keyframes use directly:

| Cue | t (s) | Beat |
|-----|-------|------|
| Power | 0.0 | panel wakes: white flash, gold sweep, frame + rings trace in |
| Trace | 1.4 | the eye is drawn stroke-by-stroke in gold |
| Ignite | 4.8 | the outline fills with the gold gradient; warm flash; rings lock |
| Wordmark | 6.4 | `HORUS I` appears letter by letter, then `GAMIN CORP` |
| Handoff | 8.2 | seal shrinks to a top badge; GPU/CPU/VRAM/FPS dashboard rises |
| end | 10.0 | fade to `#05060a` (loop reset) |

Layouts (`LAYOUTS.portrait` / `LAYOUTS.landscape`) map 1:1 to the two scene
files. Portrait stacks (eye centred, wordmark low, 1-column dashboard); landscape
is left/right (eye left, wordmark right left-aligned, 2-column dashboard).

## Engine primitives added (was `BOOT_PRIMITIVES.md` in the design project)

| § | Feature | Where |
|---|---------|-------|
| 1 | multi-keyframe `Anim` (`keys = [{t,v}]`) + `ease_out_cubic` / `ease_in_out_quad` / `ease_out_back` / `ease_out_quad` | `anim.rs` |
| 2 | `path` layer: SVG `d` subset parser → `tiny_skia::Path`, `trace` via dashed stroke, `path_length` | `path.rs`, `engine.rs` |
| 3 | `[[gradient]]` (linear/radial, `axis`) → `tiny_skia` shader for fills, CPU sampler for glyphs | `paint.rs` |
| 4 | `circle` layer: outline, `trace`, `rotation`, `radius * scale` | `engine.rs`, `framebuffer.rs` |
| 5 | per-layer transform: `scale` / `rotation` / `anchor` on `path` / `image` / `text` | `engine.rs` |
| 6 | `glow` (separable box blur, additive composite) and `blend = "add"` | `framebuffer.rs`, `engine.rs` |
| 7 | TTF text via `fontdue` (Cinzel / Rajdhani / JetBrains Mono), `letter_spacing`, `align`, per-letter reveal from `trace` | `font.rs`, `engine.rs` |
| 8 | `scanlines` layer | `framebuffer.rs` |

Rasterisation is `tiny-skia` (pure Rust). Dirty bboxes inflate by the glow radius
and, for rotated layers, to the circumscribed square.

## Known simplifications vs the reference

- The ring "lock" overshoot (`easeOutBack` 1.07→1.0 at Ignite) is folded into the
  handoff `scale` keyframes with `ease_out_cubic` — no visible pre-Ignite bump.
- `breathe` (±1.6 % sine on the eye) is dropped.
- Stroke width tracks `1/scale` (constant screen width) rather than the design's
  `1/ratio` (which thickens as the seal shrinks).
- The tweak panel (palette / glow / scanlines toggles) is design-time only; the
  scene bakes the warm-gold palette.

## Preview

```sh
cargo run -p bc250-dashboard -- --backend png --capture --fps 30 \
  --scene assets/scenes/boot.portrait.toml --out target/hp
cargo run -p bc250-dashboard -- --backend png --capture --fps 30 --orientation landscape \
  --scene assets/scenes/boot.landscape.toml --out target/hl
```
