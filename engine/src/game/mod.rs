//! NeoBrawl's single scene: menu, intro fights, tutorial and the two-players
//! one-screen match. Each MonoBehaviour is ported as a state struct plus
//! methods on `Game`; coroutines become `Task`s stepped once per FixedUpdate.

mod ai;
mod body;
mod boxes;
mod fx;
mod matchplay;
mod menu;
mod players;
mod tasks;

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use crate::shared::math::{v3, V3};
use crate::shared::rng::Rng;

use crate::physics::Physics;
use crate::scene::World;
use crate::ui::{Layout, Screen};

pub use players::{Player, DASH_COOLDOWN, DASH_SPEED, DASH_TICKS};
pub use tasks::Task;

pub const DT: f32 = 0.02;

pub fn ticks(seconds: f32) -> u32 {
    (seconds / DT).round() as u32
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

pub const fn v2(x: f32, y: f32) -> V2 {
    V2 { x, y }
}

impl V2 {
    pub const ZERO: V2 = v2(0.0, 0.0);
    pub fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    pub fn normalized(self) -> V2 {
        let l = self.len();
        if l > 1e-5 {
            v2(self.x / l, self.y / l)
        } else {
            V2::ZERO
        }
    }
    pub fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Began,
    Moved,
    Stationary,
    Ended,
}

#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub id: i32,
    pub x: f32,
    pub y: f32,
    pub phase: Phase,
}

/// A keyboard / gamepad twin stick for one player (web addition; the
/// original only had touch). `aim` is zero while the aim stick is released.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stick {
    pub mv: V2,
    pub aim: V2,
}

/// Desktop control for one player (web addition): keyboard/stick movement,
/// and either mouse-cursor aiming or a gamepad aim stick. `aiming` is true
/// while the throw is being charged (mouse button / stick held).
#[derive(Clone, Copy, Debug, Default)]
pub struct Direct {
    pub active: bool,
    pub mv: V2,
    pub aim: V2,
    pub aiming: bool,
    pub use_cursor: bool,
    pub dash: bool,
}

#[derive(Default, Clone)]
pub struct Input {
    pub touches: Vec<Touch>,
    pub sticks: [Option<Stick>; 2],
    pub any_key: bool,
    /// desktop mode: players are driven by `direct`, not by touch joysticks
    pub desktop: bool,
    pub direct: [Direct; 2],
    /// mouse cursor in screen pixels (origin bottom-left)
    pub cursor: Option<[f32; 2]>,
    /// snake movement: always moving, keys turn you
    pub snake: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Prefs {
    pub map: BTreeMap<String, Value>,
    pub dirty: bool,
}

impl Prefs {
    pub fn get_string(&self, k: &str) -> String {
        self.map.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
    }
    pub fn set_string(&mut self, k: &str, v: &str) {
        self.map.insert(k.into(), Value::String(v.into()));
        self.dirty = true;
    }
    pub fn get_int(&self, k: &str) -> i64 {
        self.map.get(k).and_then(|v| v.as_i64()).unwrap_or(0)
    }
    pub fn set_int(&mut self, k: &str, v: i64) {
        self.map.insert(k.into(), Value::from(v));
        self.dirty = true;
    }
}

#[derive(Clone, Debug)]
pub struct MusicSource {
    pub node: usize,
    pub clip: String,
    pub playing: bool,
    pub volume: f32,
}

pub struct Game {
    pub w: World,
    pub phys: Physics,
    pub layout: Layout,
    pub screen: Screen,
    pub rng: Rng,
    pub prefs: Prefs,
    pub input: Input,
    pub sounds: Vec<(String, f32)>,
    pub music: Vec<MusicSource>,
    pub tasks: Vec<Task>,
    pub frame: u64,

