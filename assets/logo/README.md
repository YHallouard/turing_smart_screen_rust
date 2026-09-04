# Horus I — logo assets

The rig identity is the **Wedjat** (Eye of Horus). Source glyph is a public-domain /
CC0 vector (freesvg.org #176178), recoloured for this project.

| File | What | Use |
|------|------|-----|
| `horus-eye.svg` | glyph only, `fill=currentColor` (defaults gold `#c9a24b`), transparent, viewBox `0 0 666.667 666.667` | favicon, monochrome mark, re-themeable |
| `horus-seal.svg` | glyph inside the circular seal: gold rings + radial-dark plate + gold-gradient glyph + drop shadow, 512² | boot screen, splash, app icon |
| `horus-wordmark.svg` | the name alone — `HORUS I` in Cinzel 600, gold gradient, 700×140 | text-only lockup, header |
| `horus-lockup.svg` | large glyph (**no ring**) tight above the wordmark, 700×450 | primary logo on dark |
| `horus-lockup-seal.svg` | large **seal** (with ring) tight above the wordmark, 700×672 | badge / hero lockup |

Palette: ink `#0e0e12` / `#202029`, gold `#8a6f37` → `#c9a24b` → `#eece86`, lapis `#2a5583`.
Fonts: *Cinzel* (wordmark), *Rajdhani* (labels), *JetBrains Mono* (values) — all
SIL OFL, vendored in `assets/fonts/` and embedded in the daemon.

`horus-eye.d` is the eye outline's raw `d` string (the scene engine embeds it as
`renderer::HORUS_EYE_PATH`, referenced from scenes as `d = "@horus_eye"`).

**Web font caveat (SVG files only):** the wordmark SVGs pull *Cinzel* from Google
Fonts (`@font-face` in `<defs>`) — fine in a browser online, falls back to a serif
offline / when rasterising. The scene engine does **not** use the SVGs for text;
it renders the vendored TTFs directly.

## On the 3.5" panel

The boot scene (`assets/scenes/boot.{portrait,landscape}.toml`) uses a raster
seal in an `image` layer: **`horus-seal-240.png`** (240², committed). Regenerate
it from the SVG if the artwork changes:

```sh
rsvg-convert -w 240 -h 240 assets/logo/horus-seal.svg -o assets/logo/horus-seal-240.png
```

If `rsvg-convert` isn't installed but `librsvg` is (Pop!_OS/Ubuntu ship the lib
without the CLI), rasterise via PyGObject + cairo — and strip the
`filter="url(#hs-shadow)"` drop-shadow first, some librsvg builds drop the glyph
inside a filtered group:

```python
import gi; gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg
import cairo
svg = open("assets/logo/horus-seal.svg").read().replace(' filter="url(#hs-shadow)"', "")
h = Rsvg.Handle.new_from_data(svg.encode())
s = cairo.ImageSurface(cairo.FORMAT_ARGB32, 240, 240)
c = cairo.Context(s); c.scale(240/512, 240/512); h.render_cairo(c)
s.write_to_png("assets/logo/horus-seal-240.png")
```

The scene engine can also draw the eye as a live `path` layer
(`d = "@horus_eye"`) with no PNG — but that is the bare glyph, without the seal's
rings and plate.
