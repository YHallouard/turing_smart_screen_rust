//! `config.toml` — static configuration read once at startup.
//!
//! ```toml
//! [panel]
//! orientation = "portrait"   # or "landscape"
//! ```
//!
//! There is no orientation sensor on the panel, so changing orientation means
//! editing this file and restarting the daemon.

use std::path::Path;

use serde::Deserialize;
use turzx::Orientation;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub panel: Panel,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panel {
    pub orientation: Orientation,
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