    pub players: Vec<Player>,
    pub boxes: Vec<boxes::BoxS>,
    pub box_of: HashMap<usize, usize>,
    pub player_of: HashMap<usize, usize>,
    pub deaths: Vec<matchplay::DeathMgr>,
    pub scores: Vec<matchplay::ScoreMgr>,
    pub cam_follow: Vec<matchplay::CamFollow>,
    pub blobs: Vec<matchplay::Blob>,
    pub showers: Vec<matchplay::Shower>,
    pub charge_bars: Vec<(usize, usize)>,
    pub pauses: Vec<matchplay::Pause>,
    pub burners: Vec<players::Burner>,
    pub menu: menu::Menu,
    pub tutorial: Option<menu::Tutorial>,
    pub misc: menu::Misc,
    /// pointer id -> button node pressed on pointer down
    pub pressed: HashMap<i32, usize>,
    /// script components whose Start() already ran
    pub started: std::collections::HashSet<(usize, &'static str)>,
    pub was_active: HashMap<usize, bool>,
    pub motion: crate::motion::Motion,
    /// velocity of the box that made the last kill (the orbs fly with it)
    pub kill_impulse: V3,
    /// positions of moving things at the start of the current tick; the
    /// renderer draws between this and now so 120 Hz frames don't judder
    pub prev_pos: HashMap<usize, V3>,
    /// Shard Lab debris and electricity
    pub shards: crate::shards::ShardSim,
    /// soft-body state per player (render-only springs)
    pub bodies: Vec<body::SoulBody>,
    /// wandering AI per input slot
    pub ai: [ai::Wanderer; 2],
    /// synthesised sound cues for the host: (name, pan -1..1, amount)
    pub cues: Vec<(&'static str, f32, f32)>,
    /// where thrown boxes end, per thrower node: [soul, box, other] (tuning)
    pub throw_ends: HashMap<usize, [u32; 3]>,
    /// who ended each soul last (for choosing its respawn)
    pub last_killer: HashMap<usize, usize>,
    /// the match is frozen under the pause menu (the original only froze
    /// time in the single-player tutorial; versus kept running under it)
    pub paused: bool,
    /// per-phase timings (RefCell: render takes &self)
    pub prof: std::cell::RefCell<crate::prof::Prof>,
}

pub fn field<'a>(w: &'a World, node: usize, script: &str, name: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    w.nodes[node].scripts.get(script).and_then(|s| s.get(name)).unwrap_or(&NULL)
}

pub fn fbool(w: &World, node: usize, script: &str, name: &str) -> bool {
    field(w, node, script, name).as_bool().unwrap_or(false)
}

pub fn fref(w: &World, node: usize, script: &str, name: &str) -> Option<usize> {
    w.obj(field(w, node, script, name))
}

pub fn has_script(w: &World, node: usize, script: &str) -> bool {
    w.nodes[node].scripts.contains_key(script)
}

impl Game {
    pub fn new(w: World, rng: Rng, prefs: Prefs, screen: Screen) -> Game {
        let mut g = Game {
            w,
            phys: Physics::default(),
            layout: Layout::default(),
            screen,
            rng,
            prefs,
            input: Input::default(),
            sounds: Vec::new(),
            music: Vec::new(),
            tasks: Vec::new(),
            frame: 0,
            players: Vec::new(),
            boxes: Vec::new(),
            box_of: HashMap::new(),
            player_of: HashMap::new(),
            deaths: Vec::new(),
            scores: Vec::new(),
            cam_follow: Vec::new(),
            blobs: Vec::new(),
            showers: Vec::new(),
            charge_bars: Vec::new(),
            pauses: Vec::new(),
            burners: Vec::new(),
            menu: menu::Menu::default(),
            tutorial: None,
            misc: menu::Misc::default(),
            pressed: HashMap::new(),
            started: Default::default(),
            was_active: HashMap::new(),
            motion: Default::default(),
            kill_impulse: V3::ZERO,
            prev_pos: HashMap::new(),
            shards: crate::shards::ShardSim::new(0x5eed),
            bodies: Vec::new(),
            ai: Default::default(),
            cues: Vec::new(),
            throw_ends: HashMap::new(),
            last_killer: HashMap::new(),
            paused: false,
            prof: Default::default(),
        };
        for i in 0..g.w.nodes.len() {
            if let Some(clip) = g.w.nodes[i].audio.clone() {
                g.music.push(MusicSource { node: i, clip, playing: false, volume: 0.3 });
            }
        }
        g.collect_scripts();
        g.relayout();
        g.menu_start();
        g.run_enables();
        g
    }

    pub fn relayout(&mut self) {
        let cam = |w: &World, c: usize| w.nodes[c].camera.as_ref().map(|k| (w.matrix(c), k.fov));
        self.layout = Layout::compute(&mut self.w, self.screen, cam);
    }

