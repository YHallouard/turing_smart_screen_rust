//! Scene description: the TOML schema the engine interprets.
//!
//! ```toml
//! [scene]
//! name = "game_start"
//! duration = 4.0
//! background = "#04070d"
//!
//! [[layer]]
//! type = "text"
//! value = "{{ game.name }}"
//! x = 20
//! y = 40
//! color = "#ffffff"
//! scale = 3
//! [[layer.anim]]
//! property = "opacity"
//! from = 0.0
//! to = 1.0
//! end = 1.0
//! easing = "ease_out"
//! ```

use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use crate::anim::Anim;

#[derive(Debug, Clone, Deserialize)]
pub struct SceneFile {
    pub scene: SceneMeta,
    #[serde(default, rename = "layer")]
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneMeta {
    pub name: String,
    /// Seconds. The daemon stops (or loops) the scene after this.
    pub duration: f32,
    #[serde(default = "default_background")]
    pub background: String,
}

fn default_background() -> String {
    "#000000".to_string()
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    Text,
    Rect,
    Image,
    ProgressBar,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layer {
    #[serde(rename = "type")]
    pub kind: LayerKind,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    /// Text pixel multiplier; defaults to 2 for `text` layers.
    #[serde(default)]
    pub scale: Option<f32>,
    /// `text`: the string (may contain `{{ vars }}`).
    /// `progress_bar`: the 0..1 fill (may be a `{{ var }}`) unless animated.
    #[serde(default)]
    pub value: Option<String>,
    /// `image`: path relative to the scene file.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default, rename = "anim")]
    pub anims: Vec<Anim>,
}

fn default_opacity() -> f32 {
    1.0
}

impl SceneFile {
    pub fn from_toml_str(s: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(s)?)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading scene {}", path.display()))?;
        Self::from_toml_str(&text).with_context(|| format!("parsing scene {}", path.display()))
    }
}
