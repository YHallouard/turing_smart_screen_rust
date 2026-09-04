//! Live system sensors for the dashboard scene.
//!
//! Pure sysfs / procfs / `nvidia-smi` / MangoHud-log reads, no library deps.
//! Every source is best-effort: if it is missing or unreadable the reading is
//! `None` and the dashboard shows a dash.
//!
//! - GPU: either the AMD render node exposing `gpu_busy_percent` (the BC-250 APU
//!   on the reference hardware; its `gpu_busy_percent` is bimodal noise, so it
//!   is burst-averaged and then EMA-smoothed) or, via `nvidia-smi`, an NVIDIA
//!   card. `[sensors] gpu = "auto" | "amd" | "nvidia"`.
//! - CPU: `/proc/stat` delta + `k10temp` (or `coretemp`).
//! - FPS: MangoHud's CSV log, when a game is logging.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use renderer::Context;

use crate::config;

/// One poll of every source.
#[derive(Debug, Default, Clone)]
pub struct Readings {
    pub gpu_pct: Option<u32>,
    pub gpu_temp_c: Option<i32>,
    pub cpu_pct: Option<u32>,
    pub cpu_temp_c: Option<i32>,
    pub vram_used: Option<u64>,
    pub vram_total: Option<u64>,
    /// In-game FPS from MangoHud, when a game is logging. `None` otherwise.
    pub fps: Option<u32>,
    /// Lowest reading across all hwmon `fanN_input` tachs (0 = a stopped fan).
    /// `None` if the platform exposes no fan tach.
    pub fan_rpm: Option<u32>,
}

/// Where GPU numbers come from.
enum Gpu {
    Amd {
        busy: Option<PathBuf>,
        temp: Option<PathBuf>,
        vram_used: Option<PathBuf>,
        vram_total: Option<PathBuf>,
    },
    Nvidia,
}

pub struct Sensors {
    gpu: Gpu,
    gpu_ema: Option<f32>,
    cpu_temp: Option<PathBuf>,
    prev_cpu: Option<(u64, u64)>, // (idle, total) jiffies
    fan_inputs: Vec<PathBuf>,
    mangohud_dir: Option<PathBuf>,
    fps_stale: Duration,
    /// Delete MangoHud CSVs older than this (keeping the newest). `None` = keep.
    fps_prune: Option<Duration>,
}

/// EMA weight for the newest GPU-load sample (`0..1`; higher = less smoothing).
const GPU_EMA_ALPHA: f32 = 0.6;
/// `gpu_busy_percent` reads per poll, spaced `GPU_BURST_GAP`, then averaged.
const GPU_BURST_N: u32 = 24;
const GPU_BURST_GAP: Duration = Duration::from_millis(2);

impl Sensors {
    pub fn detect(
        sel: config::GpuSel,
        mangohud_dir: Option<PathBuf>,
        fps_stale: Duration,
        fps_prune: Option<Duration>,
    ) -> Self {
        let gpu = match sel {
            config::GpuSel::Nvidia => Gpu::Nvidia,
            config::GpuSel::Amd => detect_amd(),
            config::GpuSel::Auto => {
                if nvidia_query().is_some() {
                    Gpu::Nvidia
                } else {
                    detect_amd()
                }
            }
        };

        let cpu_temp = hwmon_by_name("k10temp")
            .or_else(|| hwmon_by_name("coretemp"))
            .map(|d| d.join("temp1_input"));
        let fan_inputs = fan_tachs();
        let mangohud_dir = mangohud_dir.or_else(mangohud_output_folder);

        log::info!(
            "sensors: gpu={} cpu_temp={} fans={} mangohud_dir={:?}",
            match &gpu {
                Gpu::Nvidia => "nvidia-smi".to_string(),
                Gpu::Amd { busy, .. } => format!("amd sysfs (busy={})", busy.is_some()),
            },
            cpu_temp.is_some(),
            fan_inputs.len(),
            mangohud_dir.as_deref(),
        );

        Self {
            gpu,
            gpu_ema: None,
            fan_inputs,
            cpu_temp,
            prev_cpu: None,
            mangohud_dir,
            fps_stale,
            fps_prune,
        }
    }

