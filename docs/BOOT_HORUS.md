# Boot + dashboard — minimal-motion scenes

Re-authored (from the Claude Design canvas *Boot Horus Minimal*) for the real
TURZX 3.5" **revision A** panel, which repaints a full 320×480 screen in
~0.5–1 s. The old 10 s stroke-traced boot is in git history (commit `99670e5`).

Two scenes, played back to back by the daemon in its default `--mode sequence`:

| Scene | Files | Role |
|---|---|---|
| **boot** | `assets/scenes/boot.{portrait,landscape}.toml` | seal + name, ~3 s, then the daemon hard-cuts away |
| **dashboard** | `assets/scenes/dashboard.{portrait,landscape}.toml` | steady state, re-rendered ~1×/s from live sensors, held forever |

Design rules, both scenes: **constant layer set**, **only `opacity` and
progress-bar `value` animate** (no `x`/`y`/`scale`/`rotation`), **no glow, no
per-letter reveal, no full-screen layer, static background**. The single
unavoidable full-screen frame is the boot→dashboard scene switch.

## boot

| t (s) | Beat | Layers changing |
|---|---|---|
| 0.0 | Seal drawn — frame-1 baseline (~1 s on hardware) | — |
| 0.6–1.2 | `HORUS I` fades in (`opacity`) | 1 |
| 1.2–1.8 | `GAMIN CORP` fades in | 1 |
| 1.8–3.0 | Hold | 0 |
| 3.0 | Daemon loads the dashboard scene → one full repaint | — |

The seal is `assets/logo/horus-seal-240.png` in an `image` layer. **The engine
blits images at native size × `scale` and ignores `width`/`height`**, so the
layer sets `scale = 190/240` (portrait) / `170/240` (landscape) to land the seal
in the design's box.

## dashboard

Static layout: header rule, `GPU`/`CPU`/`VRAM`/`FPS` rows (label + value +
progress bar) and `GPU TEMP`/`CPU TEMP` (text only). Values are `{{ … }}` slots
the daemon fills each poll:

| slot | source | slot | source |
|---|---|---|---|
| `gpu.pct` `gpu.frac` `gpu.temp` `vram*` | GPU source (below) | `cpu.pct` `cpu.frac` | `/proc/stat` delta |
| `cpu.temp` | `k10temp` / `coretemp` hwmon | `fps` `fps.frac` | MangoHud CSV log |

Every source is best-effort; a missing one renders a dash. Values are
space-padded to a fixed width by the daemon (`daemon/src/sensors.rs`) because the
5×7 bitmap font has no `align`.

### GPU source — `config.toml [sensors] gpu`

- **`amd`** — the first DRM card with `gpu_busy_percent` (the BC-250 APU on the
  target hardware). That counter is bimodal 0/~100 noise, so it is averaged over
  24 back-to-back reads then EMA-smoothed. VRAM is `mem_info_vram_{used,total}`
  — on the BC-250 that is the 2 GiB BIOS UMA carve-out (the other ~30 GiB is
  GTT); temp is the amdgpu hwmon `edge`.
- **`nvidia`** — one `nvidia-smi --query-gpu=utilization.gpu,memory.used,
  memory.total,temperature.gpu` per poll (~15 ms). Already smooth.
- **`auto`** (default) — `nvidia` if `nvidia-smi` works, else `amd`. On a real
  BC-250 that is `amd`; on a desktop with a discrete NVIDIA card it is `nvidia`.

### FPS via MangoHud

There is no passive FPS source on Linux, so the daemon tails the newest `*.csv`
in MangoHud's `output_folder` (auto-read from `~/.config/MangoHud/MangoHud.conf`,
overridable via `config.toml [sensors] mangohud_log_dir`). It takes the last
row's first column and shows `-` when the newest log is older than
`fps_stale_secs` (no game running).

To get a live figure, MangoHud must log continuously. Add to
`~/.config/MangoHud/MangoHud.conf`:

```
autostart_log=1
log_interval=1000
output_folder=/home/you
```

and run games through it — `mangohud %command%` in the Steam launch options, or
`MANGOHUD=1` globally.

## Measured dirty pixels (`--backend png --capture --fps 30`)

| Window | Portrait | Landscape |
|---|---|---|
| seal / hold | 0 | 0 |
| `HORUS I` fade | ~830 / frame | ~830 / frame |
| `GAMIN CORP` fade | ~580 / frame | ~580 / frame |
| dashboard, one value changes | < 4 000 | < 4 000 |
| dashboard, nothing changed | 0 | 0 |

Old scene: 307 200 changed px on **every** frame.

## Run

```sh
cargo test --workspace            # ok — 22 tests
cargo run -p bc250-dashboard -- --backend png --capture --fps 30 --mode single \
  --orientation portrait  --scene assets/scenes/boot.toml --out target/bp
cargo run -p bc250-dashboard -- --backend png --capture --fps 30 --mode single \
  --orientation landscape --scene assets/scenes/boot.toml --out target/bl
```

On the panel — boot, then the live dashboard, held until Ctrl-C:

```sh
cargo run -p bc250-dashboard --features serial -- --backend serial --port /dev/ttyACM0
```
