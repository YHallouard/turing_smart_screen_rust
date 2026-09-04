//! `config.toml` — static configuration read once at startup.
//!
//! ```toml
//! [panel]
//! orientation = "portrait"   # or "landscape"
//!
//! [sensors]
//! gpu = "auto"              # "auto" | "amd" (sysfs) | "nvidia" (nvidia-smi)
//! manage_mangohud = true    # write a managed block into MangoHud.conf so
//!                           # in-game FPS is logged (autostart_log/log_interval)
//! # Where MangoHud CSVs live. When `manage_mangohud`, this is also the
//! # `output_folder` we set. Default: $XDG_DATA_HOME/bc250-dashboard/fps-logs.
//! # mangohud_log_dir = "/home/you/.local/share/bc250-dashboard/fps-logs"
//! fps_stale_secs       = 10.0   # ignore FPS logs older than this (no game)
//! fps_bar_max          = 60.0   # FPS that fills the dashboard bar
//! mangohud_prune_hours = 24.0   # delete MangoHud CSVs older than this (0 = keep)
//! ```
//!
//! There is no orientation sensor on the panel, so changing orientation means
//! editing this file and restarting the daemon.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use turzx::Orientation;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub panel: Panel,
    pub sensors: Sensors,
    pub alerts: Alerts,
    pub steam: Steam,
}

/// Local Steam integration: detect the running game, play a launch card when it
/// starts, and pop `notify.achievement` on an unlock. Pure disk reads — no
/// Steamworks link, no Web API key.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Steam {
    pub enabled: bool,
    /// How long the achievement popup stays up, in seconds.
    pub achievement_secs: f32,
    /// Play the `launch.<orientation>.toml` card when a game starts.
    pub launch_animation: bool,
    /// The single stat shown on the launch card.
    pub launch_stat: LaunchStat,
}

impl Default for Steam {
    fn default() -> Self {
        Self {
            enabled: true,
            achievement_secs: 7.9,
            launch_animation: true,
            launch_stat: LaunchStat::Achievements,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchStat {
    /// `X / Y SUCCES` for the game.
    #[default]
    Achievements,
    /// Just "STEAM".
    None,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panel {
    pub orientation: Orientation,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sensors {
    /// Which GPU the dashboard reads.
    pub gpu: GpuSel,
    /// Write a managed block into `MangoHud.conf` to turn on FPS logging.
    pub manage_mangohud: bool,
    /// MangoHud CSV folder. Read location; also the `output_folder` we set when
    /// `manage_mangohud`. `None` → a default under `$XDG_DATA_HOME` when
    /// managing, else auto-read from `MangoHud.conf`.
    pub mangohud_log_dir: Option<PathBuf>,
    /// FPS logs older than this are treated as "no game running".
    pub fps_stale_secs: f32,
    /// FPS value that fills the dashboard's FPS bar.
    pub fps_bar_max: f32,
    /// Delete MangoHud CSVs in the log folder older than this many hours
    /// (keeping the newest). `0` disables pruning.
    pub mangohud_prune_hours: f32,
}

impl Default for Sensors {
    fn default() -> Self {
        Self {
            gpu: GpuSel::Auto,
            manage_mangohud: true,
            mangohud_log_dir: None,
            fps_stale_secs: 10.0,
            fps_bar_max: 60.0,
            mangohud_prune_hours: 24.0,
        }
    }
}

impl Sensors {
    /// Default MangoHud CSV folder: `$XDG_DATA_HOME/bc250-dashboard/fps-logs`
    /// (or `~/.local/share/...`).
    pub fn default_log_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("bc250-dashboard").join("fps-logs"))
    }
}

/// Hardware alerts: when a reading crosses `*_c` / `*_frac` it takes over the
/// panel with the `alert.<orientation>.toml` scene until the reading drops back
/// past the matching `*_clear` threshold (hysteresis) and `min_secs` has
/// elapsed. Precedence, highest first: fan-stopped, GPU temp, VRAM.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Alerts {
    pub enabled: bool,
    /// GPU edge temperature, °C. `0` disables this alert.
    pub gpu_temp_c: i32,
    pub gpu_temp_clear_c: i32,
    /// VRAM used / total, `0.0..=1.0`. `0` disables.
    pub vram_frac: f32,
    pub vram_frac_clear: f32,
    /// Lowest fan tach, RPM — fires when a fan is below this. `0` disables.
    pub fan_min_rpm: u32,
    pub fan_clear_rpm: u32,
    /// Minimum time an alert stays up before the dashboard can return (a
    /// higher-precedence alert still preempts immediately).
    pub min_secs: f32,
}

impl Default for Alerts {
    fn default() -> Self {
        Self {
            enabled: true,
            gpu_temp_c: 85,
            gpu_temp_clear_c: 80,
            vram_frac: 0.95,
            vram_frac_clear: 0.90,
            fan_min_rpm: 400,
            fan_clear_rpm: 600,
            min_secs: 4.0,
        }
    }
}

/// GPU data source for the dashboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuSel {
    /// `nvidia-smi` if it works, else AMD sysfs.
    #[default]
    Auto,
    /// AMD render node `gpu_busy_percent` / `mem_info_vram_*` / amdgpu hwmon.
    Amd,
    /// `nvidia-smi --query-gpu`.
    Nvidia,
}

impl Config {
    /// Load `path`; a missing file yields defaults (portrait).
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }
}
