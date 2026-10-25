//! NeoBrawl (Unity 5.3, 2016) ported to Rust/WASM: scene graph, a small 2D
//! physics engine, uGUI layout, and the game's scripts. The host
//! (web/main.js) feeds input, calls `frame()` at 50 Hz and draws `render()`.

pub mod game;
pub mod motion;
pub mod physics;
pub mod prof;
pub mod render;
pub mod scene;
pub mod shards;
pub mod shared;
pub mod ui;

use std::collections::HashMap;

use wasm_bindgen::prelude::*;

use game::{v2, Game, Phase, Prefs, Stick, Touch};
use crate::shared::rng::Rng;
use ui::Screen;

#[wasm_bindgen]
pub struct Engine {
    game: Game,
    pointers: Vec<(i32, f32, f32, Phase)>,
    sticks: [Option<Stick>; 2],
    records: Vec<f32>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new(scene_json: &str, assets_json: &str, prefs_json: &str, seed: u64, width: f32, height: f32) -> Engine {
        let mut prefs = Prefs::default();
        if let Ok(serde_json::Value::Object(m)) = serde_json::from_str(prefs_json) {
            prefs.map = m.into_iter().collect();
        }
        let w = scene::World::load(scene_json, assets_json);
        let game = Game::new(w, Rng::new(seed), prefs, Screen { w: width.max(1.0), h: height.max(1.0) });
        Engine { game, pointers: Vec::new(), sticks: [None, None], records: Vec::new() }
    }

    /// Screen size in CSS pixels (Unity's Screen.width / height).
    pub fn set_screen(&mut self, width: f32, height: f32) {
        self.game.screen = Screen { w: width.max(1.0), h: height.max(1.0) };
    }

    /// Pointer in screen pixels, origin bottom-left. kind: 0 down, 1 move, 2 up.
    pub fn pointer(&mut self, id: i32, x: f32, y: f32, kind: u8) {
        match kind {
            0 => {
                self.pointers.retain(|p| p.0 != id);
                self.pointers.push((id, x, y, Phase::Began));
            }
            1 => {
                if let Some(p) = self.pointers.iter_mut().find(|p| p.0 == id) {
                    p.1 = x;
                    p.2 = y;
                    if p.3 != Phase::Began {
                        p.3 = Phase::Moved;
                    }
                }
            }
            _ => {
                if let Some(p) = self.pointers.iter_mut().find(|p| p.0 == id) {
                    p.1 = x;
                    p.2 = y;
                    p.3 = if p.3 == Phase::Began { Phase::Stationary } else { Phase::Ended };
                    if p.3 == Phase::Stationary {
                        // tapped within one frame: report Began now, Ended next frame
                        p.0 = -p.0 - 1_000_000;
                    }
                }
            }
        }
    }

    /// Keyboard / gamepad twin sticks for player 1 or 2 (values in -1..1).
    pub fn stick(&mut self, player: usize, mx: f32, my: f32, ax: f32, ay: f32, active: bool) {
        if player < 2 {
            self.sticks[player] = if active { Some(Stick { mv: v2(mx, my), aim: v2(ax, ay) }) } else { None };
        }
    }

    /// Desktop mode: players use `direct` controls; on-screen joysticks hide.
    /// AI temperament for a slot: 0 wanders, 1-3 fights (easy, normal, hard).
    pub fn set_ai_level(&mut self, slot: usize, level: u8) {
        if let Some(a) = self.game.ai.get_mut(slot) {
            a.level = level.min(3);
        }
    }

    /// Hand an input slot (0 = P1, 1 = P2) to the wandering AI, or back.
    pub fn set_ai(&mut self, slot: usize, on: bool) {
        if let Some(a) = self.game.ai.get_mut(slot) {
            a.on = on;
        }
    }

    /// Snake movement: you never stop; WASD / the stick set your heading.
    pub fn set_snake(&mut self, on: bool) {
        self.game.input.snake = on;
    }

    pub fn set_desktop(&mut self, on: bool) {
        self.game.input.desktop = on;
        if on {
            self.game.apply_desktop_copy();
        }
    }

    pub fn toggle_pause(&mut self) {
        self.game.toggle_pause();
    }

    /// Mouse cursor in screen pixels (origin bottom-left), or negative to clear.
    pub fn cursor(&mut self, x: f32, y: f32) {
        self.game.input.cursor = if x < 0.0 { None } else { Some([x, y]) };
    }

    /// Desktop controls for player 0/1: move (-1..1), aim stick, whether the
    /// throw is being charged, and whether to aim at the mouse cursor.
    #[allow(clippy::too_many_arguments)]
    pub fn direct(&mut self, player: usize, mx: f32, my: f32, ax: f32, ay: f32, aiming: bool, use_cursor: bool, active: bool) {
        if player < 2 {
            let dash = self.game.input.direct[player].dash;
            self.game.input.direct[player] = game::Direct { active, mv: v2(mx, my), aim: v2(ax, ay), aiming, use_cursor, dash };
        }
    }

