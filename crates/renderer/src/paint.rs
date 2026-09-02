//! Paint: a flat colour or a named gradient.
//!
//! Rect / path / circle fills use a real `tiny_skia` gradient shader (AA). Glyph
//! fills sample the gradient on the CPU (`Gradient::sample`) since fontdue hands
//! back a coverage bitmap, not a path.

use std::collections::HashMap;

use serde::Deserialize;
use tiny_skia::{
    BlendMode, Color, GradientStop, LinearGradient, Paint, Point, RadialGradient, Shader,
    SpreadMode, Transform,
};

use crate::framebuffer::Rgba;

/// Axis-aligned box a gradient is evaluated over: `(x, y, w, h)`.
pub type BBox = (f32, f32, f32, f32);

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    /// Along the layer box (see `axis`).
    #[default]
    Linear,
    /// Centre -> edge of the layer box.
    Radial,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Top -> bottom.
    #[default]
    Vertical,
    /// Left -> right.
    Horizontal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stop {
    pub at: f32,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Gradient {
    pub name: String,
    #[serde(default)]
    pub kind: GradientKind,
    pub stops: Vec<Stop>,
    /// Linear only: which way the gradient runs.
    #[serde(default)]
    pub axis: Axis,
    /// Radial only: radius as a fraction of `max(w, h) / 2`.
    #[serde(default = "one")]
    pub radius_scale: f32,
}

fn one() -> f32 {
    1.0
}

impl Gradient {
    fn resolved_stops(&self) -> Vec<(f32, Rgba)> {
        let mut v: Vec<(f32, Rgba)> = self
            .stops
            .iter()
            .filter_map(|s| Rgba::parse(&s.color).map(|c| (s.at.clamp(0.0, 1.0), c)))
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    }

    /// Colour at normalised position `u` in `0..=1` along the gradient axis.
    pub fn sample(&self, u: f32) -> Rgba {
        let stops = self.resolved_stops();
        match stops.as_slice() {
            [] => Rgba::WHITE,
            [(_, c)] => *c,
            _ => {
                let u = u.clamp(0.0, 1.0);
                if u <= stops[0].0 {
                    return stops[0].1;
                }
                let last = stops.len() - 1;
                if u >= stops[last].0 {
                    return stops[last].1;
                }
                for w in stops.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    if u >= a.0 && u <= b.0 {
                        let p = if (b.0 - a.0).abs() < f32::EPSILON {
                            0.0
                        } else {
                            (u - a.0) / (b.0 - a.0)
                        };
                        return a.1.lerp(b.1, p);
                    }
                }
                stops[last].1
            }
        }
    }

    fn shader(&self, bbox: BBox, alpha: f32) -> Shader<'static> {
        let (x, y, w, h) = bbox;
        let stops: Vec<GradientStop> = self
            .resolved_stops()
            .into_iter()
            .map(|(at, c)| GradientStop::new(at, c.with_opacity(alpha).to_color()))
            .collect();
        let fallback = Shader::SolidColor(
            self.resolved_stops()
                .first()
                .map(|(_, c)| c.with_opacity(alpha).to_color())
                .unwrap_or(Color::WHITE),
        );
        match self.kind {
            GradientKind::Linear => {
                let (start, end) = match self.axis {
                    Axis::Vertical => (Point::from_xy(x, y), Point::from_xy(x, y + h.max(1.0))),
                    Axis::Horizontal => (Point::from_xy(x, y), Point::from_xy(x + w.max(1.0), y)),
                };
                LinearGradient::new(start, end, stops, SpreadMode::Pad, Transform::identity())
                    .unwrap_or(fallback)
            }
            GradientKind::Radial => {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let r = (w.max(h) / 2.0 * self.radius_scale).max(1.0);
                RadialGradient::new(
                    Point::from_xy(cx, cy),
                    Point::from_xy(cx, cy),
                    r,
                    stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                )
                .unwrap_or(fallback)
            }
        }
    }
}

pub type GradientTable<'a> = HashMap<&'a str, &'a Gradient>;

/// A colour reference on a layer: `"#rrggbb[aa]"` or `"gradient:<name>"`.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintSpec {
    Flat(Rgba),
    Grad(String),
}

impl PaintSpec {
    pub fn parse(s: &str) -> Option<PaintSpec> {
        match s.strip_prefix("gradient:") {
            Some(name) => Some(PaintSpec::Grad(name.trim().to_string())),
            None => Rgba::parse(s).map(PaintSpec::Flat),
        }
    }

    /// CPU colour at `(px, py)` inside `bbox` — used for glyph fills.
    pub fn color_at(&self, table: &GradientTable, bbox: BBox, px: f32, py: f32) -> Rgba {
        match self {
            PaintSpec::Flat(c) => *c,
            PaintSpec::Grad(name) => match table.get(name.as_str()) {
                Some(g) => {
                    let (x, y, w, h) = bbox;
                    let u = match g.kind {
                        GradientKind::Linear => match g.axis {
                            Axis::Vertical if h.abs() > f32::EPSILON => (py - y) / h,
                            Axis::Horizontal if w.abs() > f32::EPSILON => (px - x) / w,
                            _ => 0.0,
                        },
                        GradientKind::Radial => {
                            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
                            let r = (w.max(h) / 2.0 * g.radius_scale).max(1.0);
                            ((px - cx).hypot(py - cy) / r).min(1.0)
                        }
                    };
                    g.sample(u)
                }
                None => Rgba::WHITE,
            },
        }
    }

    /// A `tiny_skia::Paint` for rect / path / circle fills.
    pub fn to_paint<'p>(
        &self,
        table: &GradientTable,
        bbox: BBox,
        alpha: f32,
        blend: BlendMode,
    ) -> Paint<'p> {
        let shader = match self {
            PaintSpec::Flat(c) => Shader::SolidColor(c.with_opacity(alpha).to_color()),
            PaintSpec::Grad(name) => match table.get(name.as_str()) {
                Some(g) => g.shader(bbox, alpha),
                None => Shader::SolidColor(Rgba::WHITE.with_opacity(alpha).to_color()),
            },
        };
        Paint {
            shader,
            blend_mode: blend,
            anti_alias: true,
            ..Paint::default()
        }
    }
}
