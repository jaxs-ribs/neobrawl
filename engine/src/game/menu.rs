//! MenuManager, TutorialManager, RotatingMenuIntro, SlowBlink, SpinningIcon,
//! FloatingText, GroundCrossBehaviour. Online-only paths (Photon lobbies) are
//! not available in the browser build and are left inert.

use crate::shared::math::{v3, Quat, V3};

use super::{fbool, field, fref, has_script, Game, Task};

#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub node: usize,
    pub starting: Option<usize>,
    pub options: Option<usize>,
    pub credits: Option<usize>,
    pub play1: Option<usize>,
    pub play2: Option<usize>,
    pub map_menu: Option<usize>,
    pub end_screen: Option<usize>,
    pub tutorial_screen: Option<usize>,
    pub game_scene: Option<usize>,
    pub single_scene: Option<usize>,
    pub tutorial_scene: Option<usize>,
    pub event_system: Option<usize>,
    pub my_canvas: Option<usize>,
    pub maps: Option<usize>,
    pub white_fade: Option<usize>,
    pub white_fade2: Option<usize>,
    pub white_fade_general: Option<usize>,
    pub intro_anims: Option<usize>,
    pub rotating: Option<usize>,
    pub menu_theme: Option<usize>,
    pub map_theme: Option<usize>,
    pub button: String,
    pub woosh: String,
    pub explosion: String,
    pub blob: String,
    pub dm_single: Option<usize>,
    pub camera: Option<usize>,
    pub music_onoff: Option<usize>,
    pub sound_onoff: Option<usize>,
    pub failsafe: bool,
    pub one_screen: bool,
    pub current_map: i32,
    pub move_map_started: bool,
    pub start_round_failsafe: bool,
    pub intro_is_happening: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Tutorial {
    pub node: usize,
    pub phase: i32,
    pub allowed_to_aim: bool,
    pub arrow: Option<usize>,
    pub screen: Option<usize>,
    pub texts: Vec<usize>,
    pub player: Option<usize>,
    pub ground_cross: Option<usize>,
    pub tut_end1: Option<usize>,
    pub tut_end2: Option<usize>,
    pub started: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Misc {
    pub rotating: Vec<(usize, bool, Quat, bool)>,
    pub blinks: Vec<(usize, bool, i32, f32, f32)>,
    pub spinners: Vec<usize>,
    pub floaters: Vec<(usize, f32)>,
    pub ground_cross: Vec<(usize, bool)>,
}

/// What a fade task animates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fade {
    Sprite(usize),
    Text(usize),
    Image(usize),
}

impl Game {
    pub(super) fn collect_menu(&mut self) {
        let w = &self.w;
        let Some(m) = (0..w.nodes.len()).find(|&i| has_script(w, i, "MenuManager")) else { return };
        let r = |n: &str| fref(w, m, "MenuManager", n);
        let clip = |n: &str| field(w, m, "MenuManager", n).as_str().and_then(|k| w.audio.get(k)).cloned().unwrap_or_default();
        let music_of = |n: Option<usize>, music: &Vec<super::MusicSource>| n.and_then(|n| music.iter().position(|s| s.node == n));
        self.menu = Menu {
            node: m,
            starting: r("StartingScreen"),
            options: r("OptionsScreen"),
            credits: r("CreditsScreen"),
            play1: r("PlayMenu1"),
            play2: r("PlayMenu2"),
            map_menu: r("MapMenu"),
            end_screen: r("EndScreen"),
            tutorial_screen: r("TutorialScreen"),
            game_scene: r("GameScene"),
            single_scene: r("SingleScreenGameScene"),
            tutorial_scene: r("TutorialScene"),
            event_system: r("EventSystem"),
            my_canvas: r("myCanvas"),
            maps: r("Maps"),
            white_fade: r("WhiteFade"),
            white_fade2: r("WhiteFadeRenderer2"),
            white_fade_general: r("INSPECTOR_WhiteFadeGeneral"),
            intro_anims: r("INSPECTOR_IntroAnims"),
            rotating: r("INSPECTOR_RotatingMenuIntro"),
            menu_theme: music_of(r("INSPECTOR_MainMenuTheme"), &self.music),
            map_theme: music_of(r("INSPECTOR_Map1Theme"), &self.music),
            button: clip("ButtonSound"),
            woosh: clip("WooshSound"),
            explosion: clip("ExplosionSound"),
            blob: clip("BlobSound"),
            dm_single: r("deathManagerSingle"),
            current_map: field(w, m, "MenuManager", "currentMap").as_i64().unwrap_or(0) as i32,
            ..Default::default()
        };
        if let Some(t) = (0..w.nodes.len()).find(|&i| has_script(w, i, "TutorialManager")) {
            self.tutorial = Some(Tutorial {
                node: t,
                ground_cross: fref(w, t, "TutorialManager", "groundCrossBehaviour"),
                ..Default::default()
            });
        }
        for i in 0..w.nodes.len() {
            if has_script(w, i, "RotatingMenuIntro") {
                self.misc.rotating.push((i, true, w.nodes[i].lrot, false));
            }
            if has_script(w, i, "SlowBlink") {
                let sprite = field(w, i, "SlowBlink", "sprite").as_bool().unwrap_or(true);
                let mut freq = field(w, i, "SlowBlink", "frequency").as_f64().unwrap_or(0.0) as f32;
                if freq == 0.0 {
                    freq = 80.0;
                }
                self.misc.blinks.push((i, sprite, 0, 0.0, freq));
            }
            if has_script(w, i, "SpinningIcon") {
                self.misc.spinners.push(i);
            }
            if has_script(w, i, "FloatingText") {
                self.misc.floaters.push((i, 0.0));
            }
            if has_script(w, i, "GroundCrossBehaviour") {
                self.misc.ground_cross.push((i, fbool(w, i, "GroundCrossBehaviour", "a")));
            }
        }
    }

