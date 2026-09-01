//! Phase 1-2 daemon: load a scene, play it through a [`DisplayBackend`], done.
//!
//! Steam detection, sensors and the event engine come later; today this is
//! enough to iterate on animations (`--backend window` on a desktop) and to
//! drive the panel over USB serial (`--backend serial`, needs `--features
//! serial`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context as _, Result};
use clap::{Parser, ValueEnum};
use renderer::{Context, SceneEngine, SceneFile};
use turzx::{DisplayBackend, PANEL_HEIGHT, PANEL_WIDTH};

#[derive(Parser, Debug)]
#[command(name = "bc250-dashboard", version, about)]
struct Cli {
    /// Scene file to play.
    #[arg(short, long, default_value = "assets/scenes/boot.toml")]
    scene: PathBuf,

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

    /// Serial device path for the `serial` backend; auto-detected if omitted.
    #[arg(long)]
    port: Option<String>,
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

fn make_backend(cli: &Cli) -> Result<Box<dyn DisplayBackend>> {
    let size = (PANEL_WIDTH, PANEL_HEIGHT);
    match cli.backend {
        BackendKind::Png => Ok(Box::new(preview::PngBackend::new(&cli.out, size)?)),
        BackendKind::Window => {
            #[cfg(feature = "window")]
            {
                Ok(Box::new(preview::WindowBackend::new(
                    "TURZX 3.5\" preview",
                    size,
                    cli.scale,
                )?))
            }
            #[cfg(not(feature = "window"))]
            {
                let _ = cli;
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
                Ok(Box::new(dev))
            }
            #[cfg(not(feature = "serial"))]
            {
                let _ = cli;
                bail!("the serial backend needs a build with `--features serial`");
            }
        }
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

    let scene = SceneFile::load(&cli.scene)?;
    let base = cli.scene.parent().unwrap_or_else(|| Path::new("."));
    let mut engine = SceneEngine::new(PANEL_WIDTH, PANEL_HEIGHT);
    engine
        .load_assets(&scene, base)
        .context("loading scene assets")?;

    let mut backend = make_backend(&cli)?;
    let frame_budget = Duration::from_secs_f32(1.0 / cli.fps.max(1) as f32);
    let duration = scene.scene.duration.max(f32::EPSILON);
    let start = Instant::now();

    log::info!(
        "playing scene '{}' ({:.1}s) via {:?} backend",
        scene.scene.name,
        scene.scene.duration,
        cli.backend
    );

    loop {
        let frame_start = Instant::now();
        let elapsed = start.elapsed().as_secs_f32();
        if !cli.loop_scene && elapsed > duration {
            break;
        }
        let scene_t = if cli.loop_scene {
            elapsed % duration
        } else {
            elapsed
        };
        let progress = (scene_t / duration).clamp(0.0, 1.0);

        let mut ctx = Context::new();
        ctx.set("status", boot_status(progress));
        ctx.set("boot.progress", format!("{progress:.3}"));

        let (frame, dirty) = engine.render(&scene, &ctx, scene_t);
        backend.present(&frame, &dirty)?;
        if backend.should_close() {
            break;
        }

        if let Some(rem) = frame_budget.checked_sub(frame_start.elapsed()) {
            std::thread::sleep(rem);
        }
    }

    log::info!("scene finished");
    Ok(())
}
