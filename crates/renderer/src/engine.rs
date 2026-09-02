//! [`SceneEngine`]: resolve a scene against a [`Context`] at time `t`, paint a
//! frame with `tiny_skia`, and report the regions that changed.

use std::collections::HashMap;
use std::path::Path as FsPath;

use anyhow::Context as _;
use tiny_skia::{BlendMode, Path, Pixmap, Transform};
use turzx::{DirtyTracker, Frame, Rect};

use crate::anim::AnimProperty;
use crate::context::Context;
use crate::font::Fonts;
use crate::framebuffer::{Canvas, Rgba};
use crate::paint::{BBox, GradientTable, PaintSpec};
use crate::path::{glyph_transform, parse_path, path_length};
use crate::scene::{Align, Anchor, Blend, Layer, LayerKind, SceneFile};

/// Centre of the built-in Horus glyph's view box.
const GLYPH_C: f32 = 333.33334;

pub struct SceneEngine {
    width: u16,
    height: u16,
    fonts: Fonts,
    images: HashMap<String, Pixmap>,
    /// raw `d` key -> (path, outline length, centre x, centre y).
    paths: HashMap<String, (Path, f32, f32, f32)>,
    prev: Vec<Resolved>,
}

#[derive(Debug, Clone, PartialEq)]
struct Resolved {
    kind: LayerKind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
    rotation: f32,
    trace: f32,
    anchor: Anchor,
    blend: Blend,
    opacity: f32,
    glow: f32,
    glow_radius: f32,
    fill: Option<PaintSpec>,
    stroke: Option<PaintSpec>,
    stroke_width: f32,
    stroke_opacity: f32,
    text: String,
    font: Option<String>,
    size: f32,
    letter_spacing: f32,
    align: Align,
    progress: f32,
    radius: f32,
    d: Option<String>,
    period: i32,
    source: Option<String>,
}