    /// MenuManager.Start
    pub(super) fn menu_start(&mut self) {
        if self.prefs.get_string("Sound").is_empty() {
            self.prefs.set_string("Sound", "ON");
        }
        if self.prefs.get_string("Music").is_empty() {
            self.prefs.set_string("Music", "ON");
        }
        if let Some(o) = self.menu.options {
            self.menu.music_onoff = self.w.nodes[o].children.get(3).copied();
            self.menu.sound_onoff = self.w.nodes[o].children.get(4).copied();
        }
        let (mo, so) = (self.menu.music_onoff, self.menu.sound_onoff);
        let (m, s) = (self.prefs.get_string("Music"), self.prefs.get_string("Sound"));
        if let Some(n) = mo {
            self.set_text(n, &m);
        }
        if let Some(n) = so {
            self.set_text(n, &s);
        }
        if let Some(t) = self.menu.menu_theme {
            self.play_stop(t, true);
        }
        self.menu.camera = self.w.with_tag("CameraMenu");
        self.menu.intro_is_happening = true;
        self.tasks.push(Task::WhiteFade { dir: -1, i: 0 });
    }

    pub(super) fn play_stop(&mut self, src: usize, play: bool) {
        if self.prefs.get_string("Music") != "ON" {
            return;
        }
        self.music[src].playing = play;
    }