    /// One Unity frame at the fixed 50 Hz step.
    pub fn frame(&mut self) {
        if self.paused {
            // frozen: only the pause menu's own fades, the menu and the
            // motion graphics keep going
            self.step_tasks_where(|t| matches!(t, Task::PauseFade { .. } | Task::FadeSound { .. } | Task::DelayedQuit { .. }));
            self.relayout();
            self.ui_events();
            self.fx_step();
            return;
        }
        if self.motion.hitstop > 0 {
            // the world holds still for a beat; only the motion graphics move
            self.motion.hitstop -= 1;
            self.fx_step();
            return;
        }
        let lap = |g: &Game, i: usize| g.prof.borrow_mut().lap(i);
        self.prof.borrow_mut().begin();
        self.snapshot_movers();
        self.ai_step();
        lap(self, 0);
        self.frame += 1;
        self.run_enables();
        lap(self, 1);
        self.fixed_update();
        lap(self, 2);
        self.phys.step(&mut self.w, DT);
        self.dispatch_physics_events();
        lap(self, 3);
        self.shards_step();
        self.shard_cues();
        lap(self, 4);
        self.step_tasks();
        lap(self, 5);
        self.update();
        lap(self, 6);
        self.relayout();
        self.ui_events();
        lap(self, 7);
        self.bodies_step();
        lap(self, 8);
        self.fx_step();
        lap(self, 9);
        self.prof.borrow_mut().end(0..10, true);
    }

    /// Step the debris; shards bounce off the arena's solid shapes.
    fn shards_step(&mut self) {
        if self.shards.shards.is_empty() {
            self.shards.step(|_, _, _, _| None);
            return;
        }
        let w = &self.w;
        // static solids near any shard, with a bounding radius for a cheap reject
        let mask = 1u32 | (1 << 8) | (1 << crate::physics::LAYER_BOX);
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for s in self.shards.shards.iter().filter(|s| !s.dead) {
            lo = [lo[0].min(s.x), lo[1].min(s.y)];
            hi = [hi[0].max(s.x), hi[1].max(s.y)];
        }
        let mut geos = Vec::new();
        for i in 0..w.nodes.len() {
            let Some(c) = w.nodes[i].collider.as_ref() else { continue };
            if !c.enabled || c.trigger || mask & (1 << w.nodes[i].layer) == 0 || !w.active_in_hierarchy(i) {
                continue;
            }
            let Some(g) = crate::physics::geometry(w, i) else { continue };
            let (cx, cy, r) = match &g {
                crate::physics::Geo::Circle { c, r } => (c[0], c[1], *r),
                crate::physics::Geo::Obb { c, h, .. } => (c[0], c[1], (h[0] * h[0] + h[1] * h[1]).sqrt()),
                crate::physics::Geo::Poly { n, p } => {
                    let n = *n as usize;
                    let (sx, sy) = p[..n].iter().fold((0.0, 0.0), |a, q| (a.0 + q[0], a.1 + q[1]));
                    let (cx, cy) = (sx / n as f32, sy / n as f32);
                    let r = p[..n].iter().map(|q| ((q[0] - cx).powi(2) + (q[1] - cy).powi(2)).sqrt()).fold(0.0f32, f32::max);
                    (cx, cy, r)
                }
            };
            if cx + r < lo[0] - 2.0 || cx - r > hi[0] + 2.0 || cy + r < lo[1] - 2.0 || cy - r > hi[1] + 2.0 {
                continue;
            }
            geos.push((g, cx, cy, r));
        }
        self.shards.step(|x0, y0, x1, y1| {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 {
                return None;
            }
            let d = [dx / len, dy / len];
            let mut best: Option<f32> = None;
            for (g, cx, cy, r) in &geos {
                // segment vs bounding circle first
                let (ox, oy) = (cx - x0, cy - y0);
                let along = (ox * d[0] + oy * d[1]).clamp(0.0, len);
                let (px, py) = (x0 + d[0] * along - cx, y0 + d[1] * along - cy);
                if px * px + py * py > r * r {
                    continue;
                }
                if let Some(t) = crate::physics::ray_geo([x0, y0], d, g) {
                    if t <= len && best.map(|b| t / len < b).unwrap_or(true) {
                        best = Some(t / len);
                    }
                }
            }
            best
        });
    }

