//! Local Steam integration (option B): detect the running game and watch its
//! achievement state on disk — no Steamworks link, no Web API key, no network.
//!
//! - Running game: `SteamAppId=` in `/proc/<pid>/environ` (same-uid processes),
//!   resolved to a name via `steamapps/appmanifest_<id>.acf`.
//! - Achievements: `<root>/appcache/stats/UserGameStats_<account>_<appid>.bin`
//!   (Steam's binary-KeyValues cache) for the unlocked set + timestamps, and
//!   `UserGameStatsSchema_<appid>.bin` for the display names. Re-read when the
//!   stats file's mtime changes; a `(set,index)` that wasn't unlocked before is
//!   a fresh unlock.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Game {
    pub appid: u32,
    pub name: String,
}

/// One newly-unlocked achievement, for the notify scene.
#[derive(Debug, Clone)]
pub struct Unlock {
    pub game: String,
    pub name: String,
    pub unlocked: u32,
    pub total: u32,
}

/// A game that just started, for the launch scene.
#[derive(Debug, Clone)]
pub struct Launch {
    pub appid: u32,
    pub name: String,
    pub unlocked: u32,
    pub total: u32,
}

pub struct Steam {
    root: PathBuf,
    account: u32,
    game: Option<Game>,
    /// (set, index) -> unlock time, for the current game.
    unlocked: HashMap<(u32, u32), i64>,
    /// (set, index) -> display name, from the schema.
    names: HashMap<(u32, u32), String>,
    total: u32,
    stats_mtime: Option<SystemTime>,
    /// Set by `switch_game` after the first poll; drained by `take_launch`.
    pending_launch: Option<Launch>,
    started: bool,
}

impl Steam {
    /// `None` when no local Steam install is found.
    pub fn detect() -> Option<Self> {
        let root = steam_root()?;
        let account = first_account(&root)?;
        log::info!("steam: {} (account {account})", root.display());
        Some(Self {
            root,
            account,
            game: None,
            unlocked: HashMap::new(),
            names: HashMap::new(),
            total: 0,
            stats_mtime: None,
            pending_launch: None,
            started: false,
        })
    }

    pub fn current_game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    /// Cover art Steam itself already cached for this app (its library grid
    /// image, or the wide store header as a fallback), if present on disk —
    /// no network, same source Steam's own UI reads from.
    pub fn cover_path(&self, appid: u32) -> Option<PathBuf> {
        let dir = self
            .root
            .join("appcache/librarycache")
            .join(appid.to_string());
        ["library_600x900.jpg", "header.jpg"]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file())
    }

    /// A game that started since the last call (`None` on the daemon's first
    /// poll even if a game is already running).
    pub fn take_launch(&mut self) -> Option<Launch> {
        self.pending_launch.take()
    }

    /// Poll once. Returns achievements unlocked since the previous poll.
    pub fn poll(&mut self) -> Vec<Unlock> {
        let appid = running_appid();

        if appid != self.game.as_ref().map(|g| g.appid) {
            match appid {
                Some(id) => {
                    self.switch_game(id);
                    if self.started {
                        self.pending_launch = Some(Launch {
                            appid: id,
                            name: self.game.as_ref().unwrap().name.clone(),
                            unlocked: self.unlocked.len() as u32,
                            total: self.total,
                        });
                    }
                }
                None => {
                    self.game = None;
                    self.unlocked.clear();
                    self.names.clear();
                    self.stats_mtime = None;
                }
            }
            self.started = true;
            return Vec::new(); // don't fire achievements for a game already ahead
        }
        self.started = true;

        let Some(game) = &self.game else {
            return Vec::new();
        };
        let stats = self
            .root
            .join("appcache/stats")
            .join(format!("UserGameStats_{}_{}.bin", self.account, game.appid));
        let mtime = fs::metadata(&stats).and_then(|m| m.modified()).ok();
        if mtime == self.stats_mtime {
            return Vec::new();
        }
        self.stats_mtime = mtime;

        let fresh = parse_unlocked(&stats);
        let mut out = Vec::new();
        for (&key, &time) in &fresh {
            if !self.unlocked.contains_key(&key) {
                out.push(Unlock {
                    game: game.name.clone(),
                    name: self
                        .names
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| format!("Achievement {}", key.1)),
                    unlocked: fresh.len() as u32,
                    total: self.total.max(fresh.len() as u32),
                });
                let _ = time;
            }
        }
        self.unlocked = fresh;
        out
    }

    fn switch_game(&mut self, appid: u32) {
        let name = game_name(&self.root, appid).unwrap_or_else(|| format!("App {appid}"));
        log::info!("steam: game -> {name} ({appid})");
        let schema = self
            .root
            .join("appcache/stats")
            .join(format!("UserGameStatsSchema_{appid}.bin"));
        self.names = parse_schema_names(&schema);
        self.total = self.names.len() as u32;
        let stats = self
            .root
            .join("appcache/stats")
            .join(format!("UserGameStats_{}_{}.bin", self.account, appid));
        self.stats_mtime = fs::metadata(&stats).and_then(|m| m.modified()).ok();
        self.unlocked = parse_unlocked(&stats); // seed: current unlocks are not "new"
        self.game = Some(Game { appid, name });
    }
}