    pub(super) fn button_pressed(&mut self, name: &str) {
        if self.menu.failsafe {
            return;
        }
        let clip = self.menu.button.clone();
        self.play(&clip, 1.0);
        let m = self.menu.clone();
        let sw = |g: &mut Game, a: Option<usize>, b: Option<usize>| {
            if let (Some(a), Some(b)) = (a, b) {
                g.tasks.push(Task::SwitchMenu { prev: a, new: b, i: 0 });
                g.tasks.push(Task::WhiteFade { dir: 1, i: 0 });
            }
        };
        match name {
            "StartingScreenButton" => {
                if self.prefs.get_int("FirstLaunch") != 1 {
                    self.fade_intro(false, 20);
                    self.start_tutorial();
                    self.prefs.set_int("FirstLaunch", 1);
                } else {
                    sw(self, m.starting, m.map_menu);
                    self.menu.one_screen = true;
                }
                self.fade_intro(false, 40);
            }
            "OptionsButton" => {
                self.fade_intro(false, 20);
                sw(self, m.starting, m.options);
            }
            "TutorialButton" => {
                self.fade_intro(false, 20);
                self.start_tutorial();
            }
            "MusicButton" => self.tasks.push(Task::FlipOnOff { music: true, i: 0 }),
            "SoundButton" => self.tasks.push(Task::FlipOnOff { music: false, i: 0 }),
            "CreditsButton" => sw(self, m.options, m.credits),
            "OptionsBackButton" => {
                sw(self, m.options, m.starting);
                self.fade_intro(true, 40);
            }
            "CreditsBackButton" => sw(self, m.credits, m.options),
            "StartAGameButton" => sw(self, m.play1, m.play2),
            "PlayMenu1BackButton" => {
                sw(self, m.play1, m.starting);
                self.fade_intro(true, 40);
            }
            "OneScreenButton" => {
                sw(self, m.play2, m.map_menu);
                self.menu.one_screen = true;
            }
            "PlayMenu2BackButton" => sw(self, m.play2, m.play1),
            "MapMenuArrowLeft" => self.move_map(1),
            "MapMenuArrowRight" => self.move_map(-1),
            "MapMenuGameStart" => {
                if !self.menu.start_round_failsafe {
                    self.menu.start_round_failsafe = true;
                    self.tasks.push(Task::Failsafe { wait: 30 });
                    self.start_round();
                    if let Some(t) = m.menu_theme {
                        self.tasks.push(Task::FadeSound { src: t, fade_in: false, volume: 0.3, i: 0 });
                        self.play_stop(t, false);
                    }
                    if let Some(t) = m.map_theme {
                        self.play_stop(t, true);
                        self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
                    }
                }
            }
            "RageQuitButton" => {
                self.reset_endscreen();
                sw(self, m.end_screen, m.starting);
                self.fade_intro(true, 40);
                if let Some(t) = m.map_theme {
                    self.play_stop(t, false);
                    self.tasks.push(Task::FadeSound { src: t, fade_in: false, volume: 0.3, i: 0 });
                }
                if let Some(t) = m.menu_theme {
                    self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
                    self.play_stop(t, true);
                }
            }
            "PlayAgainButton" => {
                self.reset_endscreen();
                self.tasks.push(Task::SwitchMenuOnOff { on: false, i: 0 });
                sw(self, m.end_screen, m.single_scene);
                self.tasks.push(Task::ResetGameScene { wait: 31 });
                if let Some(t) = m.map_theme {
                    self.play_stop(t, true);
                    self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
                }
            }
            "MapBackButton" => sw(self, m.map_menu, m.starting),
            _ => {} // online lobby buttons: Photon isn't available on the web
        }
    }

    pub(super) fn fade_intro(&mut self, fade_in: bool, time: u32) {
        if let Some(wf) = self.menu.white_fade_general {
            self.w.set_active(wf, true);
            if fade_in {
                self.tasks.push(Task::Fade { what: Fade::Sprite(wf), dur: time as f32, dir: -1, i: 0, deactivate: true });
                self.set_intro(true);
            } else {
                self.tasks.push(Task::Fade { what: Fade::Sprite(wf), dur: time as f32, dir: 1, i: 0, deactivate: false });
                self.tasks.push(Task::FadeIntroEnd { time, wait: time });
            }
        } else {
            self.set_intro(fade_in);
        }
    }

    pub(super) fn set_intro(&mut self, on: bool) {
        if let Some(r) = self.menu.rotating {
            for x in self.misc.rotating.iter_mut().filter(|x| x.0 == r) {
                x.1 = on;
            }
        }
        if let Some(a) = self.menu.intro_anims {
            self.w.set_active(a, on);
        }
        self.menu.intro_is_happening = on;
    }

    fn move_map(&mut self, dir: i32) {
        let cm = self.menu.current_map;
        if cm - dir >= 0 && cm - dir <= 3 && !self.menu.move_map_started {
            self.menu.current_map -= dir;
            self.menu.move_map_started = true;
            self.tasks.push(Task::MoveMap { dir, i: 0, acc: 2.0 });
        }
        let w = self.menu.woosh.clone();
        self.play(&w, 1.0);
    }