    fn shard_cues(&mut self) {
        let ev = std::mem::take(&mut self.shards.events);
        let z = self.players.first().map(|p| self.w.position(p.node).z).unwrap_or(0.0);
        for (kind, x, y, s) in ev {
            self.cue(if kind == 0 { "spark" } else { "bolt" }, v3(x, y, z), s);
        }
    }

    fn snapshot_movers(&mut self) {
        self.prev_pos.clear();
        let mut movers: Vec<usize> = self.players.iter().map(|p| p.node).collect();
        movers.extend(self.cam_follow.iter().map(|c| c.node));
        movers.extend(self.boxes.iter().map(|b| b.node).filter(|&n| self.w.nodes[n].body.as_ref().map(|b| b.constraints != crate::scene::FREEZE_ALL).unwrap_or(false)));
        for n in movers {
            if self.w.active_in_hierarchy(n) {
                self.prev_pos.insert(n, self.w.position(n));
            }
        }
    }

    /// Render-time offset for a node: where it was a fraction of a tick ago.
    pub fn interp_offsets(&self) -> HashMap<usize, V3> {
        let a = ((self.motion.render_now - self.motion.now) / DT).clamp(0.0, 1.0);
        let mut out = HashMap::new();
        if self.motion.hitstop > 0 || self.paused {
            return out;
        }
        // souls under a human's hands are drawn ahead (extrapolated), so input
        // shows up in the very next frame; everything else is interpolated
        let human: Vec<usize> = self
            .players
            .iter()
            .filter(|p| self.input.desktop && self.direct_slot_pub(p).map(|s| !self.ai[s].on).unwrap_or(false))
            .map(|p| p.node)
            .collect();
        for (&n, &prev) in &self.prev_pos {
            let cur = self.w.position(n);
            let d = cur - prev;
            if d.length() > 3.0 || prev.x == -666.0 || cur.x == -666.0 {
                continue; // teleports snap
            }
            let off = if human.contains(&n) { d * a } else { d * (a - 1.0) };
            let mut sub = Vec::new();
            self.w.descendants(n, &mut sub);
            for s in sub {
                out.insert(s, off);
            }
        }
        out
    }

    /// A synthesised sound at a world position (panned by the main camera).
    pub fn cue(&mut self, name: &'static str, at: V3, amount: f32) {
        if self.prefs.get_string("Sound") != "ON" || self.menu.intro_is_happening || self.cues.len() > 48 {
            return;
        }
        let pan = self.camera_main().map(|c| ((at.x - self.w.position(c).x) / 9.0).clamp(-1.0, 1.0)).unwrap_or(0.0);
        self.cues.push((name, pan, amount));
    }

    /// A synthesised UI sound (screen pixels, panned across the screen).
    pub fn cue_ui(&mut self, name: &'static str, x: f32, amount: f32) {
        if self.prefs.get_string("Sound") != "ON" || self.cues.len() > 48 {
            return;
        }
        let pan = ((x / self.screen.w.max(1.0)) * 2.0 - 1.0).clamp(-1.0, 1.0) * 0.6;
        self.cues.push((name, pan, amount));
    }

    pub fn play(&mut self, clip: &str, volume: f32) {
        if self.prefs.get_string("Sound") == "ON" && !self.menu.intro_is_happening {
            self.sounds.push((clip.to_string(), volume));
        }
    }

    pub fn camera_main(&self) -> Option<usize> {
        (0..self.w.nodes.len()).find(|&i| self.w.nodes[i].camera.is_some() && self.w.nodes[i].tag == "MainCamera" && self.w.active_in_hierarchy(i))
    }

    pub fn sprite_alpha(&mut self, node: usize, a: f32) {
        if let Some(s) = self.w.nodes[node].sprite.as_mut() {
            s.color[3] = a;
        }
    }

    pub fn text_alpha(&mut self, node: usize, a: f32) {
        if let Some(t) = self.w.nodes[node].text.as_mut() {
            t.color[3] = a;
        }
    }

    pub fn image_alpha(&mut self, node: usize, a: f32) {
        if let Some(t) = self.w.nodes[node].image.as_mut() {
            t.color[3] = a;
        }
    }

    pub fn set_text(&mut self, node: usize, s: &str) {
        if let Some(t) = self.w.nodes[node].text.as_mut() {
            t.text = s.to_string();
        }
    }