    /// Read every source. CPU % needs two calls to produce a value (it is a
    /// delta); the first call returns `cpu_pct: None`.
    pub fn sample(&mut self) -> Readings {
        let (gpu_load, gpu_temp_c, vram_used, vram_total) = match &self.gpu {
            Gpu::Amd {
                busy,
                temp,
                vram_used,
                vram_total,
            } => (
                busy.as_deref().and_then(burst_average),
                read_milli_c(temp.as_deref()),
                read_parse::<u64>(vram_used.as_deref()),
                read_parse::<u64>(vram_total.as_deref()),
            ),
            Gpu::Nvidia => match nvidia_query() {
                Some((util, used, total, temp)) => {
                    (Some(util as f32), Some(temp), Some(used), Some(total))
                }
                None => (None, None, None, None),
            },
        };

        // EMA-smooth the load; drop the EMA entirely when the source goes away.
        let gpu_pct = match gpu_load {
            Some(v) => {
                let ema = match self.gpu_ema {
                    Some(prev) => GPU_EMA_ALPHA * v + (1.0 - GPU_EMA_ALPHA) * prev,
                    None => v,
                };
                self.gpu_ema = Some(ema);
                Some(ema.round().clamp(0.0, 100.0) as u32)
            }
            None => {
                self.gpu_ema = None;
                None
            }
        };

        Readings {
            gpu_pct,
            gpu_temp_c,
            cpu_pct: self.cpu_pct(),
            cpu_temp_c: read_milli_c(self.cpu_temp.as_deref()),
            vram_used,
            vram_total,
            fps: self.mangohud_fps(),
            fan_rpm: self
                .fan_inputs
                .iter()
                .filter_map(|p| read_parse::<u32>(Some(p)))
                .min(),
        }
    }

    fn cpu_pct(&mut self) -> Option<u32> {
        let stat = fs::read_to_string("/proc/stat").ok()?;
        let first = stat.lines().next()?; // "cpu  user nice system idle iowait irq softirq ..."
        let cols: Vec<u64> = first
            .split_whitespace()
            .skip(1)
            .filter_map(|c| c.parse().ok())
            .collect();
        if cols.len() < 4 {
            return None;
        }
        let idle = cols[3] + cols.get(4).copied().unwrap_or(0); // idle + iowait
        let total: u64 = cols.iter().sum();

        let pct = match self.prev_cpu {
            Some((pidle, ptotal)) if total > ptotal => {
                let dt = total - ptotal;
                let di = idle.saturating_sub(pidle);
                Some((((dt - di) as f64 / dt as f64) * 100.0).round() as u32)
            }
            _ => None,
        };
        self.prev_cpu = Some((idle, total));
        pct
    }

    /// Newest `*.csv` in the MangoHud folder, touched within `fps_stale`: the
    /// last data row's first column (MangoHud always writes `fps` first). Also
    /// prunes CSVs older than `fps_prune` (keeping the newest).
    fn mangohud_fps(&self) -> Option<u32> {
        let dir = self.mangohud_dir.as_deref()?;
        let mut csvs: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("csv"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        csvs.sort_by_key(|&(mtime, _)| std::cmp::Reverse(mtime)); // newest first

        if let Some(max_age) = self.fps_prune {
            for (mtime, path) in csvs.iter().skip(1) {
                if mtime.elapsed().is_ok_and(|age| age > max_age) {
                    let _ = fs::remove_file(path);
                }
            }
        }

        let (mtime, path) = csvs.into_iter().next()?;
        if mtime.elapsed().ok()? > self.fps_stale {
            return None; // stale log — no game running
        }
        let text = fs::read_to_string(&path).ok()?;
        let mut after_header = false;
        let mut last: Option<f64> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("fps,") {
                after_header = true;
            } else if after_header {
                if let Some(v) = line.split(',').next().and_then(|f| f.parse::<f64>().ok()) {
                    last = Some(v);
                }
            }
        }
        last.map(|v| v.round().clamp(0.0, 9999.0) as u32)
    }
}

/// Locate the first AMD render node exposing `gpu_busy_percent`.
fn detect_amd() -> Gpu {
    for i in 0..8 {
        let dev = PathBuf::from(format!("/sys/class/drm/card{i}/device"));
        let busy = dev.join("gpu_busy_percent");
        if !busy.is_file() {
            continue;
        }
        let vu = dev.join("mem_info_vram_used");
        let vt = dev.join("mem_info_vram_total");
        return Gpu::Amd {
            busy: Some(busy),
            temp: first_temp_input(&dev.join("hwmon")),
            vram_used: vu.is_file().then_some(vu),
            vram_total: vt.is_file().then_some(vt),
        };
    }
    Gpu::Amd {
        busy: None,
        temp: None,
        vram_used: None,
        vram_total: None,
    }
}

/// `(util%, mem_used_bytes, mem_total_bytes, temp_c)` from `nvidia-smi`.
fn nvidia_query() -> Option<(u32, u64, u64, i32)> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut fields = text.lines().next()?.split(',').map(str::trim);
    let util: u32 = fields.next()?.parse().ok()?;
    let used_mib: u64 = fields.next()?.parse().ok()?;
    let total_mib: u64 = fields.next()?.parse().ok()?;
    let temp: i32 = fields.next()?.parse().ok()?;
    Some((util, used_mib << 20, total_mib << 20, temp))
}

