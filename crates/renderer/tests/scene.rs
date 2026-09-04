use renderer::{Context, SceneEngine, SceneFile};
use std::path::Path;

const BOOT_TOML: &str = include_str!("../../../assets/scenes/boot.toml");

#[test]
fn boot_scene_parses() {
    let s = SceneFile::from_toml_str(BOOT_TOML).expect("boot.toml must parse");
    assert_eq!(s.scene.name, "boot");
    assert!(s.scene.duration > 0.0);
    assert!(s.layers.len() >= 4, "expected several layers");
}

#[test]
fn engine_renders_full_frame_then_dirty_subset() {
    let scene = SceneFile::from_toml_str(BOOT_TOML).unwrap();
    let mut engine = SceneEngine::new(320, 480);
    engine.load_assets(&scene, Path::new(".")).unwrap();

    let mut ctx = Context::new();
    ctx.set("status", "GPU ONLINE");
    ctx.set("boot.progress", "0.20");

    let (frame, dirty0) = engine.render(&scene, &ctx, 1.0);
    assert_eq!((frame.width(), frame.height()), (320, 480));
    assert_eq!(
        dirty0,
        vec![turzx::Rect::new(0, 0, 320, 480)],
        "first frame is fully dirty"
    );

    // A later time with a changed status line: something is dirty, but not all.
    ctx.set("status", "SYSTEM READY");
    let (_f, dirty1) = engine.render(&scene, &ctx, 5.0);
    assert!(!dirty1.is_empty(), "animated progress bar + status changed");
    let covered: u32 = dirty1.iter().map(|r| r.area()).sum();
    assert!(
        covered < 320 * 480,
        "dirty region should be a subset, got {covered}"
    );
}

const BOOT_PORTRAIT: &str = include_str!("../../../assets/scenes/boot.portrait.toml");
const BOOT_LANDSCAPE: &str = include_str!("../../../assets/scenes/boot.landscape.toml");
const DASH_PORTRAIT: &str = include_str!("../../../assets/scenes/dashboard.portrait.toml");
const DASH_LANDSCAPE: &str = include_str!("../../../assets/scenes/dashboard.landscape.toml");

/// The scene files reference `../logo/horus-seal-240.png`; resolve it from the
/// workspace root regardless of the test's working directory.
fn scenes_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenes")
}

#[test]
fn boot_scenes_are_minimal_motion() {
    for (src, w, h) in [(BOOT_PORTRAIT, 320u16, 480u16), (BOOT_LANDSCAPE, 480, 320)] {
        let scene = SceneFile::from_toml_str(src).expect("boot scene parses");
        assert!(scene.gradients.is_empty(), "flat colours, no gradients");

        let mut engine = SceneEngine::new(w, h);
        engine
            .load_assets(&scene, &scenes_dir())
            .expect("assets load (seal PNG decodes)");
        let ctx = Context::new();

        // Logo, name fade-in, hold — render at size and never panic.
        for &t in &[0.0f32, 0.6, 0.9, 1.5, 2.0, 3.0, 4.0] {
            let (frame, _d) = engine.render(&scene, &ctx, t);
            assert_eq!((frame.width(), frame.height()), (w, h));
        }

        // Frame 1 is fully dirty; the name fade touches a small subset only.
        let mut e2 = SceneEngine::new(w, h);
        e2.load_assets(&scene, &scenes_dir()).unwrap();
        let (_f, d0) = e2.render(&scene, &ctx, 0.0);
        assert_eq!(d0, vec![turzx::Rect::new(0, 0, w, h)], "first frame full");
        let (_f, d1) = e2.render(&scene, &ctx, 0.9); // mid "HORUS I" fade
        let covered: u32 = d1.iter().map(|r| r.area()).sum();
        assert!(
            !d1.is_empty() && covered < (w as u32 * h as u32) / 4,
            "name fade should touch a small subset, got {covered}"
        );

        // The held frame paints the seal over the near-black ground.
        let (frame, _) = engine.render(&scene, &ctx, 2.5);
        let lit = frame
            .as_rgba()
            .chunks_exact(4)
            .any(|p| p[0..3] != [0x05, 0x06, 0x0a]);
        assert!(lit, "held frame should show the seal");
    }
}

#[test]
fn dashboard_scenes_interpolate_sensor_values() {
    for (src, w, h) in [(DASH_PORTRAIT, 320u16, 480u16), (DASH_LANDSCAPE, 480, 320)] {
        let scene = SceneFile::from_toml_str(src).expect("dashboard scene parses");
        let mut engine = SceneEngine::new(w, h);
        engine.load_assets(&scene, &scenes_dir()).unwrap();

        let mut ctx = Context::new();
        for (k, v) in [
            ("gpu.pct", " 62%"),
            ("gpu.frac", "0.620"),
            ("cpu.pct", " 31%"),
            ("cpu.frac", "0.310"),
            ("vram", "0.5/2G"),
            ("vram.frac", "0.262"),
            ("fps", "  -"),
            ("fps.frac", "0"),
            ("gpu.temp", " 36\u{00B0}C"),
            ("cpu.temp", " 44\u{00B0}C"),
        ] {
            ctx.set(k, v);
        }

        // Frame 1 full; a later render with the *same* context is a no-op.
        let (frame, d0) = engine.render(&scene, &ctx, 0.0);
        assert_eq!((frame.width(), frame.height()), (w, h));
        assert_eq!(d0, vec![turzx::Rect::new(0, 0, w, h)]);
        let (_f, d1) = engine.render(&scene, &ctx, 0.0);
        assert!(
            d1.is_empty(),
            "unchanged context redraws nothing, got {d1:?}"
        );

        // Changing one value dirties only a small region.
        ctx.set("gpu.pct", " 99%");
        let (_f, d2) = engine.render(&scene, &ctx, 0.0);
        let covered: u32 = d2.iter().map(|r| r.area()).sum();
        assert!(
            !d2.is_empty() && covered < 4_000,
            "one value change should be tiny, got {covered}"
        );
    }
}

