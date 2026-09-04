//! Scene description: the TOML schema the engine interprets.
//!
//! ```toml
//! [scene]
//! name = "boot"
//! duration = 10.0
//! background = "gradient:bg"
//!
//! [[gradient]]
//! name = "gold"
//! kind = "linear"
//! stops = [ { at = 0.2, color = "#eece86" }, { at = 1.0, color = "#8a6f37" } ]
//!
//! [[layer]]
//! type = "path"
//! d = "@horus_eye"
//! x = 160
//! y = 204
//! scale = 0.278
//! fill = "gradient:gold"
//! stroke = "#eece86"
//! stroke_width = 3.2
//! anchor = "center"
//! [[layer.anim]]
//! property = "trace"
//! from = 0.0
//! to = 1.0
//! start = 1.4
//! end = 4.8
//! ```

use std::path::Path;

use anyhow::Context as _;
use serde::Deserialize;

use crate::anim::Anim;
use crate::paint::Gradient;

#[derive(Debug, Clone, Deserialize)]
pub struct SceneFile {
    pub scene: SceneMeta,
    #[serde(default, rename = "gradient")]
    pub gradients: Vec<Gradient>,
    #[serde(default, rename = "layer")]
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneMeta {
    pub name: String,
    /// Seconds. The daemon stops (or loops) the scene after this.
    pub duration: f32,
    /// `"#rrggbb[aa]"` or `"gradient:<name>"`.
    #[serde(default = "default_background")]
    pub background: String,
    /// Optional `"portrait"` / `"landscape"` self-declaration. Purely a sanity
    /// hint: the daemon warns if it disagrees with the runtime orientation.
    #[serde(default)]
    pub orientation: Option<String>,
}

fn default_background() -> String {
    "#000000".to_string()
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    Text,
    Rect,
    /// Outlined rectangle (border only): `stroke`/`color` + `stroke_width`.
    StrokeRect,
    Image,
    ProgressBar,
    Path,
    Circle,
    Scanlines,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    #[default]
    TopLeft,
    Center,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    #[default]
    Normal,
    /// Additive — for the power / ignite flashes.
    Add,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Middle,
    End,
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
    /// `text`: multiplier for the 5x7 fallback font. `image`/`path`: scale
    /// factor. Defaults to 2 for bitmap text, 1 otherwise.
    #[serde(default)]
    pub scale: Option<f32>,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub trace: Option<f32>,
    #[serde(default)]
    pub anchor: Anchor,
    #[serde(default)]
    pub blend: Blend,
    #[serde(default = "default_opacity")]
    pub opacity: f32,

    /// `text`: the string. `progress_bar`: the 0..1 fill.
    #[serde(default)]
    pub value: Option<String>,
    /// `image`: path relative to the scene file. `path`: an `.svg` to read `d`
    /// from.
    #[serde(default)]
    pub source: Option<String>,
    /// `path`: inline path data, or `@horus_eye` for the built-in glyph.
    #[serde(default)]
    pub d: Option<String>,
    /// `circle`: radius.
    #[serde(default)]
    pub radius: f32,

    /// Flat `"#rrggbb"` (fallback text, rect). Kept for back-compat.
    #[serde(default)]
    pub color: Option<String>,
    /// Fill paint: `"#rrggbb[aa]"` or `"gradient:<name>"`.
    #[serde(default)]
    pub fill: Option<String>,
    /// Outline paint (`path`, `circle`).
    #[serde(default)]
    pub stroke: Option<String>,
    #[serde(default)]
    pub stroke_width: Option<f32>,
    #[serde(default)]
    pub stroke_opacity: Option<f32>,

    /// TTF text: `"cinzel"`, `"rajdhani"`, `"mono"`.
    #[serde(default)]
    pub font: Option<String>,
    /// TTF text pixel size.
    #[serde(default)]
    pub size: Option<f32>,
    /// Extra spacing between glyphs, in `em` (0.22 in the design).
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default)]
    pub align: Align,
    /// `0..1` fraction of the letters revealed (whole-run reveal).
    #[serde(default)]
    pub letter_reveal: Option<f32>,
    /// Max width in px for a `text` layer. A longer bitmap-font string is
    /// truncated with `...`. Ignored for TTF text and other layer kinds.
    #[serde(default)]
    pub max_width: Option<f32>,

    /// Glow (blur halo) strength `0..1`, and its blur radius in px.
    #[serde(default)]
    pub glow: f32,
    #[serde(default = "default_glow_radius")]
    pub glow_radius: f32,

    /// `scanlines`: row period.
    #[serde(default = "default_period")]
    pub period: i32,

    #[serde(default, rename = "anim")]
    pub anims: Vec<Anim>,
}

fn default_opacity() -> f32 {
    1.0
}
fn default_glow_radius() -> f32 {
    4.0
}
fn default_period() -> i32 {
    3
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