/// Average `GPU_BURST_N` back-to-back reads — `gpu_busy_percent` on amdgpu
/// flips between 0 and ~100 between samples.
fn burst_average(path: &Path) -> Option<f32> {
    let mut sum = 0.0f32;
    let mut n = 0u32;
    for _ in 0..GPU_BURST_N {
        if let Ok(s) = fs::read_to_string(path) {
            if let Ok(v) = s.trim().parse::<u32>() {
                sum += v as f32;
                n += 1;
            }
        }
        std::thread::sleep(GPU_BURST_GAP);
    }
    (n > 0).then(|| sum / n as f32)
}

/// Format a [`Readings`] into the `{{ … }}` slots the dashboard scene expects.
/// Values are space-padded to a fixed width so the bitmap font (no `align`)
/// lines them up under a fixed `x`. `fps_bar_max` is the FPS that fills the bar.
pub fn dashboard_context(r: &Readings, fps_bar_max: f32) -> Context {
    let mut c = Context::new();

    let pct = |v: Option<u32>| match v {
        Some(n) => format!("{:>3}%", n.min(999)),
        None => "  -%".to_string(),
    };
    let frac = |v: Option<u32>| format!("{:.3}", v.map_or(0.0, |n| n.min(100) as f32 / 100.0));

    c.set("gpu.pct", pct(r.gpu_pct));
    c.set("gpu.frac", frac(r.gpu_pct));
    c.set("cpu.pct", pct(r.cpu_pct));
    c.set("cpu.frac", frac(r.cpu_pct));

    let temp = |v: Option<i32>| match v {
        Some(n) => format!("{n:>3}\u{00B0}C"),
        None => "  -\u{00B0}C".to_string(),
    };
    c.set("gpu.temp", temp(r.gpu_temp_c));
    c.set("cpu.temp", temp(r.cpu_temp_c));

    match (r.vram_used, r.vram_total) {
        (Some(used), Some(total)) if total > 0 => {
            let gib = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
            c.set("vram", format!("{:.1}/{:.0}G", gib(used), gib(total)));
            c.set(
                "vram.frac",
                format!("{:.3}", (used as f64 / total as f64) as f32),
            );
        }
        _ => {
            c.set("vram", " -/-G");
            c.set("vram.frac", "0");
        }
    }

    c.set(
        "fps",
        r.fps
            .map_or_else(|| "  -".to_string(), |f| format!("{f:>3}")),
    );
    let fps_max = fps_bar_max.max(1.0);
    c.set(
        "fps.frac",
        format!(
            "{:.3}",
            r.fps.map_or(0.0, |f| (f as f32 / fps_max).min(1.0))
        ),
    );

    c.set(
        "fan.rpm",
        r.fan_rpm
            .map_or_else(|| "-".to_string(), |v| format!("{v} RPM")),
    );

    c
}

/// Read `output_folder=` from `MangoHud.conf` (`$MANGOHUD_CONFIGFILE`, else
/// `~/.config/MangoHud/MangoHud.conf`). Falls back to `$HOME`.
fn mangohud_output_folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let conf = std::env::var_os("MANGOHUD_CONFIGFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config/MangoHud/MangoHud.conf"));
    let folder = fs::read_to_string(&conf).ok().and_then(|text| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix("output_folder="))
            .map(|v| v.trim().to_string())
            .next()
    });
    Some(match folder.as_deref() {
        Some("~") | None => home,
        Some(v) => match v.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(v),
        },
    })
}

fn read_parse<T: std::str::FromStr>(p: Option<&Path>) -> Option<T> {
    fs::read_to_string(p?).ok()?.trim().parse().ok()
}

/// hwmon `tempN_input` is millidegrees Celsius.
fn read_milli_c(p: Option<&Path>) -> Option<i32> {
    let milli: i64 = read_parse(p)?;
    Some((milli as f64 / 1000.0).round() as i32)
}

fn first_temp_input(hwmon_dir: &Path) -> Option<PathBuf> {
    for entry in fs::read_dir(hwmon_dir).ok()?.flatten() {
        let t = entry.path().join("temp1_input");
        if t.is_file() {
            return Some(t);
        }
    }
    None
}

/// Every `fanN_input` tach under `/sys/class/hwmon/*`.
fn fan_tachs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(hwmons) = fs::read_dir("/sys/class/hwmon") else {
        return out;
    };
    for hwmon in hwmons.flatten() {
        let Ok(entries) = fs::read_dir(hwmon.path()) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("fan") && name.ends_with("_input") {
                out.push(p);
            }
        }
    }
    out
}

fn hwmon_by_name(name: &str) -> Option<PathBuf> {
    for entry in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let dir = entry.path();
        if fs::read_to_string(dir.join("name"))
            .map(|s| s.trim() == name)
            .unwrap_or(false)
        {
            return Some(dir);
        }
    }
    None
}