    pub(super) fn reset_endscreen(&mut self) {
        let Some(e) = self.menu.end_screen else { return };
        let ch = self.w.nodes[e].children.clone();
        if let Some(&c0) = ch.first() {
            self.text_alpha(c0, 0.0);
        }
        for &c in ch.iter().take(7).skip(1) {
            self.sprite_alpha(c, 0.0);
            let cc = self.w.nodes[c].children.clone();
            if let Some(&t) = cc.first() {
                self.text_alpha(t, 0.0);
            }
            if let Some(&s) = cc.get(1) {
                self.sprite_alpha(s, 0.0);
            }
        }
        for &c in ch.iter().take(9).skip(7) {
            self.text_alpha(c, 0.0);
            if let Some(&s) = self.w.nodes[c].children.first() {
                self.sprite_alpha(s, 0.0);
            }
        }
    }

    pub(super) fn start_tutorial(&mut self) {
        if let (Some(a), Some(b)) = (self.menu.starting, self.menu.tutorial_screen) {
            self.tasks.push(Task::WhiteFade { dir: 1, i: 0 });
            self.tasks.push(Task::SwitchMenu { prev: a, new: b, i: 0 });
        }
        self.tasks.push(Task::FadeInTutorial { wait: 31 });
    }

    pub(super) fn end_tutorial(&mut self) {
        if let (Some(a), Some(b)) = (self.menu.tutorial_screen, self.menu.starting) {
            self.tasks.push(Task::WhiteFade { dir: 1, i: 0 });
            self.tasks.push(Task::SwitchMenu { prev: a, new: b, i: 0 });
        }
        if let Some(t) = self.menu.tutorial_scene {
            self.w.set_active(t, false);
        }
        self.fade_intro(true, 40);
    }

    pub(super) fn fade_in_tutorial(&mut self) {
        if let Some(t) = self.menu.tutorial_scene {
            self.w.set_active(t, true);
        }
        if let Some(c) = self.menu.my_canvas {
            self.w.set_active(c, true);
        }
        if let Some(cam) = self.menu.camera {
            for c in self.w.nodes[cam].children.clone() {
                self.w.set_active(c, true);
            }
        }
    }

    fn start_round(&mut self) {
        self.tasks.push(Task::SwitchMenuOnOff { on: false, i: 0 });
        let (Some(mm), Some(single)) = (self.menu.map_menu, self.menu.single_scene) else { return };
        self.tasks.push(Task::WhiteFade { dir: 1, i: 0 });
        self.tasks.push(Task::SwitchMenu { prev: mm, new: single, i: 0 });
        // enableCorrectMap
        let maps = self.w.nodes[single].children[0];
        let cm = self.menu.current_map as usize;
        for (k, m) in self.w.nodes[maps].children.clone().into_iter().enumerate() {
            self.w.set_active(m, k == cm);
        }
        if let Some(d) = self.menu.dm_single.and_then(|n| self.dm_index(n)) {
            self.choose_spawner(d, cm);
        }
    }

    pub(super) fn fade_quit_game(&mut self) {
        if let (Some(a), Some(b)) = (self.menu.starting, self.menu.single_scene) {
            self.w.set_active(a, true);
            self.tasks.push(Task::Deactivate { node: b, wait: 1 });
        }
        self.tasks.push(Task::ReenableMenu { wait: 1 });
        self.tasks.push(Task::ResetGameScene { wait: 0 });
    }

    pub(super) fn fade_end_screen(&mut self, ranking: [[i32; 2]; 8], count: i32) {
        if let (Some(a), Some(b)) = (self.menu.single_scene, self.menu.end_screen) {
            self.tasks.push(Task::WhiteFade { dir: 1, i: 0 });
            self.tasks.push(Task::SwitchMenu { prev: a, new: b, i: 0 });
        }
        self.tasks.push(Task::ReenableMenu { wait: 30 });
        self.tasks.push(Task::EndScreen { ranking, count, step: 0, wait: super::ticks(0.5) });
        self.tasks.push(Task::ResetGameScene { wait: 0 });
        if let Some(t) = self.menu.map_theme {
            self.tasks.push(Task::FadeSound { src: t, fade_in: false, volume: 0.3, i: 0 });
            self.tasks.push(Task::StopMusic { src: t, wait: 60 });
        }
    }

    pub(super) fn determine_color(i: i32) -> [f32; 4] {
        match i {
            1 => [1.0, 32.0 / 255.0, 32.0 / 255.0, 0.0],
            2 => [32.0 / 255.0, 126.0 / 255.0, 1.0, 0.0],
            3 => [1.0, 174.0 / 255.0, 0.0, 0.0],
            4 => [0.0, 205.0 / 255.0, 68.0 / 255.0, 0.0],
            _ => [0.0; 4],
        }
    }

