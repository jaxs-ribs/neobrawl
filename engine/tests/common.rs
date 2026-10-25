#![allow(dead_code)]
use neobrawl::Engine;

pub fn engine(prefs: &str, seed: u64) -> Engine {
    let d = format!("{}/../web/assets", env!("CARGO_MANIFEST_DIR"));
    let read = |f: &str| std::fs::read_to_string(format!("{d}/{f}")).unwrap();
    Engine::new(&read("scene.json"), &read("assets.json"), prefs, seed, 1280.0, 720.0)
}

pub fn state(e: &Engine) -> serde_json::Value {
    serde_json::from_str(&e.state()).unwrap()
}

pub fn run(e: &mut Engine, n: usize) {
    for _ in 0..n {
        e.frame();
    }
}

/// Tap the named UI element; panics if it isn't on screen.
pub fn tap(e: &mut Engine, name: &str) {
    let p = e.locate(name);
    assert!(p.len() == 2, "{name} not on screen; state {}", e.state());
    e.pointer(1, p[0], p[1], 0);
    e.frame();
    e.pointer(1, p[0], p[1], 2);
    e.frame();
}
