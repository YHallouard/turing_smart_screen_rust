//! Phase 1-3 daemon: play the boot scene, then drive the live dashboard scene
//! from system sensors — or, in `--mode single`, just play one scene and exit
//! (the way `--capture` / animation iteration wants it).

mod alerts;
mod config;
mod configctl;
mod mangohud;
mod sensors;
mod steam;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use config::Config;
use renderer::{Context, SceneEngine, SceneFile};
use turzx::{DisplayBackend, Orientation};

/// With no subcommand the daemon runs (all the flags below); `config` reads or
/// changes the config file.
#[derive(Parser, Debug)]
#[command(name = "bc250-dashboard", version, about)]
struct App {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    run: Cli,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Read or change the daemon config file.
    Config {
        /// Config file (default: $XDG_CONFIG_HOME/bc250-dashboard/config.toml).
        #[arg(long, global = true)]
        file: Option<PathBuf>,
        #[command(subcommand)]
        action: configctl::Action,
    },
}

#[derive(Args, Debug)]
struct Cli {
    /// Boot scene to play. `<name>.<orientation>.toml` next to it wins if present.
    #[arg(short, long, default_value = "assets/scenes/boot.toml")]
    scene: PathBuf,

    /// Steady-state scene for `--mode sequence`, rendered from live sensors.
    #[arg(long, default_value = "assets/scenes/dashboard.toml")]
    dashboard: PathBuf,

    /// `sequence` = boot scene once, then the live dashboard forever.
    /// `single` = play `--scene` only (honours --capture/--end-at/--loop/--hold).
    #[arg(long, value_enum, default_value = "sequence")]
    mode: Mode,

    /// Config file (TOML). A missing file just means defaults.
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    /// Override `panel.orientation` from the config (portrait|landscape).
    #[arg(long, value_parser = parse_orientation)]
    orientation: Option<Orientation>,

    /// Where rendered frames go.
    #[arg(long, value_enum, default_value = "png")]
    backend: BackendKind,

    /// Output directory for the `png` backend.
    #[arg(long, default_value = "target/frames")]
    out: PathBuf,

    /// Pixel zoom for the `window` backend.
    #[arg(long, default_value_t = 2)]
    scale: usize,

    /// Render rate, frames per second.
    #[arg(long, default_value_t = 30)]
    fps: u32,

    /// Sensor poll interval for the dashboard, in seconds.
    #[arg(long, default_value_t = 1.0)]
    poll: f32,

    /// `single` mode: loop the scene instead of exiting when it ends.
    #[arg(long = "loop")]
    loop_scene: bool,

    /// `single` mode: one-shot playback stop time, in seconds (default: the
    /// scene duration). Lets a scene that fades out end on a meaningful frame.
    #[arg(long)]
    end_at: Option<f32>,

    /// `single` mode: after the scene finishes, hold the final frame on the
    /// panel (resent every 2 s). Ctrl-C to exit. Ignored with `--loop`.
    #[arg(long)]
    hold: bool,

    /// `single` mode: deterministic capture — step scene time by exactly `1/fps`
    /// each frame and render `duration * fps` frames, then exit. Implies
    /// `--mode single`.
    #[arg(long)]
    capture: bool,

    /// Serial device path for the `serial` backend; auto-detected if omitted.
    #[arg(long)]
    port: Option<String>,

    /// Dev: fire a fake achievement popup at startup (`--fake-achievement "Name"`).
    #[arg(long, hide = true)]
    fake_achievement: Option<String>,

