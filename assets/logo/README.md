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
Fonts in the original mock: *Cinzel* (wordmark), *Rajdhani* (labels).

**Web font caveat:** the wordmark SVGs pull *Cinzel* from Google Fonts (`@font-face` in
`<defs>`). It renders correctly in a browser online; offline or when rasterising it
falls back to a serif. For a fully portable asset, open the file in Inkscape and
*Path → Object to Path* on the text, or `inkscape --export-text-to-path`.

## On the 3.5" panel

The scene engine's `image` layer wants raster (PNG), not SVG. Rasterise once:

```sh
# any of these
rsvg-convert -w 240 -h 240 assets/logo/horus-seal.svg -o assets/logo/horus-seal-240.png
inkscape assets/logo/horus-seal.svg --export-type=png -w 240 -o assets/logo/horus-seal-240.png
```

then reference it from a scene:

```toml
[[layer]]
type = "image"
source = "../logo/horus-seal-240.png"
x = 40
y = 40
width = 240
height = 240
```