    /// endScreenSequence, one stage per call (stages are 1 s apart).
    pub(super) fn end_screen_stage(&mut self, ranking: [[i32; 2]; 8], count: i32, step: u32) -> Option<u32> {
        let e = self.menu.end_screen?;
        let ch = self.w.nodes[e].children.clone();
        let fade = |g: &mut Game, f: Fade| g.tasks.push(Task::Fade { what: f, dur: 15.0, dir: 1, i: 0, deactivate: false });
        if step == 0 {
            let t = ch[0];
            self.set_text(t, &format!("PLAYER {} WINS!", ranking[0][0] + 1));
            fade(self, Fade::Text(t));
            for c in self.w.nodes[t].children.clone().into_iter().take(2) {
                fade(self, Fade::Sprite(c));
            }
            return Some(1);
        }
        let j = step as i32;
        if j < count {
            let c = ch[j as usize];
            fade(self, Fade::Sprite(c));
            let cc = self.w.nodes[c].children.clone();
            let t = cc[0];
            self.set_text(t, &format!("PLAYER {}: {}", ranking[j as usize][0] + 1, ranking[j as usize][1]));
            fade(self, Fade::Text(t));
            let s = cc[1];
            if let Some(sr) = self.w.nodes[s].sprite.as_mut() {
                sr.color = Self::determine_color(ranking[j as usize][0] + 1);
            }
            fade(self, Fade::Sprite(s));
            return Some(step + 1);
        }
        for &c in ch.iter().take(9).skip(7) {
            fade(self, Fade::Text(c));
            for s in self.w.nodes[c].children.clone().into_iter().take(2) {
                fade(self, Fade::Sprite(s));
            }
        }
        None
    }

    pub(super) fn reenable_menu(&mut self) {
        if let Some(c) = self.menu.camera {
            self.w.set_active(c, true);
        }
        if let Some(e) = self.menu.event_system {
            self.w.set_active(e, true);
        }
    }

    pub(super) fn white_fade_step(&mut self, dir: i32, i: u32) -> bool {
        if dir == 1 && i == 0 {
            self.menu.failsafe = true;
        }
        let a = if dir == 1 { (i + 1) as f32 / 30.0 } else { 1.0 - (i + 1) as f32 / 30.0 };
        for n in [self.menu.white_fade, self.menu.white_fade2].into_iter().flatten() {
            self.image_alpha(n, a);
        }
        if i == 10 {
            self.menu.failsafe = false;
        }
        i + 1 < 30
    }

    pub(super) fn flip_step(&mut self, music: bool, i: u32) -> bool {
        let Some(n) = (if music { self.menu.music_onoff } else { self.menu.sound_onoff }) else { return false };
        let key = if music { "Music" } else { "Sound" };
        if i < 10 {
            self.w.rotate_self(n, 0.0, 27.0, 0.0);
        }
        if i == 9 {
            let v = if self.prefs.get_string(key) == "ON" { "OFF" } else { "ON" };
            self.prefs.set_string(key, v);
            self.set_text(n, v);
        }
        if (10..20).contains(&i) {
            self.w.rotate_self(n, 0.0, 9.0, 0.0);
        }
        if i == 19 {
            if music && self.prefs.get_string("Music") != "ON" {
                for m in &mut self.music {
                    m.playing = false;
                }
            }
            if let Some(t) = self.menu.menu_theme {
                self.play_stop(t, true);
            }
            return false;
        }
        true
    }

    pub(super) fn fade_sound_step(&mut self, src: usize, fade_in: bool, volume: f32, i: u32) -> bool {
        if self.prefs.get_string("Music") != "ON" {
            return false;
        }
        let f = i as f32 / 60.0;
        self.music[src].volume = if fade_in { f * volume } else { (1.0 - f) * volume };
        i < 60
    }

    pub(super) fn fade_step(&mut self, what: Fade, dur: f32, dir: i32, i: u32, deactivate: bool) -> bool {
        let start = if dir == -1 { 1.0 } else { 0.0 };
        let a = start + (i + 1) as f32 * (1.0 / dur) * dir as f32;
        match what {
            Fade::Sprite(n) => self.sprite_alpha(n, a),
            Fade::Text(n) => self.text_alpha(n, a),
            Fade::Image(n) => self.image_alpha(n, a),
        }
        let more = ((i + 1) as f32) < dur;
        if !more && deactivate {
            if let Fade::Sprite(n) = what {
                self.w.set_active(n, false);
            }
        }
        more
    }