    /// Dev: play the launch card at startup (`--fake-launch "Game Name"`).
    #[arg(long, hide = true)]
    fake_launch: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Mode {
    Sequence,
    Single,
}

fn parse_orientation(s: &str) -> Result<Orientation, String> {
    s.parse()
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackendKind {
    /// Write PNG frames to `--out`.
    Png,
    /// Live desktop window (requires `--features window`).
    Window,
    /// Real TURZX panel over USB serial (requires `--features serial`).
    Serial,
}

fn make_backend(
    cli: &Cli,
    size: (u16, u16),
    orientation: Orientation,
) -> Result<Box<dyn DisplayBackend>> {
    match cli.backend {
        BackendKind::Png => Ok(Box::new(preview::PngBackend::new(&cli.out, size)?)),
        BackendKind::Window => {
            #[cfg(feature = "window")]
            {
                Ok(Box::new(preview::WindowBackend::new(
                    &format!("TURZX 3.5\" preview ({orientation})"),
                    size,
                    cli.scale,
                )?))
            }
            #[cfg(not(feature = "window"))]
            {
                let _ = (cli, size, orientation);
                bail!("the window backend needs a build with `--features window`");
            }
        }
        BackendKind::Serial => {
            #[cfg(feature = "serial")]
            {
                let dev = match &cli.port {
                    Some(p) => turzx::SerialTurzx::open_path(p)?,
                    None => turzx::SerialTurzx::open()?,
                };
                Ok(Box::new(dev.with_orientation(orientation)))
            }
            #[cfg(not(feature = "serial"))]
            {
                let _ = (cli, size, orientation);
                bail!("the serial backend needs a build with `--features serial`");
            }
        }
    }
}

/// `dir/name.ext` -> `dir/name.<tag>.ext`.
fn variant_path(base: &Path, tag: &str) -> Option<PathBuf> {
    let stem = base.file_stem()?.to_str()?;
    let ext = base.extension()?.to_str()?;
    Some(base.with_file_name(format!("{stem}.{tag}.{ext}")))
}

/// Prefer an orientation-specific scene file next to `base`, else `base` itself.
fn orient_scene_path(base: &Path, orientation: Orientation) -> PathBuf {
    match variant_path(base, orientation.tag()) {
        Some(p) if p.is_file() => p,
        _ => base.to_path_buf(),
    }
}

/// Resolve, load, and build an engine for `base` at `orientation`.
fn load_scene(
    base: &Path,
    orientation: Orientation,
    size: (u16, u16),
) -> Result<(SceneFile, SceneEngine)> {
    let path = orient_scene_path(base, orientation);
    let scene = SceneFile::load(&path)?;
    if let Some(decl) = scene.scene.orientation.as_deref() {
        if !decl.eq_ignore_ascii_case(orientation.tag()) {
            log::warn!(
                "scene '{}' declares orientation '{decl}' but running as '{orientation}'",
                path.display()
            );
        }
    }
    let asset_base = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let mut engine = SceneEngine::new(size.0, size.1);
    engine
        .load_assets(&scene, &asset_base)
        .with_context(|| format!("loading assets for {}", path.display()))?;
    Ok((scene, engine))
}

/// BC-250 boot check-list from the spec, mapped onto normalised scene time.
fn boot_status(progress: f32) -> &'static str {
    const STAGES: [&str; 5] = [
        "BC-250",
        "GPU ONLINE",
        "VRAM CHECK",
        "THERMAL OK",
        "SYSTEM READY",
    ];
    let idx = (progress * STAGES.len() as f32) as usize;
    STAGES[idx.min(STAGES.len() - 1)]
}

fn main() -> Result<()> {
    let app = App::parse();
    if let Some(Command::Config { file, action }) = app.command {
        return configctl::run(file, action);
    }
    let cli = app.run;

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cfg = Config::load(&cli.config).context("loading config")?;
    let orientation = cli.orientation.unwrap_or(cfg.panel.orientation);
    let size = orientation.logical_size();

    let mut backend = make_backend(&cli, size, orientation)?;
    // Wake the backlight up front (no-op for the PNG / window backends).
    backend.set_brightness(255)?;

    let mode = if cli.capture { Mode::Single } else { cli.mode };
    log::info!(
        "{:?} mode, {orientation} {}x{} via {:?} backend",
        mode,
        size.0,
        size.1,
        cli.backend
    );

    match mode {
        Mode::Single => {
            let (scene, mut engine) = load_scene(&cli.scene, orientation, size)?;
            play_single(&mut *backend, &mut engine, &scene, &cli)?;
        }
        Mode::Sequence => {
            let (boot, mut engine) = load_scene(&cli.scene, orientation, size)?;
            log::info!("boot '{}' ({:.1}s)", boot.scene.name, boot.scene.duration);
            play_to_end(&mut *backend, &mut engine, &boot, cli.fps)?;

            run_steady_state(&mut *backend, orientation, size, &cli, &cfg)?;
        }
    }

    log::info!("done");
    Ok(())
}

/// Play `scene` once, wall-clock timed, until its declared duration. Context
/// carries the boot check-list line for scenes that show it.
fn play_to_end(
    backend: &mut dyn DisplayBackend,
    engine: &mut SceneEngine,
    scene: &SceneFile,
    fps: u32,
) -> Result<()> {
    let duration = scene.scene.duration.max(f32::EPSILON);
    let budget = Duration::from_secs_f32(1.0 / fps.max(1) as f32);
    let start = Instant::now();
    loop {
        let frame_start = Instant::now();
        let t = start.elapsed().as_secs_f32().min(duration);
        let last = t >= duration;

        let progress = (t / duration).clamp(0.0, 1.0);
        let mut ctx = Context::new();
        ctx.set("status", boot_status(progress));
        ctx.set("boot.progress", format!("{progress:.3}"));

        let (frame, dirty) = engine.render(scene, &ctx, t);
        backend.present(&frame, &dirty)?;
        if backend.should_close() || last {
            return Ok(());
        }
        if let Some(rem) = budget.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(rem);
        }
    }
}