    pub fn ui_set_position(&mut self, node: usize, p: V3) {
        let l = std::mem::take(&mut self.layout);
        l.set_position(&mut self.w, node, p);
        self.layout = l;
    }

    pub fn screen_v3(&self, x: f32, y: f32) -> V3 {
        v3(x, y, 0.0)
    }
}

impl Game {
    fn collect_scripts(&mut self) {
        self.collect_players();
        self.collect_boxes();
        self.collect_match();
        self.collect_menu();
    }

    /// OnEnable / Start for scripts whose object just became active.
    fn run_enables(&mut self) {
        let mut watch: Vec<(usize, u8)> = Vec::new();
        for (k, s) in self.scores.iter().enumerate() {
            watch.push((s.node, 0));
            let _ = k;
        }
        for p in &self.pauses {
            watch.push((p.node, 1));
        }
        for s in &self.showers {
            watch.push((s.player, 2));
        }
        for (node, kind) in watch {
            let now = self.w.active_in_hierarchy(node);
            let before = self.was_active.insert(node, now).unwrap_or(false);
            if now && !before {
                match kind {
                    0 => self.score_index_on_enable(node),
                    1 => {
                        if let Some(k) = self.pauses.iter().position(|p| p.node == node) {
                            self.pause_enable(k);
                        }
                    }
                    _ => {
                        if let Some(s) = self.showers.iter().position(|s| s.player == node) {
                            self.shower_init(s);
                        }
                    }
                }
            }
        }
    }

    fn fixed_update(&mut self) {
        self.players_fixed();
        self.boxes_fixed();
        self.cam_follow_fixed();
        self.showers_fixed();
        self.pause_fixed();
        self.misc_fixed();
    }

    fn update(&mut self) {
        self.players_update();
        self.charge_bars_update();
        self.tutorial_update();
        self.update_trails();
    }

    fn dispatch_physics_events(&mut self) {
        let events = std::mem::take(&mut self.phys.events);
        for e in &events {
            if e.trigger {
                if e.enter {
                    self.ground_cross_trigger(e.a, e.b);
                }
                continue;
            }
            if let Some(&k) = self.player_of.get(&e.a) {
                self.player_collision(k, e.b, e.enter);
            }
            if e.enter {
                if let Some(&b) = self.box_of.get(&e.a) {
                    self.box_collision(b, e.b, e.normal);
                }
            }
        }
    }

    /// Screen-pixel projection of a world point through camera `cam`.
    pub fn project(&self, cam: usize, p: V3) -> Option<[f32; 2]> {
        let c = self.w.nodes[cam].camera.as_ref()?;
        let view = self.w.matrix(cam).inverse_affine();
        let v = view.point(p);
        if v.z <= 1e-4 {
            return None;
        }
        let t = (c.fov.to_radians() / 2.0).tan();
        let aspect = self.screen.w / self.screen.h;
        let nx = v.x / (v.z * t * aspect);
        let ny = v.y / (v.z * t);
        Some([(nx + 1.0) * 0.5 * self.screen.w, (ny + 1.0) * 0.5 * self.screen.h])
    }

    /// Screen-pixel quad of a UI node, if it is on an active canvas.
    pub fn ui_screen_quad(&self, n: usize) -> Option<[[f32; 2]; 4]> {
        let &(root, mode) = self.layout.canvas_of.get(&n)?;
        let c = self.layout.corners(&self.w, n);
        if mode == 0 {
            return Some(c.map(|p| [p.x, p.y]));
        }
        let cam = self.w.nodes[root].canvas.and_then(|x| x.1).filter(|&c| self.w.active_in_hierarchy(c))?;
        let mut out = [[0.0f32; 2]; 4];
        for k in 0..4 {
            out[k] = self.project(cam, c[k])?;
        }
        Some(out)
    }

    fn button_at(&self, x: f32, y: f32) -> Option<usize> {
        if !(0..self.w.nodes.len()).any(|i| has_script(&self.w, i, "EventSystem") && self.w.active_in_hierarchy(i)) {
            return None;
        }
        // later canvases / later siblings are drawn on top: scan in draw order, keep the last hit
        let mut hit = None;
        for &i in self.ui_draw_order().iter() {
            let n = &self.w.nodes[i];
            let graphic = n.image.as_ref().map(|g| g.enabled && g.raycast).unwrap_or(false) || n.text.as_ref().map(|t| t.enabled && t.raycast).unwrap_or(false);
            if !graphic || !self.w.active_in_hierarchy(i) {
                continue;
            }
            if let Some(q) = self.ui_screen_quad(i) {
                if crate::ui::in_quad([x, y], q) {
                    hit = Some(i);
                }
            }
        }
        let mut c = hit;
        while let Some(n) = c {
            if !self.w.nodes[n].button.is_empty() {
                return Some(n);
            }
            c = self.w.nodes[n].parent;
        }
        None
    }