// ---- process / manifest -----------------------------------------------------

fn running_appid() -> Option<u32> {
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let Some(s) = name.to_str() else { continue };
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(environ) = fs::read(entry.path().join("environ")) else {
            continue;
        };
        for kv in environ.split(|&b| b == 0) {
            if let Some(v) = kv.strip_prefix(b"SteamAppId=") {
                if let Ok(id) = std::str::from_utf8(v).unwrap_or("").parse::<u32>() {
                    if id != 0 {
                        return Some(id);
                    }
                }
            }
        }
    }
    None
}

fn steam_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    for cand in [
        ".steam/steam",
        ".steam/debian-installation",
        ".steam/root",
        ".local/share/Steam",
        ".var/app/com.valvesoftware.Steam/data/Steam",
        "snap/steam/common/.local/share/Steam",
    ] {
        let p = home.join(cand);
        let p = fs::canonicalize(&p).unwrap_or(p);
        if p.join("appcache/stats").is_dir() {
            return Some(p);
        }
    }
    None
}

fn first_account(root: &Path) -> Option<u32> {
    let mut ids: Vec<u32> = fs::read_dir(root.join("userdata"))
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    ids.sort_unstable();
    ids.into_iter().next()
}

/// `\t"key"\t\t"value"` -> `Some("value")`.
fn vdf_pair<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut parts = line.split('"');
    parts.next()?; // leading indent
    (parts.next()? == key).then_some(())?;
    parts.next()?; // gap between key and value
    parts.next()
}

