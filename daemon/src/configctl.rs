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
    /// Machine-readable dump for UIs: every setting with its type, label, bounds,
    /// default and current value, as one JSON object (used by the Decky plugin).
    Json,
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

/// One settable key plus the metadata a UI needs to render it.
struct Setting {
    key: &'static str,
    kind: Kind,
    group: &'static str,
    label: &'static str,
    help: &'static str,
    /// Allowed values for enum-like strings (empty = free text / number).
    choices: &'static [&'static str],
    /// `(min, max, step)` for numbers.
    range: Option<(f64, f64, f64)>,
}

const fn s(
    key: &'static str,
    kind: Kind,
    group: &'static str,
    label: &'static str,
    help: &'static str,
) -> Setting {
    Setting {
        key,
        kind,
        group,
        label,
        help,
        choices: &[],
        range: None,
    }
}

/// The settable keys. Keep in sync with config.rs.
const SETTINGS: &[Setting] = &[
    Setting {
        choices: &["portrait", "landscape"],
        ..s("panel.orientation", Kind::Str, "Écran", "Orientation", "Montage physique de l'écran (pas de capteur).")
    },
    Setting {
        choices: &["auto", "amd", "nvidia"],
        ..s("sensors.gpu", Kind::Str, "Capteurs", "Source GPU", "auto = nvidia-smi si présent, sinon AMD.")
    },
    s("sensors.manage_mangohud", Kind::Bool, "Capteurs", "Gérer MangoHud", "Écrit le log FPS dans MangoHud.conf."),
    s("sensors.mangohud_log_dir", Kind::Str, "Capteurs", "Dossier des logs FPS", "Vide = défaut."),
    Setting {
        range: Some((1.0, 60.0, 1.0)),
        ..s("sensors.fps_stale_secs", Kind::Float, "Capteurs", "FPS périmé après (s)", "Au-delà : « pas de jeu ».")
    },
    Setting {
        range: Some((15.0, 240.0, 5.0)),
        ..s("sensors.fps_bar_max", Kind::Float, "Capteurs", "FPS barre pleine", "FPS qui remplit la barre.")
    },
    Setting {
        range: Some((0.0, 30.0, 1.0)),
        ..s("sensors.fps_avg_secs", Kind::Float, "Capteurs", "Moyenne FPS (s)", "Fenêtre de la moyenne glissante. 0 = dernière mesure.")
    },
    Setting {
        range: Some((0.0, 168.0, 1.0)),
        ..s("sensors.mangohud_prune_hours", Kind::Float, "Capteurs", "Purge des logs (h)", "0 = garder.")
    },
    s("alerts.enabled", Kind::Bool, "Alertes", "Alertes activées", "Écran plein cadre si seuil dépassé."),
    Setting {
        range: Some((0.0, 110.0, 1.0)),
        ..s("alerts.gpu_temp_c", Kind::Int, "Alertes", "Température GPU (°C)", "Déclenchement. 0 = désactivé.")
    },
    Setting {
        range: Some((0.0, 110.0, 1.0)),
        ..s("alerts.gpu_temp_clear_c", Kind::Int, "Alertes", "Température GPU, retour (°C)", "Seuil de fin d'alerte.")
    },
    Setting {
        range: Some((0.0, 1.0, 0.01)),
        ..s("alerts.vram_frac", Kind::Float, "Alertes", "VRAM utilisée", "Fraction 0–1. 0 = désactivé.")
    },
    Setting {
        range: Some((0.0, 1.0, 0.01)),
        ..s("alerts.vram_frac_clear", Kind::Float, "Alertes", "VRAM, retour", "Seuil de fin d'alerte.")
    },
    Setting {
        range: Some((0.0, 5000.0, 50.0)),
        ..s("alerts.fan_min_rpm", Kind::Int, "Alertes", "Ventilo minimum (tr/min)", "0 = désactivé.")
    },
    Setting {
        range: Some((0.0, 5000.0, 50.0)),
        ..s("alerts.fan_clear_rpm", Kind::Int, "Alertes", "Ventilo, retour (tr/min)", "Seuil de fin d'alerte.")
    },
    Setting {
        range: Some((0.0, 30.0, 0.5)),
        ..s("alerts.min_secs", Kind::Float, "Alertes", "Durée minimale (s)", "Temps minimum d'une alerte.")
    },
    s("steam.enabled", Kind::Bool, "Steam", "Intégration Steam", "Détecte le jeu en cours."),
    Setting {
        range: Some((1.0, 30.0, 0.5)),
        ..s("steam.achievement_secs", Kind::Float, "Steam", "Durée du succès (s)", "Affichage du popup de succès.")
    },
    s("steam.launch_animation", Kind::Bool, "Steam", "Animation de lancement", "Carte affichée au lancement d'un jeu."),
    Setting {
        choices: &["achievements", "none"],
        ..s("steam.launch_stat", Kind::Str, "Steam", "Stat au lancement", "Stat affichée sur la carte.")
    },
];

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
            for st in SETTINGS {
                println!("{} = {}", st.key, effective(&cfg, st.key).unwrap_or_default());
            }
        }

        Action::Json => {
            let cfg = Config::load(&path).context("loading config")?;
            println!("{}", to_json(&path, &cfg));
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
    SETTINGS
        .iter()
        .find(|st| st.key == key)
        .map(|st| st.kind)
        .with_context(|| {
            let known: Vec<_> = SETTINGS.iter().map(|st| st.key).collect();
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
        "sensors.fps_avg_secs" => s.fps_avg_secs.to_string(),
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

fn json_str(raw: &str) -> String {
    let mut out = String::from("\"");
    for c in raw.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A setting value as a JSON scalar of its declared kind.
fn json_value(kind: Kind, raw: &str) -> String {
    match kind {
        Kind::Str => json_str(raw),
        Kind::Bool | Kind::Int | Kind::Float if raw.is_empty() => "null".into(),
        _ => raw.to_string(),
    }
}

fn to_json(path: &std::path::Path, cfg: &Config) -> String {
    let defaults = Config::default();
    let mut items = Vec::new();
    for st in SETTINGS {
        let kind = match st.kind {
            Kind::Str => "string",
            Kind::Bool => "bool",
            Kind::Int => "int",
            Kind::Float => "float",
        };
        let mut o = format!(
            "{{\"key\":{},\"type\":\"{kind}\",\"group\":{},\"label\":{},\"help\":{},\"value\":{},\"default\":{}",
            json_str(st.key),
            json_str(st.group),
            json_str(st.label),
            json_str(st.help),
            json_value(st.kind, &effective(cfg, st.key).unwrap_or_default()),
            json_value(st.kind, &effective(&defaults, st.key).unwrap_or_default()),
        );
        if !st.choices.is_empty() {
            let c: Vec<_> = st.choices.iter().map(|c| json_str(c)).collect();
            o.push_str(&format!(",\"choices\":[{}]", c.join(",")));
        }
        if let Some((min, max, step)) = st.range {
            o.push_str(&format!(",\"min\":{min},\"max\":{max},\"step\":{step}"));
        }
        o.push('}');
        items.push(o);
    }
    format!(
        "{{\"path\":{},\"exists\":{},\"settings\":[{}]}}",
        json_str(&path.display().to_string()),
        path.exists(),
        items.join(",")
    )
}