/// Render `scene` forever, refreshing sensor values every `--poll` seconds.
/// The steady state after boot: the dashboard scene, with hardware alerts (a
/// parametric full-screen scene) preempting it while a reading is past its
/// threshold. Sensors are polled every `--poll` s; rules and the render both run
/// off that. Never returns (Ctrl-C / window close).
fn run_steady_state(
    backend: &mut dyn DisplayBackend,
    orientation: Orientation,
    size: (u16, u16),
    cli: &Cli,
    cfg: &Config,
) -> Result<()> {
    let budget = Duration::from_secs_f32(1.0 / cli.fps.max(1) as f32);
    let poll = Duration::from_secs_f32(cli.poll.max(0.1));
    let s = &cfg.sensors;

    // MangoHud CSV folder (and write MangoHud.conf if we manage it).
    let log_dir = if s.manage_mangohud {
        let dir = s
            .mangohud_log_dir
            .clone()
            .or_else(config::Sensors::default_log_dir);
        if let Some(dir) = &dir {
            if let Err(e) = mangohud::ensure_logging(dir) {
                log::warn!("could not configure MangoHud logging: {e}");
            }
        }
        dir
    } else {
        s.mangohud_log_dir.clone()
    };

    let mut sensors = sensors::Sensors::detect(
        s.gpu,
        log_dir,
        Duration::from_secs_f32(s.fps_stale_secs.max(1.0)),
        (s.mangohud_prune_hours > 0.0)
            .then(|| Duration::from_secs_f32(s.mangohud_prune_hours * 3600.0)),
    );

    let (dash_scene, mut dash_engine) = load_scene(&cli.dashboard, orientation, size)?;
    log::info!(
        "dashboard '{}' — live, Ctrl-C to exit",
        dash_scene.scene.name
    );

    let mut alert_scene = if cfg.alerts.enabled {
        Some(load_scene(
            &cli.dashboard.with_file_name("alert.toml"),
            orientation,
            size,
        )?)
    } else {
        None
    };
    let mut alerts = alerts::Alerts::new(cfg.alerts);

    let mut steam = cfg.steam.enabled.then(steam::Steam::detect).flatten();
    let mut notify_scene = if steam.is_some() {
        Some(load_scene(
            &cli.dashboard.with_file_name("notify.achievement.toml"),
            orientation,
            size,
        )?)
    } else {
        None
    };
    let ach_secs = cfg.steam.achievement_secs.max(0.5);
    let mut achievement: Option<(steam::Unlock, Instant)> =
        cli.fake_achievement.as_ref().map(|n| {
            if notify_scene.is_none() {
                notify_scene = load_scene(
                    &cli.dashboard.with_file_name("notify.achievement.toml"),
                    orientation,
                    size,
                )
                .ok();
            }
            (
                steam::Unlock {
                    game: "Test Game".into(),
                    name: n.clone(),
                    unlocked: 12,
                    total: 45,
                },
                Instant::now(),
            )
        });

    let want_launch = cfg.steam.launch_animation && (steam.is_some() || cli.fake_launch.is_some());
    let mut launch_scene = want_launch
        .then(|| {
            load_scene(
                &cli.dashboard.with_file_name("launch.toml"),
                orientation,
                size,
            )
        })
        .transpose()?;
    let launch_secs = launch_scene
        .as_ref()
        .map_or(4.0, |(sc, _)| sc.scene.duration.max(0.5));
    // Same key the `cover.png` `image` layer in launch.<orientation>.toml
    // uses as its `source` — the placeholder shipped in assets/launch/, and
    // the key `set_image` overwrites once a game's real cover art is found.
    const COVER_KEY: &str = "../launch/cover.png";
    let default_cover =
        orient_scene_path(&cli.dashboard.with_file_name("launch.toml"), orientation)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(COVER_KEY);
    let mut launch: Option<(steam::Launch, Instant)> = cli.fake_launch.as_ref().map(|n| {
        (
            steam::Launch {
                appid: 0,
                name: n.clone(),
                unlocked: 12,
                total: 45,
            },
            Instant::now(),
        )
    });

    let mut readings = sensors.sample();
    let mut ctx = sensors::dashboard_context(&readings, s.fps_bar_max);
    let mut showing = Showing::Dashboard;
    let mut scene_start = Instant::now();
    let mut last_poll = Instant::now();

    loop {
        let frame_start = Instant::now();

        if last_poll.elapsed() >= poll {
            let now = Instant::now();
            readings = sensors.sample();
            ctx = sensors::dashboard_context(&readings, s.fps_bar_max);

            if let Some(st) = &mut steam {
                if let Some(u) = st.poll().into_iter().next_back() {
                    log::info!("steam: achievement '{}' ({})", u.name, u.game);
                    achievement = Some((u, now));
                }
                if launch_scene.is_some() {
                    if let Some(l) = st.take_launch() {
                        log::info!("steam: launch '{}'", l.name);
                        if let Some((_, engine)) = launch_scene.as_mut() {
                            let cover = st
                                .cover_path(l.appid)
                                .unwrap_or_else(|| default_cover.clone());
                            if let Err(e) = engine.set_image(COVER_KEY, &cover) {
                                log::warn!("launch cover '{}': {e:#}", cover.display());
                            }
                        }
                        launch = Some((l, now));
                    }
                }
                if let Some(g) = st.current_game() {
                    ctx.set("game.name", g.name.clone());
                    ctx.set("game.appid", g.appid.to_string());
                }
            }
            let launch_live = launch
                .as_ref()
                .is_some_and(|(_, t)| now.duration_since(*t).as_secs_f32() < launch_secs);
            let ach_live = achievement
                .as_ref()
                .is_some_and(|(_, t)| now.duration_since(*t).as_secs_f32() < ach_secs);

            let hw = alert_scene
                .as_ref()
                .and_then(|_| alerts.evaluate(&readings, now));
            let want = match (hw, launch_live, ach_live) {
                (Some(a), _, _) => Showing::Alert(a),
                (None, true, _) => Showing::Launch,
                (None, false, true) => Showing::Achievement,
                (None, false, false) => Showing::Dashboard,
            };
            if want != showing {
                log::info!("panel -> {want:?}");
                showing = want;
                scene_start = now;
                match showing {
                    Showing::Alert(_) => alert_scene.as_mut().unwrap().1.reset(),
                    Showing::Launch => launch_scene.as_mut().unwrap().1.reset(),
                    Showing::Achievement => notify_scene.as_mut().unwrap().1.reset(),
                    Showing::Dashboard => dash_engine.reset(),
                }
            }
            last_poll = now;
        }

        let (frame, dirty) = match showing {
            Showing::Alert(a) => {
                let (scene, engine) = alert_scene.as_mut().unwrap();
                let mut actx = ctx.clone();
                alerts::overlay(a, &readings, &cfg.alerts, &mut actx);
                engine.render(scene, &actx, scene_start.elapsed().as_secs_f32())
            }
            Showing::Launch => {
                let (scene, engine) = launch_scene.as_mut().unwrap();
                let mut actx = ctx.clone();
                if let Some((l, _)) = &launch {
                    launch_overlay(l, cfg.steam.launch_stat, &mut actx);
                }
                engine.render(scene, &actx, scene_start.elapsed().as_secs_f32())
            }
            Showing::Achievement => {
                let (scene, engine) = notify_scene.as_mut().unwrap();
                let mut actx = ctx.clone();
                if let Some((u, _)) = &achievement {
                    actx.set("achievement.name", u.name.clone());
                    actx.set("game.name", u.game.clone());
                    actx.set(
                        "achievement.progress",
                        format!("{} / {} SUCCES", u.unlocked, u.total),
                    );
                    actx.set(
                        "achievement.frac",
                        format!("{:.3}", u.unlocked as f32 / u.total.max(1) as f32),
                    );
                }
                engine.render(scene, &actx, scene_start.elapsed().as_secs_f32())
            }
            Showing::Dashboard => dash_engine.render(&dash_scene, &ctx, 0.0),
        };
        backend.present(&frame, &dirty)?;
        if backend.should_close() {
            return Ok(());
        }
        if let Some(rem) = budget.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(rem);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Showing {
    Dashboard,
    Alert(alerts::Alert),
    Launch,
    Achievement,
}

/// Fill the launch card's `{{ game.* }}` slots for the selected stat.
fn launch_overlay(l: &steam::Launch, stat: config::LaunchStat, ctx: &mut Context) {
    ctx.set("game.name", l.name.clone());
    let (subtitle, frac) = match stat {
        config::LaunchStat::Achievements if l.total > 0 => (
            format!("{} / {} SUCCES", l.unlocked, l.total),
            l.unlocked as f32 / l.total as f32,
        ),
        _ => ("STEAM".to_string(), 0.0),
    };
    ctx.set("game.subtitle", subtitle);
    ctx.set("game.stat_frac", format!("{frac:.3}"));
}

/// `--mode single`: the original one-scene player (capture / loop / end-at / hold).
fn play_single(
    backend: &mut dyn DisplayBackend,
    engine: &mut SceneEngine,
    scene: &SceneFile,
    cli: &Cli,
) -> Result<()> {
    let duration = scene.scene.duration.max(f32::EPSILON);
    let budget = Duration::from_secs_f32(1.0 / cli.fps.max(1) as f32);
    let end_at = cli
        .end_at
        .map(|e| e.clamp(0.0, duration))
        .unwrap_or(duration);
    let start = Instant::now();

    log::info!(
        "playing '{}' ({:.1}s)",
        scene.scene.name,
        scene.scene.duration
    );

    let ctx_at = |t: f32| {
        let progress = (t / duration).clamp(0.0, 1.0);
        let mut ctx = Context::new();
        ctx.set("status", boot_status(progress));
        ctx.set("boot.progress", format!("{progress:.3}"));
        ctx
    };

    let mut frame_idx: u32 = 0;
    loop {
        let frame_start = Instant::now();
        let (scene_t, is_last) = if cli.capture {
            let t = frame_idx as f32 / cli.fps.max(1) as f32;
            (t.min(end_at), t >= end_at)
        } else {
            let elapsed = start.elapsed().as_secs_f32();
            if cli.loop_scene {
                (elapsed % duration, false)
            } else {
                (elapsed.min(end_at), elapsed >= end_at)
            }
        };
        frame_idx += 1;

        let (frame, dirty) = engine.render(scene, &ctx_at(scene_t), scene_t);
        backend.present(&frame, &dirty)?;
        if backend.should_close() || is_last {
            break;
        }
        if !cli.capture {
            if let Some(rem) = budget.checked_sub(frame_start.elapsed()) {
                std::thread::sleep(rem);
            }
        }
    }
    log::info!("scene finished at {end_at:.2}s");

    if cli.hold && !cli.loop_scene {
        log::info!("holding final frame (Ctrl-C to exit)");
        let (frame, _) = engine.render(scene, &ctx_at(end_at), end_at);
        while !backend.should_close() {
            backend.present(&frame, &[])?;
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_path_inserts_tag() {
        assert_eq!(
            variant_path(Path::new("assets/scenes/boot.toml"), "landscape").unwrap(),
            PathBuf::from("assets/scenes/boot.landscape.toml")
        );
    }

    #[test]
    fn orient_falls_back_when_variant_absent() {
        let base = Path::new("/definitely/missing/boot.toml");
        assert_eq!(orient_scene_path(base, Orientation::Landscape), base);
    }

    #[test]
    fn logical_size_swaps_with_orientation() {
        assert_eq!(Orientation::Portrait.logical_size(), (320, 480));
        assert_eq!(Orientation::Landscape.logical_size(), (480, 320));
    }
}