    /// Dash button state for player 0/1 (Shift / controller A).
    pub fn dash(&mut self, player: usize, held: bool) {
        if player < 2 {
            self.game.input.direct[player].dash = held;
        }
    }

    pub fn any_key(&mut self, down: bool) {
        self.game.input.any_key = down;
    }

    pub fn frame(&mut self) {
        let mut touches: Vec<Touch> = self
            .pointers
            .iter()
            .map(|p| Touch {
                id: p.0,
                x: p.1,
                y: p.2,
                // a press released within one frame reports Began now, Ended next frame
                phase: if p.0 <= -1_000_000 && p.3 == Phase::Stationary { Phase::Began } else { p.3 },
            })
            .collect();
        if !self.game.input.desktop {
            touches.extend(self.synthetic_touches());
        }
        self.game.input.touches = touches;
        self.game.frame();
        // advance pointer phases
        let mut next = Vec::new();
        for p in self.pointers.drain(..) {
            match p.3 {
                Phase::Ended => {}
                Phase::Stationary if p.0 <= -1_000_000 => next.push((p.0, p.1, p.2, Phase::Ended)),
                Phase::Began | Phase::Moved => next.push((p.0, p.1, p.2, Phase::Stationary)),
                _ => next.push(p),
            }
        }
        self.pointers = next;
    }

    /// Sticks become touches on the original on-screen joystick positions so
    /// the touch code paths (movement magnitude, aim release = throw) apply.
    fn synthetic_touches(&self) -> Vec<Touch> {
        let (w, h) = (self.game.screen.w, self.game.screen.h);
        let tutorial = self.game.tutorial.as_ref().map(|t| self.game.w.active_in_hierarchy(t.node)).unwrap_or(false);
        let r = w / 20.0;
        let mut out = Vec::new();
        for (k, s) in self.sticks.iter().enumerate() {
            let Some(s) = s else { continue };
            let (mc, ac) = if tutorial {
                ((0.18 * w, 0.25 * h), (0.82 * w, 0.25 * h))
            } else if k == 0 {
                ((0.18 * w, 0.82 * h), (0.18 * w, 0.18 * h))
            } else {
                ((0.82 * w, 0.18 * h), (0.82 * w, 0.82 * h))
            };
            if tutorial && k == 1 {
                continue;
            }
            if s.mv.len() > 0.15 {
                out.push(Touch { id: 9000 + k as i32 * 2, x: mc.0 + s.mv.x * r, y: mc.1 + s.mv.y * r, phase: Phase::Moved });
            }
            if s.aim.len() > 0.15 {
                let a = s.aim.normalized();
                out.push(Touch { id: 9001 + k as i32 * 2, x: ac.0 + a.x * r, y: ac.1 + a.y * r, phase: Phase::Moved });
            }
        }
        out
    }

    /// Build draw records; returns their f32 count (read via records_ptr).
    /// How far (0..1 of a tick) the host is past the last fixed step, so
    /// motion graphics draw between ticks.
    pub fn set_subtick(&mut self, a: f32) {
        self.game.motion.render_now = self.game.motion.now + a.clamp(0.0, 1.0) * crate::game::DT;
    }

    /// Debug: shatter a box's worth of shards at a world point, flying
    /// back along (vx, vy), as if a throw at that speed hit something.
    #[doc(hidden)]
    pub fn debug_shatter(&mut self, x: f32, y: f32, vx: f32, vy: f32) {
        let sp = (vx * vx + vy * vy).sqrt();
        let z = self.game.players.first().map(|p| self.game.w.position(p.node).z).unwrap_or(0.0);
        self.game.shards.shatter(x, y, z, [vx, vy], sp, 14.0);
        self.game.fx_debug_impact(x, y, z);
    }

    /// Profile since the last call: [ticks, frames, (mean ms, worst ms) per
    /// phase in `phase_names()` order]. Resets.
    pub fn profile(&mut self) -> Vec<f32> {
        self.game.prof.borrow_mut().take()
    }

    pub fn phase_names(&self) -> String {
        serde_json::to_string(&crate::prof::PHASES).unwrap()
    }

    pub fn render(&mut self) -> usize {
        let mut out = std::mem::take(&mut self.records);
        self.game.render(&mut out);
        let n = out.len();
        self.records = out;
        n
    }

    pub fn records_ptr(&self) -> *const f32 {
        self.records.as_ptr()
    }

