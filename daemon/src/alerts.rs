//! Hardware alerts: decide which alert scene (if any) should own the panel now.
//!
//! Fixed precedence — fan-stopped > GPU thermal > VRAM saturation. Each alert
//! has a trigger threshold and a lower `*_clear` threshold (hysteresis); an
//! active alert holds for at least `min_secs` before the dashboard returns, but
//! a higher-precedence alert preempts immediately.

use std::time::Instant;

use renderer::Context;

use crate::config;
use crate::sensors::Readings;

/// The alerts, highest precedence first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Fan,
    GpuTemp,
    Vram,
}

const ORDER: [Alert; 3] = [Alert::Fan, Alert::GpuTemp, Alert::Vram];

pub struct Alerts {
    cfg: config::Alerts,
    active: Option<(Alert, Instant)>,
}

impl Alerts {
    pub fn new(cfg: config::Alerts) -> Self {
        Self { cfg, active: None }
    }

    /// Which alert should own the panel at `now` (`None` = show the dashboard).
    pub fn evaluate(&mut self, r: &Readings, now: Instant) -> Option<Alert> {
        if !self.cfg.enabled {
            self.active = None;
            return None;
        }

        let active_kind = self.active.map(|(a, _)| a);
        // Highest-precedence alert that is over its trigger threshold — or, for
        // the one already showing, still over its (lower) clear threshold.
        let want = ORDER
            .into_iter()
            .find(|&a| self.triggered(a, r) || (active_kind == Some(a) && self.sustained(a, r)));

        let min = self.cfg.min_secs.max(0.0);
        self.active = match (want, self.active) {
            (Some(a), Some((cur, since))) if a == cur => Some((cur, since)),
            (Some(a), Some((cur, since))) => {
                let preempts = prio(a) < prio(cur);
                let held = now.duration_since(since).as_secs_f32() >= min;
                if preempts || held {
                    Some((a, now))
                } else {
                    Some((cur, since))
                }
            }
            (Some(a), None) => Some((a, now)),
            (None, Some((cur, since))) => {
                (now.duration_since(since).as_secs_f32() < min).then_some((cur, since))
            }
            (None, None) => None,
        };
        self.active.map(|(a, _)| a)
    }

    fn triggered(&self, a: Alert, r: &Readings) -> bool {
        let c = &self.cfg;
        match a {
            Alert::GpuTemp => c.gpu_temp_c > 0 && r.gpu_temp_c.is_some_and(|t| t >= c.gpu_temp_c),
            Alert::Vram => c.vram_frac > 0.0 && vram_frac(r).is_some_and(|f| f >= c.vram_frac),
            Alert::Fan => c.fan_min_rpm > 0 && r.fan_rpm.is_some_and(|v| v < c.fan_min_rpm),
        }
    }

    fn sustained(&self, a: Alert, r: &Readings) -> bool {
        let c = &self.cfg;
        match a {
            Alert::GpuTemp => r.gpu_temp_c.is_some_and(|t| t >= c.gpu_temp_clear_c),
            Alert::Vram => vram_frac(r).is_some_and(|f| f >= c.vram_frac_clear),
            Alert::Fan => r.fan_rpm.is_some_and(|v| v < c.fan_clear_rpm),
        }
    }
}

fn prio(a: Alert) -> usize {
    ORDER.iter().position(|&x| x == a).unwrap()
}

fn vram_frac(r: &Readings) -> Option<f32> {
    match (r.vram_used, r.vram_total) {
        (Some(u), Some(t)) if t > 0 => Some(u as f32 / t as f32),
        _ => None,
    }
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// Fill the `{{ alert.* }}` slots the parametric alert scene reads, for `a`'s
/// current state. Overlay these onto the dashboard context.
pub fn overlay(a: Alert, r: &Readings, cfg: &config::Alerts, ctx: &mut Context) {
    let (title, label, value, threshold, action, frac): (_, _, String, String, _, f32) = match a {
        Alert::GpuTemp => {
            let t = r.gpu_temp_c.unwrap_or(0);
            (
                "! ALERTE THERMIQUE",
                "GPU TEMP",
                format!("{t}\u{00B0}C"),
                format!("SEUIL {}\u{00B0}C", cfg.gpu_temp_c),
                "VENTILATION 100%",
                (t as f32 / cfg.gpu_temp_c.max(1) as f32).clamp(0.0, 1.0),
            )
        }
        Alert::Vram => {
            let total = r.vram_total.map(gib).unwrap_or(0.0);
            (
                "! MEMOIRE SATUREE",
                "VRAM",
                format!("{:.1}G", r.vram_used.map(gib).unwrap_or(0.0)),
                format!("SEUIL {:.1}G", total * cfg.vram_frac as f64),
                "PURGE CACHE",
                vram_frac(r).unwrap_or(0.0).clamp(0.0, 1.0),
            )
        }
        Alert::Fan => (
            "! VENTILATEUR ARRETE",
            "FAN",
            format!("{} RPM", r.fan_rpm.unwrap_or(0)),
            format!("MINIMUM {} RPM", cfg.fan_min_rpm),
            "ARRET IMMINENT",
            0.0,
        ),
    };
    ctx.set("alert.title", title);
    ctx.set("alert.label", label);
    ctx.set("alert.value", value);
    ctx.set("alert.threshold", threshold);
    ctx.set("alert.action", action);
    ctx.set("alert.frac", format!("{frac:.3}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(gpu_temp: Option<i32>, vram: Option<(u64, u64)>, fan: Option<u32>) -> Readings {
        Readings {
            gpu_temp_c: gpu_temp,
            vram_used: vram.map(|v| v.0),
            vram_total: vram.map(|v| v.1),
            fan_rpm: fan,
            ..Default::default()
        }
    }

    fn cfg() -> config::Alerts {
        config::Alerts {
            min_secs: 0.0,
            ..Default::default()
        }
    }

    #[test]
    fn hysteresis_holds_between_trigger_and_clear() {
        let mut a = Alerts::new(cfg());
        let now = Instant::now();
        assert_eq!(a.evaluate(&r(Some(70), None, None), now), None);
        assert_eq!(
            a.evaluate(&r(Some(86), None, None), now),
            Some(Alert::GpuTemp)
        );
        // 82 is below trigger (85) but above clear (80) -> stays.
        assert_eq!(
            a.evaluate(&r(Some(82), None, None), now),
            Some(Alert::GpuTemp)
        );
        // 78 is below clear -> drops (min_secs 0).
        assert_eq!(a.evaluate(&r(Some(78), None, None), now), None);
    }

    #[test]
    fn higher_precedence_preempts_immediately() {
        let mut a = Alerts::new(config::Alerts {
            min_secs: 999.0,
            ..Default::default()
        });
        let now = Instant::now();
        a.evaluate(&r(Some(90), None, None), now);
        // fan stops while the thermal alert's min_secs is nowhere near met.
        assert_eq!(
            a.evaluate(&r(Some(90), None, Some(0)), now),
            Some(Alert::Fan)
        );
    }

    #[test]
    fn min_secs_delays_only_the_return_to_dashboard() {
        let mut a = Alerts::new(config::Alerts {
            min_secs: 999.0,
            ..Default::default()
        });
        let now = Instant::now();
        a.evaluate(&r(Some(90), None, None), now);
        // condition gone, but min_secs not met -> still shown.
        assert_eq!(
            a.evaluate(&r(Some(60), None, None), now),
            Some(Alert::GpuTemp)
        );
    }
}