    // ---- TutorialManager -------------------------------------------------

    fn tutorial_start(&mut self) {
        let Some(t) = self.tutorial.as_mut() else { return };
        if t.started {
            return;
        }
        t.started = true;
        let w = &self.w;
        let tut_end1 = w.with_tag("tutEnd1");
        let tut_end2 = w.with_tag("tutEnd2");
        let arrow = w.with_tag("CameraMenu").and_then(|c| w.nodes[c].children.first().copied());
        let screen = w.with_tag("TutorialScreen");
        let player = w.with_tag("Player1Tutorial");
        let texts = screen.map(|s| w.nodes[s].children.iter().copied().take(4).collect()).unwrap_or_default();
        let t = self.tutorial.as_mut().unwrap();
        t.tut_end1 = tut_end1;
        t.tut_end2 = tut_end2;
        t.arrow = arrow;
        t.screen = screen;
        t.player = player;
        t.texts = texts;
    }

    pub(super) fn tutorial_update(&mut self) {
        let Some(node) = self.tutorial.as_ref().map(|t| t.node) else { return };
        if !self.w.active_in_hierarchy(node) {
            return;
        }
        self.tutorial_start();
        let t = self.tutorial.clone().unwrap();
        let touches = self.input.touches.len();
        let fade_t = |g: &mut Game, n: Option<usize>, dur: f32, dir: i32| {
            if let Some(n) = n {
                g.tasks.push(Task::Fade { what: Fade::Text(n), dur, dir, i: 0, deactivate: false });
            }
        };
        let text = |k: usize| t.texts.get(k).copied();
        let set_phase = |g: &mut Game, p: i32| g.tutorial.as_mut().unwrap().phase = p;
        let wait_phase = |g: &mut Game, ticks: u32, p: i32| {
            g.tasks.push(Task::TutorialPhase { phase: p, wait: ticks });
            g.tutorial.as_mut().unwrap().phase = 666;
        };
        match t.phase {
            0 if touches > 0 => {
                if let Some(n) = text(0) {
                    self.set_text(n, "Good!");
                }
                fade_t(self, text(0), 50.0, -1);
                if let Some(a) = t.arrow {
                    self.tasks.push(Task::Fade { what: Fade::Sprite(a), dur: 40.0, dir: -1, i: 0, deactivate: false });
                }
                set_phase(self, 1);
            }
            1 => wait_phase(self, 40, 5),
            2 => {
                fade_t(self, text(1), 40.0, 1);
                self.tutorial.as_mut().unwrap().allowed_to_aim = true;
                set_phase(self, 3);
            }
            3 if touches > 1 => {
                if let Some(n) = text(1) {
                    self.set_text(n, "Well Done!");
                }
                fade_t(self, text(1), 50.0, -1);
                set_phase(self, 4);
            }
            4 => set_phase(self, 666),
            5 => {
                fade_t(self, text(2), 40.0, 1);
                set_phase(self, 6);
            }
            6 => {
                let picked = t.player.and_then(|p| self.player_of.get(&p)).map(|&k| self.players[k].picked_box.is_some()).unwrap_or(false);
                if picked {
                    if let Some(n) = text(2) {
                        self.set_text(n, "Good!");
                    }
                    fade_t(self, text(2), 50.0, -1);
                    fade_t(self, text(3), 50.0, 1);
                    set_phase(self, 2);
                }
            }
            7 => {
                if let Some(n) = text(1) {
                    self.set_text(n, "");
                }
                if let Some(wf) = self.w.with_tag("WhiteFade") {
                    self.tasks.push(Task::Fade { what: Fade::Image(wf), dur: 40.0, dir: 1, i: 0, deactivate: false });
                }
                set_phase(self, 100);
            }
            100 => wait_phase(self, 20, 101),
            101 => {
                fade_t(self, text(3), 10.0, -1);
                if let Some(wf) = self.w.with_tag("WhiteFade") {
                    self.tasks.push(Task::Fade { what: Fade::Image(wf), dur: 40.0, dir: -1, i: 0, deactivate: false });
                }
                for c in self.w.nodes[node].children.clone() {
                    self.w.set_active(c, false);
                }
                if let Some(cam) = self.menu.camera {
                    for c in self.w.nodes[cam].children.clone() {
                        self.w.set_active(c, false);
                    }
                }
                if let Some(c) = fref(&self.w, node, "TutorialManager", "myCanvas") {
                    self.w.set_active(c, false);
                }
                set_phase(self, 8);
            }
            8 => wait_phase(self, 1, 9),
            9 => {
                fade_t(self, t.tut_end1, 40.0, 1);
                set_phase(self, 10);
            }
            10 => wait_phase(self, 60, 11),
            11 => {
                fade_t(self, t.tut_end2, 80.0, 1);
                set_phase(self, 12);
            }
            12 => wait_phase(self, 30, 13),
            13 if touches > 0 => {
                if let Some(cam) = self.menu.camera {
                    self.w.set_position(cam, v3(0.0, 1.0, -10.0));
                }
                set_phase(self, 14);
            }
            14 => {
                fade_t(self, t.tut_end1, 80.0, -1);
                fade_t(self, t.tut_end2, 80.0, -1);
                set_phase(self, 15);
            }
            15 => wait_phase(self, 80, 16),
            16 => {
                for c in self.w.nodes[node].children.clone() {
                    self.w.set_active(c, true);
                }
                if let Some(&boxes) = self.w.nodes[node].children.get(2) {
                    for b in self.w.nodes[boxes].children.clone() {
                        if let Some(&bi) = self.box_of.get(&b) {
                            let p = self.boxes[bi].initial_pos;
                            self.w.set_position(b, p);
                            // original sets eulerAngles = initialPos (a typo); keep rotation sane
                            self.w.set_euler(b, p.x, p.y, p.z);
                        }
                    }
                }
                if let Some(g) = t.ground_cross {
                    for x in self.misc.ground_cross.iter_mut().filter(|x| x.0 == g) {
                        x.1 = false;
                    }
                }
                self.end_tutorial();
                set_phase(self, 0);
            }
            _ => {}
        }
    }

