//! Phase 1-2 daemon: load a scene, play it through a [`DisplayBackend`], done.
//!
//! Steam detection, sensors and the event engine come later; today this is
//! enough to iterate on animations (`--backend window` on a desktop) and to
//! drive the panel over USB serial (`--backend serial`, needs `--features
//! serial`).

mod config;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use clap::{Parser, ValueEnum};
use config::Config;
use renderer::{Context, SceneEngine, SceneFile};
use turzx::{DisplayBackend, Orientation};

#[derive(Parser, Debug)]
#[command(name = "bc250-dashboard", version, about)]
struct Cli {
    /// Scene file to play. `<name>.<orientation>.toml` next to it wins if present.
    #[arg(short, long, default_value = "assets/scenes/boot.toml")]
    scene: PathBuf,

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

    /// Loop the scene instead of exiting when it ends (best with `window`).
    #[arg(long = "loop")]
    loop_scene: bool,

    /// Deterministic capture: step scene time by exactly `1/fps` each frame
    /// (ignore the wall clock) and render `duration * fps` frames, then exit.
    #[arg(long)]
    capture: bool,

    /// Serial device path for the `serial` backend; auto-detected if omitted.
    #[arg(long)]
    port: Option<String>,
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    let cfg = Config::load(&cli.config).context("loading config")?;
    let orientation = cli.orientation.unwrap_or(cfg.panel.orientation);
    let (width, height) = orientation.logical_size();

    let scene_path = orient_scene_path(&cli.scene, orientation);
    let scene = SceneFile::load(&scene_path)?;
    if let Some(decl) = scene.scene.orientation.as_deref() {
        if !decl.eq_ignore_ascii_case(orientation.tag()) {
            log::warn!(
                "scene '{}' declares orientation '{decl}' but running as '{orientation}'",
                scene_path.display()
            );
        }
    }

    let base = scene_path.parent().unwrap_or_else(|| Path::new("."));
    let mut engine = SceneEngine::new(width, height);
    engine
        .load_assets(&scene, base)
        .context("loading scene assets")?;

    let mut backend = make_backend(&cli, (width, height), orientation)?;
    let frame_budget = Duration::from_secs_f32(1.0 / cli.fps.max(1) as f32);
    let duration = scene.scene.duration.max(f32::EPSILON);
    let start = Instant::now();

    log::info!(
        "playing '{}' ({:.1}s) as {orientation} {width}x{height} via {:?} backend",
        scene.scene.name,
        scene.scene.duration,
        cli.backend
    );

    let mut frame_idx: u32 = 0;
    loop {
        let frame_start = Instant::now();
        let scene_t = if cli.capture {
            frame_idx as f32 / cli.fps.max(1) as f32
        } else {
            let elapsed = start.elapsed().as_secs_f32();
            if cli.loop_scene {
                elapsed % duration
            } else {
                elapsed
            }
        };
        frame_idx += 1;
        if !cli.loop_scene && scene_t > duration {
            break;
        }
        let progress = (scene_t / duration).clamp(0.0, 1.0);

        let mut ctx = Context::new();
        ctx.set("status", boot_status(progress));
        ctx.set("boot.progress", format!("{progress:.3}"));

        let (frame, dirty) = engine.render(&scene, &ctx, scene_t);
        backend.present(&frame, &dirty)?;
        if backend.should_close() {
            break;
        }

        if !cli.capture {
            if let Some(rem) = frame_budget.checked_sub(frame_start.elapsed()) {
                std::thread::sleep(rem);
            }
        }
    }

    log::info!("scene finished");
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
