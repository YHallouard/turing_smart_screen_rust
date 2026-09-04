//! `bc250-dashboard config …` — read and change the daemon config file without
//! hand-editing TOML. Edits are format-preserving (`toml_edit`) and validated
//! against [`crate::config::Config`] before they are written.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use toml_edit::{value, DocumentMut, Item};

use crate::config::Config;

#[derive(Subcommand, Debug)]
pub enum Action {
    /// Print the config file path.
    Path,
    /// Show every setting and its effective value.
    Show,
    /// Print one value, e.g. `panel.orientation`.
    Get { key: String },
    /// Set one value (e.g. `sensors.gpu nvidia`); restarts the service to apply.
    Set {
        key: String,
        value: String,
        /// Don't restart bc250-dashboard.service afterwards.
        #[arg(long = "no-restart")]
        no_restart: bool,
    },
    /// Open the config file in `$EDITOR`; restarts the service to apply.
    Edit {
        /// Don't restart bc250-dashboard.service afterwards.
        #[arg(long = "no-restart")]
        no_restart: bool,
    },
}

#[derive(Clone, Copy)]
enum Kind {
    Str,
    Bool,
    Int,
    Float,
}

/// The settable keys, `"table.field"` → value kind.
const KEYS: &[(&str, Kind)] = &[
    ("panel.orientation", Kind::Str),
    ("sensors.gpu", Kind::Str),
    ("sensors.manage_mangohud", Kind::Bool),
    ("sensors.mangohud_log_dir", Kind::Str),
    ("sensors.fps_stale_secs", Kind::Float),
    ("sensors.fps_bar_max", Kind::Float),
    ("sensors.mangohud_prune_hours", Kind::Float),
    ("alerts.enabled", Kind::Bool),
    ("alerts.gpu_temp_c", Kind::Int),
    ("alerts.gpu_temp_clear_c", Kind::Int),
    ("alerts.vram_frac", Kind::Float),
    ("alerts.vram_frac_clear", Kind::Float),
    ("alerts.fan_min_rpm", Kind::Int),
    ("alerts.fan_clear_rpm", Kind::Int),
    ("alerts.min_secs", Kind::Float),
    ("steam.enabled", Kind::Bool),
    ("steam.achievement_secs", Kind::Float),
    ("steam.launch_animation", Kind::Bool),
    ("steam.launch_stat", Kind::Str),
]; // keep in sync with config.rs

/// `$XDG_CONFIG_HOME/bc250-dashboard/config.toml` (or `~/.config/…`).
pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("bc250-dashboard").join("config.toml"))
}

pub fn run(file: Option<PathBuf>, action: Action) -> Result<()> {
    let path = file
        .or_else(default_path)
        .context("no $HOME / $XDG_CONFIG_HOME to locate the config file")?;

    match action {
        Action::Path => println!("{}", path.display()),

        Action::Show => {
            let cfg = Config::load(&path).context("loading config")?;
            let exists = path.exists();
            println!(
                "# {} {}",
                path.display(),
                if exists {
                    ""
                } else {
                    "(missing — showing defaults)"
                }
            );
            for (key, _) in KEYS {
                println!("{key} = {}", effective(&cfg, key).unwrap_or_default());
            }
        }

        Action::Get { key } => {
            check_key(&key)?;
            let cfg = Config::load(&path).context("loading config")?;
            println!("{}", effective(&cfg, &key).unwrap_or_default());
        }

        Action::Set {
            key,
            value: val,
            no_restart,
        } => {
            let kind = check_key(&key)?;
            let (table, field) = key.split_once('.').unwrap();

            let mut doc = read_doc(&path)?;
            let item = doc
                .entry(table)
                .or_insert_with(|| Item::Table(Default::default()));
            let tbl = item
                .as_table_like_mut()
                .with_context(|| format!("`{table}` in {} is not a table", path.display()))?;
            tbl.insert(field, typed(kind, &val)?);

            // Validate before writing: a bad value must not reach the daemon.
            let text = doc.to_string();
            toml::from_str::<Config>(&text)
                .with_context(|| format!("`{key} = {val}` would make an invalid config"))?;

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
            println!("{key} = {val}  ->  {}", path.display());
            apply(no_restart);
        }

        Action::Edit { no_restart } => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            let status = Command::new(&editor)
                .arg(&path)
                .status()
                .with_context(|| format!("launching $EDITOR ({editor})"))?;
            if !status.success() {
                bail!("{editor} exited with {status}");
            }
            // Surface a syntax / schema error right away.
            Config::load(&path).context("the edited config does not parse")?;
            apply(no_restart);
        }
    }
    Ok(())
}

