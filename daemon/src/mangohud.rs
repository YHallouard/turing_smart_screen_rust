//! Best-effort management of `MangoHud.conf` so in-game FPS logging is on
//! without the user hand-editing it.
//!
//! We own a delimited block at the end of the file and rewrite only that block;
//! everything the user (or GOverlay) put above is left untouched. MangoHud takes
//! the last value for a duplicate key, so our block wins regardless of what is
//! above it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const BEGIN: &str = "# >>> bc250-dashboard (managed) >>>";
const END: &str = "# <<< bc250-dashboard (managed) <<<";

/// Ensure MangoHud logs a CSV row every 2 s into `log_dir` while a game runs.
/// Returns the config path. Creates `log_dir` and the config's parent dir.
pub fn ensure_logging(log_dir: &Path) -> Result<PathBuf> {
    let conf = config_path().context("no HOME/XDG_CONFIG_HOME for MangoHud config")?;
    if let Some(parent) = conf.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(log_dir)?;

    let current = fs::read_to_string(&conf).unwrap_or_default();

    let mut out = strip_block(&current).trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(BEGIN);
    out.push_str("\nautostart_log=1\nlog_interval=2000\noutput_folder=");
    out.push_str(&log_dir.display().to_string());
    out.push('\n');
    out.push_str(END);
    out.push('\n');

    if out != current {
        // Write via a temp file + rename so a concurrent MangoHud read never
        // sees a half-written config.
        let tmp = conf.with_file_name("MangoHud.conf.bc250.tmp");
        fs::write(&tmp, out.as_bytes())?;
        fs::rename(&tmp, &conf)?;
        log::info!(
            "MangoHud config: FPS logging on -> {} ({})",
            log_dir.display(),
            conf.display()
        );
    } else {
        log::debug!("MangoHud config already current ({})", conf.display());
    }
    Ok(conf)
}

fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MANGOHUD_CONFIGFILE") {
        return Some(PathBuf::from(p));
    }
    let cfg_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(cfg_home.join("MangoHud").join("MangoHud.conf"))
}

/// Everything outside our `BEGIN..END` block, newline-terminated.
fn strip_block(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_block = false;
    for line in s.lines() {
        match line.trim() {
            BEGIN => in_block = true,
            END => in_block = false,
            _ if !in_block => {
                out.push_str(line);
                out.push('\n');
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_block_removes_only_managed_lines() {
        let src = "fps\nvram\n# >>> bc250-dashboard (managed) >>>\nautostart_log=1\n# <<< bc250-dashboard (managed) <<<\nposition=top-left\n";
        assert_eq!(strip_block(src), "fps\nvram\nposition=top-left\n");
    }

    #[test]
    fn strip_block_noop_without_block() {
        assert_eq!(strip_block("fps\nvram\n"), "fps\nvram\n");
    }
}
