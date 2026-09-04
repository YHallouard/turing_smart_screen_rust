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
| `crates/turzx` | `Frame`, `DisplayBackend` trait, `Orientation`, dirty-region tracking, USB serial driver (`serial` feature) | trait + preview solid; **serial protocol = confirmed rev A** (`USB35INCHIPSV2`, see `docs/PROTOCOL.md`) |
| `crates/renderer` | TOML scene engine on `tiny_skia` (AA paths, gradients, transforms, additive blend, glow) + `fontdue` TTF text; multi-keyframe animation | working |
| `crates/preview` | non-hardware backends: PNG dump + desktop window (`window` feature) | working |
| `daemon` (`bc250-dashboard`) | `--mode sequence` (default): boot scene once, then the live dashboard forever from sysfs / `nvidia-smi` / MangoHud; `--mode single`: play one scene and exit | working |

**Boot + dashboard** (`assets/scenes/{boot,dashboard}.{portrait,landscape}.toml`):
a minimal-motion boot — static Horus seal, name fades in, hard cut — then a
GPU/CPU/VRAM/FPS/temps dashboard fed by live sensors. Re-authored for the real
rev-A panel (full-screen repaint ≈ 1 s); see `docs/BOOT_HORUS.md`.

Hardware alerts (GPU temp / VRAM / fan-stopped) preempt the dashboard with a
full-screen scene while a reading is past its threshold; a Steam achievement
unlock pops `notify.achievement` for a few seconds — see **Alerts** below.
Not yet started: a wider scene library, a "now playing" dashboard line.

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

## Install

### `.deb` (Debian / Ubuntu / Pop!_OS)

```sh
sudo apt install pkg-config libudev-dev dpkg-dev   # build deps
make deb                                           # -> dist/bc250-dashboard_<ver>_amd64.deb
sudo apt install ./dist/bc250-dashboard_*.deb      # that's it
```

The `.deb`'s `postinst` reloads udev, enables the systemd **user** unit for all
users, and — since `sudo` tells it who you are — does `daemon-reload` +
`enable --now` + `restart` in your session and `loginctl enable-linger` so it also
starts at boot before login. No manual `systemctl` step.

Installs `/usr/bin/bc250-dashboard` (with the `config` subcommand), assets under
`/usr/share/bc250-dashboard`, the udev rule (`/lib/udev/rules.d/70-turzx-panel.rules`
→ `/dev/turzx-panel`, world-rw), the user unit in `/usr/lib/systemd/user/`, and
`/etc/bc250-dashboard/config.toml` (a per-user copy is seeded into
`~/.config/bc250-dashboard/` on first run). Each `make deb` stamps a newer
version, so upgrading is just `make deb && sudo apt install ./dist/*.deb`. Logs:
`journalctl --user -u bc250-dashboard -f`. Remove with `sudo apt remove
bc250-dashboard`.

> Already ran `make install` (below) once? Its unit in `~/.config/systemd/user/`
> shadows the packaged one — run `make uninstall` before the first `apt install`.

> Bazzite / SteamOS are rpm/ostree — `apt` isn't available there; use
> `make install` below (writes only to `~/.local` + one udev rule).

### From a checkout (no packaging)

```sh
make install        # per-user: ~/.local/bin, ~/.local/share, ~/.config, user unit
make install-udev   # one-time, sudo: the udev rule + /dev/turzx-panel
make enable         # systemctl --user enable --now + loginctl enable-linger
```

`make disable` / `make uninstall` to undo.

### What the service does

`--mode sequence`: boot animation, then the live dashboard. It writes a managed
block into `~/.config/MangoHud/MangoHud.conf` (`autostart_log` / `log_interval` /
`output_folder`) so in-game FPS is logged and picked up automatically — set
`[sensors] manage_mangohud = false` to opt out. It also prunes old FPS CSVs
(`mangohud_prune_hours`, default 24).

### Alerts

