//! Keyframe animations.
//!
//! An [`Anim`] drives one scalar property through a list of `(t, v)` keys with a
//! single easing between segments (mirrors the design tool's `interpolate`).
//! The old two-key `from`/`to`/`start`/`end` form is still accepted and expands
//! to two keys.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    EaseOutQuad,
    EaseInOutQuad,
    EaseOutCubic,
    /// Overshoots past the target then settles (`c1 = 1.70158`).
    EaseOutBack,
}

impl Easing {
    /// Map `t` in `0..=1` through the curve. May return slightly outside
    /// `0..=1` for `EaseOutBack` (intentional overshoot).
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Easing::EaseOutQuad => t * (2.0 - t),
            Easing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            Easing::EaseOutCubic => {
                let u = t - 1.0;
                u * u * u + 1.0
            }
            Easing::EaseOutBack => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                let u = t - 1.0;
                1.0 + c3 * u * u * u + c1 * u * u
            }
        }
    }
}

/// Which resolved property an [`Anim`] drives.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnimProperty {
    Opacity,
    X,
    Y,
    Scale,
    Width,
    Height,
    /// Degrees, clockwise, about the layer anchor.
    Rotation,
    /// `0..1` fraction of a path / circle outline that is drawn.
    Trace,
    /// `0..1` fill of a progress bar.
    Value,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct Key {
    pub t: f32,
    pub v: f32,
}

#[derive(Debug, Clone)]
pub struct Anim {
    pub property: AnimProperty,
    /// At least two, sorted by `t`.
    pub keys: Vec<Key>,
    pub easing: Easing,
}

impl Anim {
    /// Resolved value at scene time `t` seconds (clamped to the end keys).
    pub fn sample(&self, t: f32) -> f32 {
        let ks = &self.keys;
        let last = ks.len() - 1;
        if t <= ks[0].t {
            return ks[0].v;
        }
        if t >= ks[last].t {
            return ks[last].v;
        }
        for w in ks.windows(2) {
            let (a, b) = (w[0], w[1]);
            if t >= a.t && t <= b.t {
                let span = (b.t - a.t).max(f32::EPSILON);
                let p = self.easing.apply((t - a.t) / span);
                return a.v + (b.v - a.v) * p;
            }
        }
        ks[last].v
    }
}

/// Wire form: either `keys = [{t, v}, ...]` or `from`/`to`/`start`/`end`.
#[derive(Deserialize)]
struct AnimRepr {
    property: AnimProperty,
    #[serde(default)]
    easing: Easing,
    #[serde(default)]
    keys: Option<Vec<Key>>,
    #[serde(default)]
    from: Option<f32>,
    #[serde(default)]
    to: Option<f32>,
    #[serde(default)]
    start: Option<f32>,
    #[serde(default)]
    end: Option<f32>,
}

impl TryFrom<AnimRepr> for Anim {
    type Error = String;

    fn try_from(r: AnimRepr) -> Result<Self, String> {
        let keys = match r.keys {
            Some(mut ks) => {
                if ks.len() < 2 {
                    return Err("anim `keys` needs at least two entries".into());
                }
                ks.sort_by(|a, b| a.t.total_cmp(&b.t));
                ks
            }
            None => {
                let from = r.from.ok_or("anim needs `keys` or `from`/`to`/`end`")?;
                let to = r.to.ok_or("anim `from` given without `to`")?;
                let start = r.start.unwrap_or(0.0);
                let end = r.end.ok_or("anim needs `end`")?;
                vec![
                    Key { t: start, v: from },
                    Key {
                        t: end.max(start),
                        v: to,
                    },
                ]
            }
        };
        Ok(Anim {
            property: r.property,
            keys,
            easing: r.easing,
        })
    }
}

impl<'de> Deserialize<'de> for Anim {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Anim::try_from(AnimRepr::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(toml_src: &str) -> Anim {
        #[derive(Deserialize)]
        struct W {
            anim: Anim,
        }
        toml::from_str::<W>(toml_src).unwrap().anim
    }

    #[test]
    fn two_key_sugar() {
        let a = anim(
            "[anim]\nproperty='opacity'\nfrom=0.0\nto=1.0\nstart=1.0\nend=3.0\neasing='linear'",
        );
        assert_eq!(a.sample(0.0), 0.0);
        assert_eq!(a.sample(2.0), 0.5);
        assert_eq!(a.sample(9.0), 1.0);
    }

    #[test]
    fn multi_key_round_trip() {
        let a = anim(
            "[anim]\nproperty='opacity'\neasing='linear'\nkeys=[{t=0.0,v=0.0},{t=2.0,v=1.0},{t=4.0,v=0.0}]",
        );
        assert_eq!(a.sample(1.0), 0.5);
        assert_eq!(a.sample(3.0), 0.5);
        assert_eq!(a.sample(4.0), 0.0);
        assert_eq!(a.sample(99.0), 0.0);
    }

    #[test]
    fn ease_out_back_overshoots() {
        assert!(Easing::EaseOutBack.apply(0.6) > 1.0);
    }
}
