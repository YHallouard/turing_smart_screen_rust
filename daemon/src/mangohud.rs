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
    if apply_block(&conf, log_dir, true)? {
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

/// Game Mode runs `mangoapp`, which reads a *temporary* config named by its own
/// `MANGOHUD_CONFIGFILE` (Steam rewrites it when the overlay level changes), not
/// `~/.config/MangoHud/MangoHud.conf`. Put our block there too. Cheap enough to
/// call every few seconds; returns the files it actually changed.
pub fn ensure_logging_mangoapp(log_dir: &Path) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    for conf in mangoapp_configs() {
        match apply_block(&conf, log_dir, false) {
            Ok(true) => {
                log::info!("mangoapp config: FPS logging on -> {}", conf.display());
                changed.push(conf);
            }
            Ok(false) => {}
            Err(e) => log::warn!("mangoapp config {}: {e:#}", conf.display()),
        }
    }
    changed
}

/// `MANGOHUD_CONFIGFILE` of every `mangoapp` process we own.
fn mangoapp_configs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(procs) = fs::read_dir("/proc") else {
        return out;
    };
    for e in procs.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = e.path();
        if !fs::read_to_string(dir.join("comm")).is_ok_and(|c| c.trim() == "mangoapp") {
            continue;
        }
        let Ok(env) = fs::read(dir.join("environ")) else {
            continue; // not ours / not readable
        };
        for var in env.split(|b| *b == 0) {
            if let Some(v) = var.strip_prefix(b"MANGOHUD_CONFIGFILE=") {
                let p = PathBuf::from(String::from_utf8_lossy(v).into_owned());
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Rewrite the managed block in `conf`. `atomic` = temp file + rename (a normal
/// config); otherwise write in place (a file another program holds by path).
/// Returns whether the file changed.
fn apply_block(conf: &Path, log_dir: &Path, atomic: bool) -> Result<bool> {
    let current = fs::read_to_string(conf).unwrap_or_default();

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

    if out == current {
        return Ok(false);
    }
    if atomic {
        // A concurrent MangoHud read never sees a half-written config.
        let tmp = conf.with_file_name("MangoHud.conf.bc250.tmp");
        fs::write(&tmp, out.as_bytes())?;
        fs::rename(&tmp, conf)?;
    } else {
        fs::write(conf, out.as_bytes())?;
    }
    Ok(true)
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

    /// A process whose comm is `mangoapp` and whose env names a config file:
    /// our block must land in that file, and only once.
    #[cfg(target_os = "linux")]
    #[test]
    fn patches_mangoapp_config_found_via_proc() {
        let dir = std::env::temp_dir().join(format!("bc250-mango-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("mangoapp");
        fs::copy("/bin/sleep", &fake).unwrap();
        let conf = dir.join("mangohud.TEST");
        fs::write(&conf, "no_display\n").unwrap();

        let mut child = std::process::Command::new(&fake)
            .arg("30")
            .env("MANGOHUD_CONFIGFILE", &conf)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));

        let logs = dir.join("logs");
        let first = ensure_logging_mangoapp(&logs);
        let second = ensure_logging_mangoapp(&logs);
        let text = fs::read_to_string(&conf).unwrap();
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(first, vec![conf]);
        assert!(second.is_empty(), "second pass must be a no-op");
        assert!(text.starts_with("no_display\n"));
        assert!(text.contains("autostart_log=1"));
        assert_eq!(text.matches(BEGIN).count(), 1);
    }

    #[test]
    fn strip_block_noop_without_block() {
        assert_eq!(strip_block("fps\nvram\n"), "fps\nvram\n");
    }
}
