//! Keyframe animations: a single scalar property tweened from `from` to `to`
//! between `start` and `end` seconds of scene time, through an easing curve.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    /// Map `t` in `0..=1` through the curve.
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
    /// The 0..1 fill of a progress bar.
    Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Anim {
    pub property: AnimProperty,
    pub from: f32,
    pub to: f32,
    #[serde(default)]
    pub start: f32,
    pub end: f32,
    #[serde(default)]
    pub easing: Easing,
}

impl Anim {
    /// Resolved value at scene time `t` seconds (clamped outside `start..end`).
    pub fn sample(&self, t: f32) -> f32 {
        if t <= self.start {
            return self.from;
        }
        if t >= self.end {
            return self.to;
        }
        let span = (self.end - self.start).max(f32::EPSILON);
        let p = self.easing.apply((t - self.start) / span);
        self.from + (self.to - self.from) * p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_interpolates() {
        let a = Anim {
            property: AnimProperty::Opacity,
            from: 0.0,
            to: 1.0,
            start: 1.0,
            end: 3.0,
            easing: Easing::Linear,
        };
        assert_eq!(a.sample(0.0), 0.0);
        assert_eq!(a.sample(2.0), 0.5);
        assert_eq!(a.sample(9.0), 1.0);
    }
}
