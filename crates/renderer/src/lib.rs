//! Data-driven render engine.
//!
//! A [`SceneFile`] (TOML) describes layers — text, rectangles, images, progress
//! bars, vector paths, circles, scanlines — each with optional multi-keyframe
//! [`anim`]ations. [`SceneEngine::render`] resolves a scene against a [`Context`]
//! of `{{ variables }}` at a given time and returns a [`turzx::Frame`] plus the
//! dirty regions that changed since the previous call.
//!
//! Rasterisation is `tiny_skia` (AA paths, gradients, transforms, additive
//! blend); text is `fontdue` over the vendored OFL fonts, with a 5x7 bitmap
//! fallback. Nothing here knows about Steam, sensors or specific games.

pub mod anim;
pub mod context;
pub mod font;
pub mod framebuffer;
pub mod paint;
pub mod path;
pub mod scene;

mod engine;

pub use context::Context;
pub use engine::SceneEngine;
pub use scene::SceneFile;

/// The Horus / Wedjat glyph outline (`d`), referenced from scenes as
/// `d = "@horus_eye"`. Same vector as `assets/logo/horus-eye.svg`.
pub const HORUS_EYE_PATH: &str = include_str!("../../../assets/logo/horus-eye.d");