/// Restart `bc250-dashboard.service` (user scope) so the change takes effect,
/// unless told not to or it isn't running.
fn apply(no_restart: bool) {
    const UNIT: &str = "bc250-dashboard.service";
    let hint = || println!("apply with:  systemctl --user restart bc250-dashboard");

    if no_restart {
        hint();
        return;
    }
    let active = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", UNIT])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !active {
        println!("({UNIT} not running under `systemctl --user` — nothing to restart)");
        return;
    }
    match Command::new("systemctl")
        .args(["--user", "restart", UNIT])
        .status()
    {
        Ok(s) if s.success() => println!("restarted {UNIT}"),
        _ => hint(),
    }
}

fn check_key(key: &str) -> Result<Kind> {
    KEYS.iter()
        .find(|(k, _)| *k == key)
        .map(|(_, kind)| *kind)
        .with_context(|| {
            let known: Vec<_> = KEYS.iter().map(|(k, _)| *k).collect();
            format!("unknown key `{key}`. known keys:\n  {}", known.join("\n  "))
        })
}

fn typed(kind: Kind, raw: &str) -> Result<Item> {
    Ok(match kind {
        Kind::Str => value(raw),
        Kind::Bool => value(
            raw.parse::<bool>()
                .with_context(|| format!("`{raw}` is not true/false"))?,
        ),
        Kind::Int => value(
            raw.parse::<i64>()
                .with_context(|| format!("`{raw}` is not a whole number"))?,
        ),
        Kind::Float => value(
            raw.parse::<f64>()
                .with_context(|| format!("`{raw}` is not a number"))?,
        ),
    })
}

fn read_doc(path: &std::path::Path) -> Result<DocumentMut> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<DocumentMut>()
            .with_context(|| format!("{} is not valid TOML", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn effective(cfg: &Config, key: &str) -> Option<String> {
    let s = &cfg.sensors;
    let a = &cfg.alerts;
    Some(match key {
        "panel.orientation" => cfg.panel.orientation.to_string(),
        "sensors.gpu" => format!("{:?}", s.gpu).to_lowercase(),
        "sensors.manage_mangohud" => s.manage_mangohud.to_string(),
        "sensors.mangohud_log_dir" => s
            .mangohud_log_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        "sensors.fps_stale_secs" => s.fps_stale_secs.to_string(),
        "sensors.fps_bar_max" => s.fps_bar_max.to_string(),
        "sensors.mangohud_prune_hours" => s.mangohud_prune_hours.to_string(),
        "alerts.enabled" => a.enabled.to_string(),
        "alerts.gpu_temp_c" => a.gpu_temp_c.to_string(),
        "alerts.gpu_temp_clear_c" => a.gpu_temp_clear_c.to_string(),
        "alerts.vram_frac" => a.vram_frac.to_string(),
        "alerts.vram_frac_clear" => a.vram_frac_clear.to_string(),
        "alerts.fan_min_rpm" => a.fan_min_rpm.to_string(),
        "alerts.fan_clear_rpm" => a.fan_clear_rpm.to_string(),
        "alerts.min_secs" => a.min_secs.to_string(),
        "steam.enabled" => cfg.steam.enabled.to_string(),
        "steam.achievement_secs" => cfg.steam.achievement_secs.to_string(),
        "steam.launch_animation" => cfg.steam.launch_animation.to_string(),
        "steam.launch_stat" => format!("{:?}", cfg.steam.launch_stat).to_lowercase(),
        _ => return None,
    })
}
