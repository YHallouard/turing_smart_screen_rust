//! [`SceneEngine`]: resolve a scene against a [`Context`] at time `t`, paint a
//! frame, and report the regions that changed since the previous call.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context as _;
use turzx::{DirtyTracker, Frame, Rect};

use crate::anim::AnimProperty;
use crate::context::Context;
use crate::font;
use crate::framebuffer::{Canvas, Rgba};
use crate::scene::{Layer, LayerKind, SceneFile};

pub struct SceneEngine {
    width: u16,
    height: u16,
    /// `source` path -> (w, h, rgba8).
    images: HashMap<String, (u32, u32, Vec<u8>)>,
    prev: Vec<Resolved>,
}

/// A layer with every animated / templated field collapsed to a concrete value.
#[derive(Debug, Clone, PartialEq)]
struct Resolved {
    kind: LayerKind,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: i32,
    opacity: f32,
    text: String,
    color: Rgba,
    progress: f32,
    source: Option<String>,
}

impl SceneEngine {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            images: HashMap::new(),
            prev: Vec::new(),
        }
    }

    /// Decode every `image` layer's file once, up front.
    pub fn load_assets(&mut self, scene: &SceneFile, base: &Path) -> anyhow::Result<()> {
        for layer in &scene.layers {
            if layer.kind != LayerKind::Image {
                continue;
            }
            let Some(src) = &layer.source else { continue };
            if self.images.contains_key(src) {
                continue;
            }
            let path = base.join(src);
            let img = image::open(&path)
                .with_context(|| format!("opening image {}", path.display()))?
                .to_rgba8();
            let (w, h) = img.dimensions();
            self.images.insert(src.clone(), (w, h, img.into_raw()));
        }
        Ok(())
    }

    /// Render `scene` at time `t` seconds. Returns the frame plus the dirty
    /// regions relative to the previous `render` call (full-frame on the first).
    pub fn render(&mut self, scene: &SceneFile, ctx: &Context, t: f32) -> (Frame, Vec<Rect>) {
        let resolved: Vec<Resolved> = scene
            .layers
            .iter()
            .map(|l| self.resolve(l, ctx, t))
            .collect();

        let mut frame = Frame::new(self.width, self.height);
        let bg = Rgba::parse(&scene.scene.background).unwrap_or(Rgba::BLACK);
        {
            let mut canvas = Canvas::new(&mut frame);
            canvas.clear(bg);
            for r in &resolved {
                self.paint(&mut canvas, r);
            }
        }

        let mut dirty = DirtyTracker::new();
        if self.prev.len() != resolved.len() {
            dirty.add(Rect::new(0, 0, self.width, self.height));
        } else {
            for (a, b) in self.prev.iter().zip(&resolved) {
                if a != b {
                    dirty.add(self.clamp(bbox(a)));
                    dirty.add(self.clamp(bbox(b)));
                }
            }
        }
        self.prev = resolved;
        (frame, dirty.regions().to_vec())
    }

    fn resolve(&self, l: &Layer, ctx: &Context, t: f32) -> Resolved {
        let mut x = l.x;
        let mut y = l.y;
        let mut scale = l.scale.unwrap_or(2.0);
        let mut opacity = l.opacity;
        let mut animated_progress: Option<f32> = None;

        for a in &l.anims {
            let v = a.sample(t);
            match a.property {
                AnimProperty::X => x = v,
                AnimProperty::Y => y = v,
                AnimProperty::Scale => scale = v,
                AnimProperty::Opacity => opacity = v,
                AnimProperty::Value => animated_progress = Some(v),
            }
        }

        let raw_value = l.value.as_deref().unwrap_or("");
        let text = ctx.expand(raw_value);
        let progress = animated_progress
            .or_else(|| ctx.expand_f32(raw_value))
            .unwrap_or(0.0);
        let color = l
            .color
            .as_deref()
            .and_then(Rgba::parse)
            .unwrap_or(Rgba::WHITE);

        Resolved {
            kind: l.kind,
            x: x.round() as i32,
            y: y.round() as i32,
            w: l.width.round() as i32,
            h: l.height.round() as i32,
            scale: (scale.round() as i32).max(1),
            opacity: opacity.clamp(0.0, 1.0),
            text,
            color,
            progress,
            source: l.source.clone(),
        }
    }

    fn paint(&self, c: &mut Canvas, r: &Resolved) {
        let col = r.color.with_opacity(r.opacity);
        match r.kind {
            LayerKind::Rect => c.fill_rect(r.x, r.y, r.w, r.h, col),
            LayerKind::Text => c.draw_text(r.x, r.y, &r.text, r.scale, col),
            LayerKind::ProgressBar => c.progress_bar(r.x, r.y, r.w, r.h, r.progress, col),
            LayerKind::Image => {
                if let Some((iw, ih, data)) = r.source.as_deref().and_then(|s| self.images.get(s)) {
                    let w = if r.w > 0 { r.w } else { *iw as i32 };
                    let h = if r.h > 0 { r.h } else { *ih as i32 };
                    c.blit_rgba(
                        r.x,
                        r.y,
                        w.min(*iw as i32),
                        h.min(*ih as i32),
                        data,
                        r.opacity,
                    );
                }
            }
        }
    }

    fn clamp(&self, r: Rect) -> Rect {
        let x = r.x.min(self.width);
        let y = r.y.min(self.height);
        Rect::new(x, y, r.w.min(self.width - x), r.h.min(self.height - y))
    }
}

fn bbox(r: &Resolved) -> Rect {
    let (w, h) = match r.kind {
        LayerKind::Text => (
            font::text_width(&r.text, r.scale).max(1),
            font::GLYPH_H * r.scale,
        ),
        _ => (r.w.max(1), r.h.max(1)),
    };
    Rect::new(
        r.x.max(0) as u16,
        r.y.max(0) as u16,
        w.max(0) as u16,
        h.max(0) as u16,
    )
}