    /// Text strings for text records: {node: [text, style]}
    /// A checksum of the visible texts: the host refetches `texts()` only
    /// when this changes (it would otherwise parse them 50 times a second).
    pub fn texts_version(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for (i, n) in self.game.w.nodes.iter().enumerate() {
            if let Some(t) = &n.text {
                if !t.text.is_empty() && self.game.w.active_in_hierarchy(i) {
                    h = (h ^ i as u32).wrapping_mul(16777619);
                    for b in t.text.bytes() {
                        h = (h ^ b as u32).wrapping_mul(16777619);
                    }
                    h = (h ^ t.style as u32).wrapping_mul(16777619);
                }
            }
        }
        h
    }

    pub fn texts(&self) -> String {
        let mut m = serde_json::Map::new();
        for (i, n) in self.game.w.nodes.iter().enumerate() {
            if let Some(t) = &n.text {
                if !t.text.is_empty() && self.game.w.active_in_hierarchy(i) {
                    m.insert(i.to_string(), serde_json::json!([t.text, t.style]));
                }
            }
        }
        serde_json::Value::Object(m).to_string()
    }

    /// {textures: [file], meshes: [key], materials: [[key, [r,g,b,a]]], font}
    pub fn tables(&self) -> String {
        let w = &self.game.w;
        serde_json::json!({
            "textures": w.textures.iter().map(|t| &t.1).collect::<Vec<_>>(),
            "meshes": w.meshes,
            "materials": w.materials.iter().map(|m| serde_json::json!([m.0, m.1])).collect::<Vec<_>>(),
            "font": w.font,
        })
        .to_string()
    }

    /// One-shot sounds since the last call: [[clip, volume]]
    pub fn drain_sounds(&mut self) -> String {
        let s = serde_json::to_string(&self.game.sounds).unwrap();
        self.game.sounds.clear();
        s
    }

    /// Synthesised cues since the last call: [[name, pan, amount]]
    pub fn drain_cues(&mut self) -> String {
        let s = serde_json::to_string(&self.game.cues).unwrap();
        self.game.cues.clear();
        s
    }

    /// Continuous mix inputs, every frame: [charge p0, pan p0, charge p1,
    /// pan p1, lull 0..1, paused 0/1, in match 0/1]
    pub fn mix(&self) -> Vec<f32> {
        let g = &self.game;
        let mut out = Vec::new();
        let cam = g.camera_main().map(|c| g.w.position(c).x).unwrap_or(0.0);
        for k in 0..2 {
            match g.players.iter().filter(|p| !p.playback && !p.single).nth(k) {
                Some(p) if g.w.active_in_hierarchy(p.node) && p.picked_box.is_some() => {
                    out.push((p.charge_timer / 90.0).clamp(0.0, 1.0));
                    out.push(((g.w.position(p.node).x - cam) / 9.0).clamp(-1.0, 1.0));
                }
                _ => {
                    out.push(0.0);
                    out.push(0.0);
                }
            }
        }
        let in_match = g.players.iter().any(|p| !p.single && !p.playback && g.w.active_in_hierarchy(p.node));
        let lull = if in_match { ((g.motion.now - g.motion.last_action - 1.5) / 2.0).clamp(0.0, 1.0) } else { 0.0 };
        let paused = g.paused;
        out.extend([lull, paused as i32 as f32, in_match as i32 as f32]);
        out
    }

    /// Music sources: [[clip, playing, volume]] (muted when Music is OFF)
    pub fn music(&self) -> String {
        let on = self.game.prefs.get_string("Music") == "ON";
        let v: Vec<_> = self.game.music.iter().map(|m| serde_json::json!([m.clip, m.playing && on, m.volume])).collect();
        serde_json::to_string(&v).unwrap()
    }

    pub fn take_prefs(&mut self) -> String {
        if !self.game.prefs.dirty {
            return String::new();
        }
        self.game.prefs.dirty = false;
        serde_json::to_string(&self.game.prefs.map).unwrap()
    }

    pub fn state(&self) -> String {
        self.game.debug_state().to_string()
    }

    /// Screen-space quad of the first active node with this name (for tests/automation).
    pub fn locate(&self, name: &str) -> Vec<f32> {
        let w = &self.game.w;
        for i in w.find_any(name) {
            if w.active_in_hierarchy(i) {
                if let Some(q) = self.game.ui_screen_quad(i) {
                    let cx = (q[0][0] + q[2][0]) / 2.0;
                    let cy = (q[0][1] + q[2][1]) / 2.0;
                    return vec![cx, cy];
                }
            }
        }
        Vec::new()
    }
}

impl Engine {
    pub fn game(&self) -> &Game {
        &self.game
    }
    pub fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }
    pub fn render_vec(&mut self) -> Vec<f32> {
        let n = self.render();
        self.records[..n].to_vec()
    }
}

#[allow(dead_code)]
fn _unused(_: HashMap<u8, u8>) {}
