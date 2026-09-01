//! Data-driven render engine.
//!
//! A [`SceneFile`] (TOML) describes layers — text, rectangles, images, progress
//! bars — each with optional keyframe [`anim`]ations. [`SceneEngine::render`]
//! resolves a scene against a [`Context`] of `{{ variables }}` at a given time
//! and returns a [`turzx::Frame`] plus the dirty regions that changed since the
//! previous call.
//!
//! Nothing here knows about Steam, sensors or specific games: a scene is handed
//! a context and renders whatever is in it.

pub mod anim;
pub mod context;
pub mod font;
pub mod framebuffer;
pub mod scene;

mod engine;

pub use context::Context;
pub use engine::SceneEngine;
pub use scene::SceneFile;