    pub(super) fn ground_cross_trigger(&mut self, gc: usize, other: usize) {
        let Some(k) = self.misc.ground_cross.iter().position(|x| x.0 == gc) else { return };
        if self.w.nodes[other].tag == "Box" && !self.misc.ground_cross[k].1 {
            self.tasks.push(Task::ScaleDownVelocity { node: other, i: 0 });
            if let Some(t) = self.tutorial.as_mut() {
                t.phase = 7;
            }
            self.misc.ground_cross[k].1 = true;
        }
    }

    pub(super) fn misc_fixed(&mut self) {
        for k in 0..self.misc.rotating.len() {
            let (n, rot, init, _) = self.misc.rotating[k];
            if !self.w.active_in_hierarchy(n) {
                continue;
            }
            if rot {
                self.w.rotate_self(n, 0.0, 0.0, 0.2);
            } else {
                self.w.nodes[n].lrot = init;
            }
        }
        for k in 0..self.misc.blinks.len() {
            let (n, sprite, timer, mut alpha, freq) = self.misc.blinks[k];
            if !self.w.active_in_hierarchy(n) {
                continue;
            }
            if (timer as f32) % freq < freq / 2.0 {
                alpha += 1.0 / (freq / 2.0);
            } else {
                alpha -= 1.0 / (freq / 2.0);
            }
            if sprite {
                self.sprite_alpha(n, alpha);
            } else {
                self.text_alpha(n, alpha);
            }
            self.misc.blinks[k] = (n, sprite, timer + 1, alpha, freq);
        }
        for k in 0..self.misc.spinners.len() {
            let n = self.misc.spinners[k];
            if self.w.active_in_hierarchy(n) {
                self.w.rotate_self(n, 0.0, 0.0, 0.6);
            }
        }
        for k in 0..self.misc.floaters.len() {
            let (n, t) = self.misc.floaters[k];
            if self.w.active_in_hierarchy(n) {
                let d = V3 { x: 0.0, y: t.sin() / 40.0, z: 0.0 };
                self.w.translate_world(n, d);
                self.misc.floaters[k].1 = t + 0.04;
            }
        }
    }
}