While the dashboard is up the daemon watches the same sensors and, when one
crosses its threshold, takes over the panel with `assets/scenes/alert.<orientation>.toml`
— a parametric full-screen scene the daemon fills with the live value, the
threshold, and a suggested action. It clears once the reading drops back past a
lower `*_clear` threshold (hysteresis) and `min_secs` has elapsed; a
higher-precedence alert (fan-stopped > GPU temp > VRAM) preempts immediately.
Tune or disable it under `[alerts]` in `config.toml` (or `bc250-dashboard config
set alerts.gpu_temp_c 90`).

### Steam (`[steam]`, default on)

The daemon finds the running Steam game from `/proc/<pid>/environ`
(`SteamAppId`), then reads Steam's own local caches under
`~/.steam/…/appcache/stats/` — no Steamworks link, no Web API key, no network.

- **Launch card** — when a game starts, `launch.<orientation>.toml` plays for
  ~10 s: the game name + one stat (`launch_stat`, default the achievement
  `unlocked / total`), a fade in / hold / fade out. `launch_animation = false`
  to skip it.
- **Achievement popup** — a fresh unlock in
  `UserGameStats_<account>_<appid>.bin` pops `notify.achievement` (name, game,
  `unlocked / total`) for `achievement_secs`.

Precedence: a hardware alert preempts either; the launch card preempts the
achievement popup.

### Configure from the CLI

```sh
bc250-dashboard config show                         # every setting + effective value
bc250-dashboard config set panel.orientation landscape
bc250-dashboard config set sensors.gpu nvidia       # auto | amd | nvidia
bc250-dashboard config set sensors.fps_bar_max 144
bc250-dashboard config get sensors.gpu
bc250-dashboard config edit                         # $EDITOR the file
systemctl --user restart bc250-dashboard            # apply
```

Edits are format-preserving and validated before they are written (a bad value
is refused, not saved). Target file:
`$XDG_CONFIG_HOME/bc250-dashboard/config.toml` (`--file` to override).

## Run (from a checkout)

```sh
# deterministic filmstrip, no hardware:
cargo run -p bc250-dashboard -- --backend png --mode single --capture --fps 30 \
  --scene assets/scenes/boot.toml --out target/frames

# live preview window (iterate on animations on a desktop):
cargo run -p bc250-dashboard --features window -- --backend window --mode single --loop \
  --scene assets/scenes/boot.toml

# the real panel: boot, then the live dashboard (Ctrl-C to stop):
cargo run -p bc250-dashboard --features serial -- --backend serial
```

The panel is a serial device (`/dev/ttyACM*` on Linux, `/dev/cu.usbmodem*` on
macOS); the wire protocol is the confirmed "revision A" one (`docs/PROTOCOL.md`).
Without the udev rule the node is `root:dialout` — add yourself to that group
(`sudo usermod -aG dialout $USER`, re-login) or run with `sudo`. On Linux the
`serial` feature links `libudev` (`pkg-config` + `libudev-dev`).

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
  "cinzel"|"rajdhani"|"mono"`, TTF at `size`, `letter_spacing`, `align`;
  `max_width` truncates an over-long bitmap string with `...`),
  `rect`, `stroke_rect` (border only: `stroke`/`color` + `stroke_width`),
  `image`, `progress_bar`, `path` (`d = "@horus_eye"` or inline / an `.svg`),
  `circle` (`radius`), or `scanlines`. Common fields: `x/y/width/height`,
  `scale`, `rotation`, `anchor` (`top_left`/`center`), `fill`/`stroke`
  (paint spec), `stroke_width`, `blend` (`normal`/`add`), `glow` + `glow_radius`,
  `trace` (0..1 outline / letter reveal).
- `[[layer.anim]]` — `property` (`opacity`/`x`/`y`/`width`/`height`/`scale`/
  `rotation`/`trace`/`value`), `easing`, and either `keys = [{ t, v }, …]` or the
  shorthand `from`/`to`/`start`/`end`.

Text and numeric fields interpolate `{{ context.vars }}` from the daemon. Full
worked example: `assets/scenes/boot.portrait.toml` /
`assets/scenes/boot.landscape.toml`.