impl SceneEngine {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            fonts: Fonts::embedded(),
            images: HashMap::new(),
            paths: HashMap::new(),
            prev: Vec::new(),
        }
    }

    /// Decode image PNGs and parse `path` layers once, up front.
    #[allow(clippy::map_entry)] // the inserts are fallible; `entry` would be worse
    pub fn load_assets(&mut self, scene: &SceneFile, base: &FsPath) -> anyhow::Result<()> {
        for layer in &scene.layers {
            match layer.kind {
                LayerKind::Image => {
                    if let Some(src) = &layer.source {
                        if !self.images.contains_key(src) {
                            let p = base.join(src);
                            let bytes = std::fs::read(&p)
                                .with_context(|| format!("reading image {}", p.display()))?;
                            let pm = Pixmap::decode_png(&bytes)
                                .with_context(|| format!("decoding {}", p.display()))?;
                            self.images.insert(src.clone(), pm);
                        }
                    }
                }
                LayerKind::Path => {
                    let key = layer
                        .d
                        .clone()
                        .or_else(|| layer.source.clone())
                        .context("path layer needs `d` or `source`")?;
                    if !self.paths.contains_key(&key) {
                        let d = resolve_path_data(&key, base)?;
                        let path = parse_path(&d)
                            .with_context(|| format!("parsing path data for `{key}`"))?;
                        let b = path.bounds();
                        let (cx, cy) = if key == "@horus_eye" {
                            (GLYPH_C, GLYPH_C)
                        } else {
                            (b.left() + b.width() / 2.0, b.top() + b.height() / 2.0)
                        };
                        let len = path_length(&path);
                        self.paths.insert(key, (path, len, cx, cy));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn render(&mut self, scene: &SceneFile, ctx: &Context, t: f32) -> (Frame, Vec<Rect>) {
        let grads: GradientTable = scene
            .gradients
            .iter()
            .map(|g| (g.name.as_str(), g))
            .collect();

        let resolved: Vec<Resolved> = scene
            .layers
            .iter()
            .map(|l| self.resolve(l, ctx, t))
            .collect();

        let mut canvas = Canvas::new(self.width, self.height);
        let full = (0.0, 0.0, self.width as f32, self.height as f32);
        match PaintSpec::parse(&scene.scene.background) {
            Some(PaintSpec::Flat(c)) => canvas.clear(c),
            Some(spec) => {
                canvas.clear(Rgba::BLACK);
                let p = spec.to_paint(&grads, full, 1.0, BlendMode::SourceOver);
                canvas.fill_all(&p);
            }
            None => canvas.clear(Rgba::BLACK),
        }

        for r in &resolved {
            self.paint(&mut canvas, r, &grads);
        }

        let frame = canvas.into_frame();

        let mut dirty = DirtyTracker::new();
        if self.prev.len() != resolved.len() {
            dirty.add(Rect::new(0, 0, self.width, self.height));
        } else {
            for (a, b) in self.prev.iter().zip(&resolved) {
                if a != b {
                    dirty.add(self.clamp(self.bbox(a)));
                    dirty.add(self.clamp(self.bbox(b)));
                }
            }
        }
        self.prev = resolved;
        (frame, dirty.regions().to_vec())
    }

    fn resolve(&self, l: &Layer, ctx: &Context, t: f32) -> Resolved {
        let bitmap_text = l.kind == LayerKind::Text && l.font.is_none();
        let mut x = l.x;
        let mut y = l.y;
        let mut w = l.width;
        let mut h = l.height;
        let mut scale = l.scale.unwrap_or(if bitmap_text { 2.0 } else { 1.0 });
        let mut rotation = l.rotation;
        let mut opacity = l.opacity;
        let mut trace = l
            .trace
            .or(if l.kind == LayerKind::Text {
                l.letter_reveal
            } else {
                None
            })
            .unwrap_or(1.0);
        let mut anim_value: Option<f32> = None;

        for a in &l.anims {
            let v = a.sample(t);
            match a.property {
                AnimProperty::X => x = v,
                AnimProperty::Y => y = v,
                AnimProperty::Width => w = v,
                AnimProperty::Height => h = v,
                AnimProperty::Scale => scale = v,
                AnimProperty::Rotation => rotation = v,
                AnimProperty::Trace => trace = v,
                AnimProperty::Opacity => opacity = v,
                AnimProperty::Value => anim_value = Some(v),
            }
        }

        let raw = l.value.as_deref().unwrap_or("");
        let text = ctx.expand(raw);
        let progress = anim_value
            .or_else(|| ctx.expand_f32(if raw.is_empty() { "0" } else { raw }))
            .unwrap_or(0.0);

        let parse_paint = |s: &str| match s {
            "none" => None,
            other => PaintSpec::parse(other),
        };
        let fill = if l.fill.as_deref() == Some("none") {
            None
        } else {
            l.fill
                .as_deref()
                .and_then(PaintSpec::parse)
                .or_else(|| l.color.as_deref().and_then(PaintSpec::parse))
                .or(match l.kind {
                    LayerKind::Path | LayerKind::Circle | LayerKind::Image => None,
                    _ => Some(PaintSpec::Flat(Rgba::WHITE)),
                })
        };
        let stroke = l.stroke.as_deref().and_then(parse_paint);

        Resolved {
            kind: l.kind,
            x,
            y,
            w,
            h,
            scale: scale.max(0.0),
            rotation,
            trace: trace.clamp(0.0, 1.0),
            anchor: l.anchor,
            blend: l.blend,
            opacity: opacity.clamp(0.0, 1.0),
            glow: l.glow.clamp(0.0, 1.0),
            glow_radius: l.glow_radius.max(0.0),
            fill,
            stroke,
            stroke_width: l.stroke_width.unwrap_or(2.0),
            stroke_opacity: l.stroke_opacity.unwrap_or(1.0).clamp(0.0, 1.0),
            text,
            font: l.font.clone(),
            size: l.size.unwrap_or(24.0),
            letter_spacing: l.letter_spacing,
            align: l.align,
            progress,
            radius: l.radius,
            d: l.d.clone().or_else(|| l.source.clone()),
            period: l.period,
            source: l.source.clone(),
        }
    }

    fn paint(&self, canvas: &mut Canvas, r: &Resolved, grads: &GradientTable) {
        if r.opacity <= 0.0 {
            return;
        }
        if r.glow > 0.0 {
            let mut halo = Canvas::new(self.width, self.height);
            self.paint_core(&mut halo, r, grads);
            halo.blur(r.glow_radius);
            canvas.draw_pixmap(
                halo.pixmap(),
                Transform::identity(),
                r.glow,
                BlendMode::Plus,
            );
        }
        self.paint_core(canvas, r, grads);
    }

    fn paint_core(&self, canvas: &mut Canvas, r: &Resolved, grads: &GradientTable) {
        let bbox = self.layer_bbox(r);
        let blend = if r.blend == Blend::Add {
            BlendMode::Plus
        } else {
            BlendMode::SourceOver
        };

        match r.kind {
            LayerKind::Rect => {
                if let Some(f) = &r.fill {
                    let p = f.to_paint(grads, bbox, r.opacity, blend);
                    canvas.fill_rect(r.x, r.y, r.w, r.h, &p);
                }
                if let Some(s) = &r.stroke {
                    let p = s.to_paint(grads, bbox, r.opacity * r.stroke_opacity, blend);
                    canvas.stroke_rect(r.x, r.y, r.w, r.h, r.stroke_width, &p);
                }
            }
            LayerKind::Scanlines => {
                let c = r
                    .fill
                    .as_ref()
                    .map(flat_of)
                    .unwrap_or(Rgba(0, 0, 0, 100))
                    .with_opacity(r.opacity);
                canvas.scanlines(r.period, c);
            }
            LayerKind::ProgressBar => {
                if let Some(f) = &r.fill {
                    let track = f.to_paint(grads, bbox, r.opacity * 0.24, blend);
                    canvas.fill_rect(r.x, r.y, r.w, r.h, &track);
                    let fillp = f.to_paint(grads, bbox, r.opacity, blend);
                    canvas.fill_rect(r.x, r.y, r.w * r.progress.clamp(0.0, 1.0), r.h, &fillp);
                }
            }
            LayerKind::Circle => {
                let rad = (r.radius * r.scale).max(0.1);
                let Some(path) = Canvas::circle_path(r.x, r.y, rad) else {
                    return;
                };
                let ts = Transform::from_translate(r.x, r.y)
                    .pre_rotate(r.rotation)
                    .pre_translate(-r.x, -r.y);
                let paint_spec = r.stroke.as_ref().or(r.fill.as_ref());
                if let Some(spec) = paint_spec {
                    let p = spec.to_paint(grads, bbox, r.opacity * r.stroke_opacity, blend);
                    let circ = 2.0 * std::f32::consts::PI * rad;
                    let dash = (r.trace < 1.0).then_some((circ * r.trace, circ + 1.0));
                    canvas.stroke_path(&path, &p, r.stroke_width, ts, dash);
                }
            }
            LayerKind::Path => {
                let Some(key) = &r.d else { return };
                let Some((path, len, cx, cy)) = self.paths.get(key) else {
                    return;
                };
                let ts = glyph_transform(r.x, r.y, r.scale, r.rotation, *cx, *cy);
                if let Some(spec) = &r.stroke {
                    if r.trace > 0.0 {
                        let p = spec.to_paint(grads, bbox, r.opacity * r.stroke_opacity, blend);
                        let sl = len * r.scale;
                        let dash = (r.trace < 1.0).then_some((sl * r.trace, sl + 1.0));
                        let w = (r.stroke_width / r.scale.max(0.02)).max(0.1);
                        canvas.stroke_path(path, &p, w, ts, dash);
                    }
                }
                if let Some(spec) = &r.fill {
                    let p = spec.to_paint(grads, bbox, r.opacity, blend);
                    canvas.fill_path(path, &p, ts);
                }
            }
            LayerKind::Image => {
                let Some(src) = &r.source else { return };
                let Some(pm) = self.images.get(src) else {
                    return;
                };
                let (iw, ih) = (pm.width() as f32, pm.height() as f32);
                let mut ts = Transform::from_translate(r.x, r.y)
                    .pre_rotate(r.rotation)
                    .pre_scale(r.scale, r.scale);
                if r.anchor == Anchor::Center {
                    ts = ts.pre_translate(-iw / 2.0, -ih / 2.0);
                }
                canvas.draw_pixmap(pm, ts, r.opacity, blend);
            }
            LayerKind::Text => self.paint_text(canvas, r, grads, bbox),
        }
    }

    fn paint_text(&self, canvas: &mut Canvas, r: &Resolved, grads: &GradientTable, bbox: BBox) {
        let Some(spec) = &r.fill else { return };
        let Some(font_name) = &r.font else {
            let c = flat_of(spec).with_opacity(r.opacity);
            canvas.draw_bitmap_text(r.x, r.y, &r.text, r.scale, c);
            return;
        };

        let font = self.fonts.get(font_name);
        let px = (r.size * r.scale).max(1.0);
        let tracking = r.letter_spacing * px;
        let total = self.fonts.measure(font_name, &r.text, px, tracking);
        let start_x = match r.align {
            Align::Start => r.x,
            Align::Middle => r.x - total / 2.0,
            Align::End => r.x - total,
        };

        let n = r.text.chars().count().max(1) as f32;
        let mut pen = start_x;
        for (i, ch) in r.text.chars().enumerate() {
            let m = font.metrics(ch, px);
            if !ch.is_whitespace() {
                let (mm, cov) = font.rasterize(ch, px);
                // per-letter reveal driven by `trace`
                let letter = (r.trace * n - i as f32).clamp(0.0, 1.0);
                let gx = (pen + mm.xmin as f32).round() as i32;
                let gy = (r.y - mm.height as f32 - mm.ymin as f32).round() as i32;
                canvas.blit_coverage(
                    gx,
                    gy,
                    mm.width,
                    mm.height,
                    &cov,
                    r.opacity * letter,
                    |px_, py_| spec.color_at(grads, bbox, px_, py_),
                );
            }
            pen += m.advance_width + tracking;
        }
    }

    /// Box a gradient / glow / dirty region is evaluated against.
    fn layer_bbox(&self, r: &Resolved) -> BBox {
        match r.kind {
            LayerKind::Text => {
                let px = (r.size * r.scale).max(1.0);
                let w = if r.font.is_some() {
                    self.fonts.measure(
                        r.font.as_deref().unwrap_or(""),
                        &r.text,
                        px,
                        r.letter_spacing * px,
                    )
                } else {
                    r.text.chars().count() as f32 * 6.0 * r.scale
                };
                let x0 = match r.align {
                    Align::Start => r.x,
                    Align::Middle => r.x - w / 2.0,
                    Align::End => r.x - w,
                };
                (x0, r.y - px, w.max(1.0), px * 1.3)
            }
            LayerKind::Circle => {
                let rad = r.radius * r.scale;
                (r.x - rad, r.y - rad, 2.0 * rad, 2.0 * rad)
            }
            LayerKind::Path => {
                let s = 666.667 * r.scale;
                (r.x - s / 2.0, r.y - s / 2.0, s, s)
            }
            LayerKind::Image => {
                let side = 256.0 * r.scale;
                if r.anchor == Anchor::Center {
                    (r.x - side / 2.0, r.y - side / 2.0, side, side)
                } else {
                    (r.x, r.y, side, side)
                }
            }
            _ => (r.x, r.y, r.w.max(1.0), r.h.max(1.0)),
        }
    }

    fn bbox(&self, r: &Resolved) -> Rect {
        let (mut x, mut y, mut w, mut h) = self.layer_bbox(r);
        if r.rotation.abs() > 0.5 {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let d = (w * w + h * h).sqrt();
            x = cx - d / 2.0;
            y = cy - d / 2.0;
            w = d;
            h = d;
        }
        if r.glow > 0.0 {
            let g = r.glow_radius * 3.0;
            x -= g;
            y -= g;
            w += g * 2.0;
            h += g * 2.0;
        }
        Rect::new(
            x.max(0.0) as u16,
            y.max(0.0) as u16,
            w.max(0.0) as u16,
            h.max(0.0) as u16,
        )
    }

    fn clamp(&self, r: Rect) -> Rect {
        let x = r.x.min(self.width);
        let y = r.y.min(self.height);
        Rect::new(x, y, r.w.min(self.width - x), r.h.min(self.height - y))
    }
}

fn flat_of(spec: &PaintSpec) -> Rgba {
    match spec {
        PaintSpec::Flat(c) => *c,
        PaintSpec::Grad(_) => Rgba::WHITE,
    }
}

fn resolve_path_data(key: &str, base: &FsPath) -> anyhow::Result<String> {
    if key == "@horus_eye" {
        return Ok(crate::HORUS_EYE_PATH.trim().to_string());
    }
    if let Some(rest) = key.strip_prefix('@') {
        anyhow::bail!("unknown built-in path `@{rest}`");
    }
    if key.ends_with(".svg") {
        let p = base.join(key);
        let svg =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        let d = svg
            .split_once(" d=\"")
            .or_else(|| svg.split_once("\td=\""))
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(d, _)| d.to_string())
            .with_context(|| format!("no <path d=…> in {}", p.display()))?;
        return Ok(d.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    Ok(key.to_string())
}
