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