const ALERT_SCENES: &[(&str, u16, u16)] = &[
    (
        include_str!("../../../assets/scenes/alert.portrait.toml"),
        320,
        480,
    ),
    (
        include_str!("../../../assets/scenes/alert.landscape.toml"),
        480,
        320,
    ),
    (
        include_str!("../../../assets/scenes/notify.achievement.portrait.toml"),
        320,
        480,
    ),
    (
        include_str!("../../../assets/scenes/notify.achievement.landscape.toml"),
        480,
        320,
    ),
    (
        include_str!("../../../assets/scenes/launch.portrait.toml"),
        320,
        480,
    ),
    (
        include_str!("../../../assets/scenes/launch.landscape.toml"),
        480,
        320,
    ),
];

#[test]
fn alert_and_notify_scenes_render() {
    for &(src, w, h) in ALERT_SCENES {
        let scene = SceneFile::from_toml_str(src).expect("alert/notify scene parses");
        let mut engine = SceneEngine::new(w, h);
        engine
            .load_assets(&scene, &scenes_dir())
            .expect("assets load (achievement.png / stroke_rect)");
        // Alert / notify scenes read `{{ alert.* }}` / `{{ achievement.* }}`
        // from the daemon; provide representative values.
        let mut ctx = Context::new();
        for (k, v) in [
            ("alert.title", "! ALERTE THERMIQUE"),
            ("alert.value", "89\u{00B0}C"),
            ("alert.frac", "0.94"),
            ("achievement.name", "Sans Faute"),
            ("achievement.progress", "12 / 45 SUCCES"),
            ("achievement.frac", "0.27"),
            ("game.name", "Elden Ring"),
            ("game.subtitle", "12 / 45 SUCCES"),
            ("game.stat_frac", "0.27"),
        ] {
            ctx.set(k, v);
        }

        // Invisible at t=0, then rendered across the hold — must stay in size
        // and never panic (guards the thin-rect hairline path).
        for &t in &[0.0f32, 0.05, 1.0, 2.0, 5.0, 9.0] {
            let (frame, _d) = engine.render(&scene, &ctx, t);
            assert_eq!((frame.width(), frame.height()), (w, h));
        }

        // Frame 1 is fully dirty; well into the hold the scene has painted.
        let mut e2 = SceneEngine::new(w, h);
        e2.load_assets(&scene, &scenes_dir()).unwrap();
        let (_f, d0) = e2.render(&scene, &ctx, 0.0);
        assert_eq!(d0, vec![turzx::Rect::new(0, 0, w, h)]);
        let (held, _d) = e2.render(&scene, &ctx, 2.0);
        let lit = held
            .as_rgba()
            .chunks_exact(4)
            .any(|p| p[0..3] != [0x05, 0x06, 0x0a]);
        assert!(lit, "held frame should be painted");
    }
}

#[test]
fn max_width_truncates_long_bitmap_text() {
    let src = r##"
[scene]
name = "t"
duration = 1
[[layer]]
type = "text"
value = "{{ name }}"
x = 10
y = 40
scale = 4
max_width = 264
color = "#ffffff"
"##;
    let scene = SceneFile::from_toml_str(src).unwrap();
    let mut engine = SceneEngine::new(320, 480);
    engine.load_assets(&scene, Path::new(".")).unwrap();

    // `max_width = 264` at scale 4 (24 px/char) = 11 chars including "...".
    // A long name must not paint past the right edge.
    let mut ctx = Context::new();
    ctx.set("name", "CLAIR OBSCUR: EXPEDITION 33");
    let (frame, _d) = engine.render(&scene, &ctx, 0.0);
    let w = frame.width() as usize;
    let rgba = frame.as_rgba();
    let painted_past = (0..frame.height() as usize).any(|y| {
        (11 * 24 + 10..w).any(|x| {
            let p = &rgba[(y * w + x) * 4..][..3];
            p != [0, 0, 0]
        })
    });
    assert!(!painted_past, "text should be truncated inside max_width");

    // A short name is untouched.
    ctx.set("name", "DOOM");
    let (f2, _) = engine.render(&scene, &ctx, 0.0);
    assert!(f2.as_rgba().chunks_exact(4).any(|p| p[0] > 200));
}

#[test]
fn non_black_pixels_are_drawn() {
    let scene = SceneFile::from_toml_str(BOOT_TOML).unwrap();
    let mut engine = SceneEngine::new(320, 480);
    let ctx = Context::new();
    let (frame, _) = engine.render(&scene, &ctx, 3.0);
    let lit = frame
        .as_rgba()
        .chunks_exact(4)
        .any(|p| p[0..3] != [0x04, 0x07, 0x0d]);
    assert!(lit, "scene should paint something over the background");
}
