//! NeoBrawl's single scene: menu, intro fights, tutorial and the two-players
//! one-screen match. Each MonoBehaviour is ported as a state struct plus
//! methods on `Game`; coroutines become `Task`s stepped once per FixedUpdate.

mod boxes;
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

pub use players::Player;
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
        self.frame += 1;
        self.run_enables();
        self.fixed_update();
        self.phys.step(&mut self.w, DT);
        self.dispatch_physics_events();
        self.step_tasks();
        self.update();
        self.relayout();
        self.ui_events();
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
                    if let Some(b) = self.button_at(t.x, t.y) {
                        self.pressed.insert(t.id, b);
                    }
                }
                Phase::Ended => {
                    let Some(b) = self.pressed.remove(&t.id) else { continue };
                    if self.button_at(t.x, t.y) != Some(b) {
                        continue;
                    }
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
            "screen": [self.screen.w, self.screen.h],
        })
    }
}