    /// UI nodes in hierarchy order: camera/world canvases first, overlays last.
    pub fn ui_draw_order(&self) -> Vec<usize> {
        let mut roots: Vec<(i32, usize)> = self.layout.canvas_of.iter().filter(|(n, (r, _))| *n == r).map(|(_, &(r, m))| (if m == 0 { 1 } else { 0 }, r)).collect();
        roots.sort();
        let mut out = Vec::new();
        for (_, r) in roots {
            self.w.descendants(r, &mut out);
        }
        out
    }

    fn ui_events(&mut self) {
        let touches = self.input.touches.clone();
        for t in touches {
            match t.phase {
                Phase::Began => {
                    self.fx_click(t.x, t.y);
                    if let Some(b) = self.button_at(t.x, t.y) {
                        self.pressed.insert(t.id, b);
                    }
                }
                Phase::Ended => {
                    let Some(b) = self.pressed.remove(&t.id) else { continue };
                    if self.button_at(t.x, t.y) != Some(b) {
                        continue;
                    }
                    self.fx_button(b);
                    for c in self.w.nodes[b].button.clone() {
                        let name = c.arg.map(|a| self.w.nodes[a].name.clone()).unwrap_or_default();
                        let Some(target) = c.target else { continue };
                        if c.method == "buttonPressed" && has_script(&self.w, target, "PauseMenuBehaviour") {
                            if let Some(k) = self.pauses.iter().position(|p| p.node == target) {
                                self.pause_button(k, &name);
                            }
                        } else if c.method == "buttonPressed" {
                            self.button_pressed(&name);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Observable state for tests and the debug overlay.
    pub fn debug_state(&self) -> Value {
        let active = |n: Option<usize>| n.map(|n| self.w.active_in_hierarchy(n)).unwrap_or(false);
        let players: Vec<Value> = self
            .players
            .iter()
            .filter(|p| self.w.active_in_hierarchy(p.node) && !p.playback)
            .map(|p| {
                let pos = self.w.position(p.node);
                serde_json::json!({"name": self.w.nodes[p.node].name, "pos": [pos.x, pos.y], "carrying": p.picked_box.is_some(), "charge": p.charge_timer, "aim": [p.touch[1].x, p.touch[1].y]})
            })
            .collect();
        let score = self.scores.iter().find(|s| self.w.active_in_hierarchy(s.node)).map(|s| {
            let mut v = s.score;
            v.sort_by_key(|r| r[0]);
            serde_json::json!({"p1": v[0][1], "p2": v[1][1], "ended": s.ended})
        });
        let screen = [
            ("start", self.menu.starting),
            ("options", self.menu.options),
            ("credits", self.menu.credits),
            ("play1", self.menu.play1),
            ("play2", self.menu.play2),
            ("maps", self.menu.map_menu),
            ("game", self.menu.single_scene),
            ("end", self.menu.end_screen),
            ("tutorial", self.menu.tutorial_scene),
        ]
        .iter()
        .filter(|(_, n)| active(*n))
        .map(|(k, _)| *k)
        .collect::<Vec<_>>();
        serde_json::json!({
            "frame": self.frame,
            "screens": screen,
            "players": players,
            "score": score,
            "map": self.menu.current_map,
            "tutorialPhase": self.tutorial.as_ref().map(|t| t.phase),
            "tasks": self.tasks.len(),
            "boxesShattered": self.boxes.iter().filter(|b| b.raving).count(),
            "cursor": self.input.cursor,
            "shards": { "live": self.shards.live(), "bonds": self.shards.active.len(), "lines": self.shards.imps.iter().map(|i| i.lines.len()).sum::<usize>(), "impacts": self.shards.imps.len() },
            "camera": self.camera_main().map(|c| { let p = self.w.position(c); [p.x, p.y, p.z] }),
            "screen": [self.screen.w, self.screen.h],
        })
    }
}
