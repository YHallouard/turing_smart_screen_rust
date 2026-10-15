//! Live system sensors for the dashboard scene.
//!
//! Pure sysfs / procfs / `nvidia-smi` / MangoHud-log reads, no library deps.
//! Every source is best-effort: if it is missing or unreadable the reading is
//! `None` and the dashboard shows a dash.
//!
//! - GPU: either the AMD render node exposing `gpu_busy_percent` (the BC-250 APU
//!   on the reference hardware; its `gpu_busy_percent` is bimodal noise, so it
//!   is burst-averaged and then EMA-smoothed) or, via `nvidia-smi`, an NVIDIA
//!   card. `[sensors] gpu = "auto" | "amd" | "nvidia"`. When
//!   `gpu_busy_percent` can't be read (EOPNOTSUPP on the BC-250) the load comes
//!   from the per-process DRM `fdinfo` engine counters instead.
//! - CPU: `/proc/stat` delta + `k10temp` (or `coretemp`).
//! - FPS: MangoHud's CSV log, when a game is logging.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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
    /// GPU load from DRM per-client counters, when `gpu_busy_percent` is
    /// unusable (the BC-250's amdgpu answers EOPNOTSUPP).
    fdinfo: Option<DrmFdinfo>,
    fan_inputs: Vec<PathBuf>,
    /// Parallel to `fan_inputs`: has this tach ever read > 0?
    fan_seen_spinning: Vec<bool>,
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

        let fdinfo = matches!(gpu, Gpu::Amd { busy: None, .. }).then(DrmFdinfo::new);
        if fdinfo.is_some() {
            log::info!("sensors: gpu_busy_percent unusable, GPU load from DRM fdinfo");
        }

        Self {
            gpu,
            fdinfo,
            gpu_ema: None,
            fan_seen_spinning: vec![false; fan_inputs.len()],
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
        let fdinfo_load = self.fdinfo.as_mut().and_then(DrmFdinfo::sample);
        let (gpu_load, gpu_temp_c, vram_used, vram_total) = match &self.gpu {
            Gpu::Amd {
                busy,
                temp,
                vram_used,
                vram_total,
            } => (
                busy.as_deref().and_then(burst_average).or(fdinfo_load),
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
            fan_rpm: self.fan_rpm(),
        }
    }

    /// Lowest RPM among the fans that have ever been seen spinning. Boards
    /// expose tachs for headers with nothing plugged in; those read 0 forever
    /// and must not count as a "stopped fan". A fan only counts once it has
    /// read > 0, so a real stop (spinning, then 0) is still reported.
    fn fan_rpm(&mut self) -> Option<u32> {
        let mut lowest: Option<u32> = None;
        for (path, seen) in self
            .fan_inputs
            .iter()
            .zip(self.fan_seen_spinning.iter_mut())
        {
            let Some(rpm) = read_parse::<u32>(Some(path)) else {
                continue;
            };
            if rpm > 0 {
                *seen = true;
            }
            if *seen {
                lowest = Some(lowest.map_or(rpm, |l| l.min(rpm)));
            }
        }
        lowest
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
            // The file can exist yet fail on read (EOPNOTSUPP on the BC-250).
            busy: fs::read_to_string(&busy).is_ok().then_some(busy),
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

/// GPU utilisation from the kernel's per-client DRM counters: every process
/// with a render node open exposes cumulative engine time in
/// `/proc/<pid>/fdinfo/<fd>` (`drm-engine-gfx: <ns> ns`, same source as nvtop /
/// amdgpu_top). Needs no root for our own processes (the games). Load =
/// delta of busy-ns over wall-ns between two samples.
struct DrmFdinfo {
    /// fdinfo files that pointed at `/dev/dri/*` at the last scan.
    files: Vec<PathBuf>,
    last_scan: Option<Instant>,
    /// client id -> cumulative busy ns at the previous sample.
    prev: HashMap<u64, u64>,
    prev_at: Option<Instant>,
}

/// Re-walk `/proc/*/fd` this often; in between only known fdinfo files are read.
const FDINFO_RESCAN: Duration = Duration::from_secs(10);

impl DrmFdinfo {
    fn new() -> Self {
        Self {
            files: Vec::new(),
            last_scan: None,
            prev: HashMap::new(),
            prev_at: None,
        }
    }

    fn scan(&mut self) {
        self.files.clear();
        let Ok(procs) = fs::read_dir("/proc") else {
            return;
        };
        for p in procs.flatten() {
            let name = p.file_name();
            let Some(pid) = name
                .to_str()
                .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
            else {
                continue;
            };
            let Ok(fds) = fs::read_dir(p.path().join("fd")) else {
                continue; // not our process
            };
            for fd in fds.flatten() {
                let is_drm = fs::read_link(fd.path()).is_ok_and(|t| t.starts_with("/dev/dri/"));
                if is_drm {
                    self.files.push(PathBuf::from(format!(
                        "/proc/{pid}/fdinfo/{}",
                        fd.file_name().to_string_lossy()
                    )));
                }
            }
        }
        self.last_scan = Some(Instant::now());
    }

    /// GPU load in percent since the previous call (`None` on the first one).
    fn sample(&mut self) -> Option<f32> {
        let now = Instant::now();
        if self
            .last_scan
            .map_or(true, |t| now.duration_since(t) >= FDINFO_RESCAN)
        {
            self.scan();
        }

        // client id -> busy ns; dedupes fds shared across dup()/fork().
        let mut cur: HashMap<u64, u64> = HashMap::new();
        let mut seen = HashSet::new();
        for f in &self.files {
            let Ok(text) = fs::read_to_string(f) else {
                continue;
            };
            let mut id = None;
            let mut busy = 0u64;
            for line in text.lines() {
                let Some((k, v)) = line.split_once(':') else {
                    continue;
                };
                let v = v.trim();
                match k {
                    "drm-client-id" => id = v.parse::<u64>().ok(),
                    "drm-engine-gfx" | "drm-engine-compute" => {
                        busy += v
                            .split_whitespace()
                            .next()
                            .and_then(|n| n.parse().ok())
                            .unwrap_or(0);
                    }
                    _ => {}
                }
            }
            if let Some(id) = id {
                if seen.insert(id) {
                    cur.insert(id, busy);
                }
            }
        }

        let load = self.prev_at.map(|t0| {
            let wall_ns = now.duration_since(t0).as_nanos().max(1) as f64;
            // Clients that appeared since the last sample have no baseline: skip them.
            let delta: u64 = cur
                .iter()
                .filter_map(|(id, ns)| self.prev.get(id).map(|p| ns.saturating_sub(*p)))
                .sum();
            (delta as f64 / wall_ns * 100.0).clamp(0.0, 100.0) as f32
        });
        self.prev = cur;
        self.prev_at = Some(now);
        load
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
    out.sort();
    for p in &out {
        log::info!("sensors: fan tach {}", p.display());
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