fn game_name(root: &Path, appid: u32) -> Option<String> {
    let mut libs = vec![root.join("steamapps")];
    if let Ok(txt) = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
        for line in txt.lines() {
            if let Some(path) = vdf_pair(line, "path") {
                libs.push(PathBuf::from(path.replace("\\\\", "/")).join("steamapps"));
            }
        }
    }
    for lib in libs {
        let acf = lib.join(format!("appmanifest_{appid}.acf"));
        if let Ok(txt) = fs::read_to_string(&acf) {
            for line in txt.lines() {
                if let Some(name) = vdf_pair(line, "name") {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

// ---- binary KeyValues -----------------------------------------------------

/// Minimal Steam binary-KV node. Only what the two stats files need.
enum Kv {
    Obj(Vec<(String, Kv)>),
    Str(String),
    Int(i32),
    Other,
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Obj(v) => v.iter().find(|(k, _)| k == key).map(|(_, val)| val),
            _ => None,
        }
    }
    fn children(&self) -> &[(String, Kv)] {
        match self {
            Kv::Obj(v) => v,
            _ => &[],
        }
    }
    fn as_int(&self) -> Option<i32> {
        match self {
            Kv::Int(i) => Some(*i),
            _ => None,
        }
    }
    fn as_str(&self) -> Option<&str> {
        match self {
            Kv::Str(s) => Some(s),
            _ => None,
        }
    }
}

fn parse_bkv(path: &Path) -> Option<Kv> {
    let data = fs::read(path).ok()?;
    let mut pos = 0usize;
    let node = read_obj(&data, &mut pos)?;
    Some(node)
}

fn read_cstr(d: &[u8], pos: &mut usize) -> Option<String> {
    let start = *pos;
    let end = d[start..].iter().position(|&b| b == 0)? + start;
    *pos = end + 1;
    Some(String::from_utf8_lossy(&d[start..end]).into_owned())
}

fn read_obj(d: &[u8], pos: &mut usize) -> Option<Kv> {
    let mut out = Vec::new();
    loop {
        let t = *d.get(*pos)?;
        *pos += 1;
        if t == 0x08 {
            return Some(Kv::Obj(out));
        }
        let key = read_cstr(d, pos)?;
        let val = match t {
            0x00 => read_obj(d, pos)?,
            0x01 => Kv::Str(read_cstr(d, pos)?),
            0x02 | 0x06 | 0x04 => {
                let v = i32::from_le_bytes(d.get(*pos..*pos + 4)?.try_into().ok()?);
                *pos += 4;
                Kv::Int(v)
            }
            0x03 => {
                *pos += 4;
                Kv::Other
            }
            0x07 | 0x0A => {
                *pos += 8;
                Kv::Other
            }
            0x05 => {
                // wide string: UTF-16 until 00 00
                while d.get(*pos..*pos + 2)? != [0, 0] {
                    *pos += 2;
                }
                *pos += 2;
                Kv::Other
            }
            _ => return None,
        };
        out.push((key, val));
    }
}

/// From `UserGameStats_*.bin`: `(set, index) -> unlock unix time`.
fn parse_unlocked(path: &Path) -> HashMap<(u32, u32), i64> {
    let mut out = HashMap::new();
    let Some(root) = parse_bkv(path) else {
        return out;
    };
    // cache { "<set>" { AchievementTimes { "<idx>" = <ts> } } }
    let cache = root.get("cache").unwrap_or(&root);
    for (set_key, set) in cache.children() {
        let Ok(set_id) = set_key.parse::<u32>() else {
            continue;
        };
        let Some(times) = set.get("AchievementTimes") else {
            continue;
        };
        for (idx_key, ts) in times.children() {
            if let (Ok(idx), Some(t)) = (idx_key.parse::<u32>(), ts.as_int()) {
                if t > 0 {
                    out.insert((set_id, idx), t as i64);
                }
            }
        }
    }
    out
}

/// From `UserGameStatsSchema_*.bin`: `(set, index) -> display name`.
fn parse_schema_names(path: &Path) -> HashMap<(u32, u32), String> {
    let mut out = HashMap::new();
    let Some(root) = parse_bkv(path) else {
        return out;
    };
    // <appid> { stats { "<set>" { bits { "<idx>" { display { name { english=.. french=.. } } } } } } }
    let app = root.children().first().map(|(_, v)| v).unwrap_or(&root);
    let Some(stats) = app.get("stats") else {
        return out;
    };
    for (set_key, set) in stats.children() {
        let Ok(set_id) = set_key.parse::<u32>() else {
            continue;
        };
        let Some(bits) = set.get("bits") else {
            continue;
        };
        for (idx_key, bit) in bits.children() {
            let Ok(idx) = idx_key.parse::<u32>() else {
                continue;
            };
            let name = bit
                .get("display")
                .and_then(|d| d.get("name"))
                .and_then(|n| {
                    n.get("french")
                        .or_else(|| n.get("english"))
                        .or_else(|| n.children().first().map(|(_, v)| v))
                })
                .and_then(Kv::as_str)
                .or_else(|| bit.get("name").and_then(Kv::as_str))
                .unwrap_or("")
                .to_string();
            out.insert((set_id, idx), name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse whatever real Steam stats files are on this machine (skips on CI /
    /// no Steam). Catches format regressions against the actual binary layout.
    #[test]
    fn parses_local_steam_stats() {
        let Some(root) = steam_root() else {
            eprintln!("no local Steam — skipping");
            return;
        };
        let dir = root.join("appcache/stats");
        let mut checked = 0;
        for e in fs::read_dir(&dir).unwrap().flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            let Some(appid) = n
                .strip_prefix("UserGameStatsSchema_")
                .and_then(|s| s.strip_suffix(".bin"))
            else {
                continue;
            };
            let names = parse_schema_names(&e.path());
            let stats = dir.join(format!(
                "UserGameStats_{}_{appid}.bin",
                first_account(&root).unwrap()
            ));
            let unlocked = parse_unlocked(&stats);
            // Every unlocked (set,index) should resolve to a schema entry.
            for key in unlocked.keys() {
                assert!(
                    names.contains_key(key) || names.is_empty(),
                    "app {appid}: unlocked {key:?} not in schema"
                );
            }
            if !names.is_empty() {
                checked += 1;
            }
        }
        eprintln!("parsed {checked} Steam schemas");
        assert!(checked > 0, "no schema parsed — parser likely broken");
    }
}
