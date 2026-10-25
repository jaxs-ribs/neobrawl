//! DeathManager, ScoreManager, CameraFollowDouble, BlobExplosion,
//! ScoreShowerBehaviour, PauseMenuBehaviour, ChargeBarPlayerBehaviour.

use crate::shared::math::{v3, V3};

use super::{fbool, field, fref, has_script, ticks, Game, Task};

#[derive(Clone, Debug, Default)]
pub struct DeathMgr {
    pub node: usize,
    pub spawn_points: Option<usize>,
    pub double_cam: bool,
    pub blobs: Vec<usize>,
    pub players: Vec<usize>,
    pub intro_fight: bool,
    pub double: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ScoreMgr {
    pub node: usize,
    pub score: [[i32; 2]; 8],
    pub max_score: i32,
    pub ended: bool,
    pub player_count: i32,
    pub players: Vec<usize>,
    pub one_screen: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CamFollow {
    pub node: usize,
    pub players: [Option<usize>; 2],
    pub should_follow: bool,
    pub started: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Blob {
    pub node: usize,
    pub init_pos: V3,
    pub init_scale: V3,
    pub checked: i32,
    pub intro: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Shower {
    pub player: usize,
    pub bar: Option<usize>,
    pub text: Option<usize>,
    pub index: usize,
    pub score: i32,
    pub running: bool,
    pub stop: bool,
    pub two: bool,
    pub p1: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Pause {
    pub node: usize,
    pub white_fade: Option<usize>,
    pub resume: Option<usize>,
    pub quit: Option<usize>,
    pub single: bool,
    pub ended: bool,
}

impl Game {
    pub(super) fn collect_match(&mut self) {
        for i in 0..self.w.nodes.len() {
            let w = &self.w;
            if has_script(w, i, "DeathManager") {
                let arr = |name: &str| -> Vec<usize> {
                    field(w, i, "DeathManager", name).as_array().map(|a| a.iter().filter_map(|v| w.obj(v)).collect()).unwrap_or_default()
                };
                self.deaths.push(DeathMgr {
                    node: i,
                    spawn_points: None,
                    double_cam: fbool(w, i, "DeathManager", "doubleCam"),
                    blobs: arr("PointBlobParent"),
                    players: arr("Players"),
                    intro_fight: fbool(w, i, "DeathManager", "INSPECTOR_introFight"),
                    double: fbool(w, i, "DeathManager", "INSPECTOR_double"),
                });
            }
            if has_script(w, i, "ScoreManager") {
                let players = field(w, i, "ScoreManager", "instantiatedPlayers")
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| w.obj(v)).collect())
                    .unwrap_or_default();
                let mut s = ScoreMgr {
                    node: i,
                    max_score: 99,
                    player_count: field(w, i, "ScoreManager", "playerCount").as_i64().unwrap_or(2) as i32,
                    players,
                    one_screen: fbool(w, i, "ScoreManager", "INSPECTOR_oneScreen"),
                    ..Default::default()
                };
                for k in 0..8 {
                    s.score[k] = [k as i32, 0];
                }
                self.scores.push(s);
            }
            if has_script(w, i, "CameraFollowDouble") {
                self.cam_follow.push(CamFollow { node: i, ..Default::default() });
            }
            if has_script(w, i, "BlobExplosion") {
                let c0 = w.nodes[i].children[0];
                self.blobs.push(Blob {
                    node: i,
                    init_pos: w.nodes[c0].lpos,
                    init_scale: w.nodes[c0].lscale,
                    checked: 0,
                    intro: fbool(w, i, "BlobExplosion", "INSPECTOR_intro"),
                });
            }
            if has_script(w, i, "ScoreShowerBehaviour") {
                self.showers.push(Shower {
                    player: i,
                    two: fbool(w, i, "ScoreShowerBehaviour", "INSPECTOR_TwoPlayersOneScreen"),
                    p1: fbool(w, i, "ScoreShowerBehaviour", "INSPECTOR_Player1"),
                    ..Default::default()
                });
            }
            if has_script(w, i, "ChargeBarPlayerBehaviour") {
                if let Some(p) = fref(w, i, "ChargeBarPlayerBehaviour", "correspondingPlayer") {
                    self.charge_bars.push((i, p));
                }
            }
            if has_script(w, i, "PauseMenuBehaviour") {
                self.pauses.push(Pause {
                    node: i,
                    white_fade: fref(w, i, "PauseMenuBehaviour", "whiteFade"),
                    resume: fref(w, i, "PauseMenuBehaviour", "ResumeButton"),
                    quit: fref(w, i, "PauseMenuBehaviour", "QuitButton"),
                    single: fbool(w, i, "PauseMenuBehaviour", "singlePlayer"),
                    ended: false,
                });
            }
        }
    }

    fn death_mgr_of_main(&self) -> Option<usize> {
        let cam = self.camera_main()?;
        self.deaths.iter().position(|d| d.node == cam)
    }

    fn score_mgr_of_main(&self) -> Option<usize> {
        let cam = self.camera_main()?;
        self.scores.iter().position(|d| d.node == cam)
    }

    /// DeathManager.death (called on Camera.main, or the menu camera in intros).
    pub(super) fn death(&mut self, player: usize, killer: Option<usize>) {
        let Some(killer) = killer else { return };
        if player == killer {
            return;
        }
        let dm = self.death_mgr_of_main().or_else(|| {
            let mc = self.w.with_tag("CameraMenu")?;
            self.deaths.iter().position(|d| d.node == mc)
        });
        let Some(d) = dm else { return };
        self.attach_camera(d, false);
        // showScore
        if self.deaths[d].double {
            if let Some(s) = self.showers.iter().position(|s| s.player == killer) {
                self.shower_trigger(s);
            }
        }
        let cam = self.deaths[d].node;
        self.tasks.push(Task::CamShake { cam, i: 0, d: v3(0.08, 0.08, 0.0) });
        // teleportPointBlob
        let tag = self.w.nodes[player].tag.clone();
        let idx = tag.chars().last().and_then(|c| c.to_digit(10)).unwrap_or(1) as usize - 1;
        if let Some(&parent) = self.deaths[d].blobs.get(idx) {
            self.w.set_active(parent, true);
            let pp = self.w.position(player) + v3(0.0, 0.0, 2.0);
            self.w.set_position(parent, pp);
            self.blob_explosion(parent, killer);
        }
        self.w.set_position(player, v3(-666.0, -666.0, -666.0));
        self.phys.forget(player);
        self.tasks.push(Task::Respawn { dm: d, player, wait: ticks(3.0) });
    }

    pub(super) fn attach_camera(&mut self, d: usize, on: bool) {
        if self.deaths[d].intro_fight {
            return;
        }
        let cam = self.deaths[d].node;
        if let Some(c) = self.cam_follow.iter_mut().find(|c| c.node == cam) {
            c.should_follow = on;
        }
    }

    pub(super) fn set_aimer(&mut self, on: bool) {
        if let Some(a) = self.w.with_tag("AimerAnchor") {
            if let Some(&c) = self.w.nodes[a].children.first() {
                if let Some(s) = self.w.nodes[c].sprite.as_mut() {
                    s.enabled = on;
                }
            }
        }
    }

    pub(super) fn respawn(&mut self, d: usize, player: usize) {
        self.set_aimer(true);
        if !self.deaths[d].intro_fight {
            if let Some(sp) = self.deaths[d].spawn_points {
                let n = self.w.nodes[sp].children.len() as i32;
                let r = self.rng.range(0, n) as usize;
                let p = self.w.position(self.w.nodes[sp].children[r]);
                self.w.set_position(player, p);
                self.phys.forget(player);
            }
        }
        self.attach_camera(d, true);
    }

    pub(super) fn choose_spawner(&mut self, d: usize, map: usize) {
        let cam = self.deaths[d].node;
        let Some(parent) = self.w.nodes[cam].parent else { return };
        let maps = self.w.nodes[parent].children[0];
        let m = self.w.nodes[maps].children[map];
        self.deaths[d].spawn_points = self.w.nodes[m].children.first().copied();
    }

    pub(super) fn reset_players(&mut self, d: usize) {
        let players = self.deaths[d].players.clone();
        for (num, &p) in players.iter().enumerate() {
            self.set_aimer(true);
            if let Some(sp) = self.deaths[d].spawn_points {
                if let Some(&c) = self.w.nodes[sp].children.get(num) {
                    let pos = self.w.position(c);
                    self.w.set_position(p, pos);
                    self.phys.forget(p);
                }
            }
            if let Some(&k) = self.player_of.get(&p) {
                self.drop_box(k);
            }
            self.attach_camera(d, true);
        }
    }

    pub(super) fn blob_explosion(&mut self, parent: usize, killer: usize) {
        let Some(b) = self.blobs.iter().position(|x| x.node == parent) else { return };
        for i in 0..8usize {
            let (mut x, mut y) = [(1.0, 0.0), (0.7, 0.7), (0.0, 1.0), (-0.7, 0.7), (-1.0, 0.0), (-0.7, -0.7), (0.0, -1.0), (0.7, -0.7)][i];
            x += self.rng.rangef(-0.3, 0.3);
            y += self.rng.rangef(-0.3, 0.3);
            let k = self.rng.rangef(1.1, 3.0) / 10.0;
            let orb = self.w.nodes[parent].children[i];
            self.tasks.push(Task::OrbFly { blob: b, orb, killer, v: v3(x * k, y * k, 0.0), i: 1 });
            self.tasks.push(Task::OrbScale { orb, j: 0 });
        }
    }

    /// moveTowardsNearestPlayer; returns false when the orb arrived.
    pub(super) fn orb_seek(&mut self, b: usize, orb: usize, killer: usize, step: &mut f32) -> bool {
        let t = self.w.position(orb);
        let k = self.w.position(killer);
        let reached = (t.x - k.x).abs() <= 0.1 && (t.y - k.y).abs() <= 0.1;
        let target = k + v3(0.0, 0.0, 1.0);
        let d = target - t;
        let l = d.length();
        let np = if l <= *step || l < 1e-6 { target } else { t + d * (*step / l) };
        self.w.set_position(orb, np);
        *step += 0.02;
        if !reached {
            return true;
        }
        self.w.nodes[orb].lscale = V3::ZERO;
        // resetPositionAndScale
        self.blobs[b].checked += 1;
        if self.blobs[b].checked == 8 {
            let parent = self.blobs[b].node;
            let (ip, is) = (self.blobs[b].init_pos, self.blobs[b].init_scale);
            for c in self.w.nodes[parent].children.clone() {
                self.w.nodes[c].lpos = ip;
                self.w.nodes[c].lscale = is;
            }
            self.blobs[b].checked = 0;
            self.w.set_active(parent, false);
        }
        self.blob_scale_start(killer);
        if !self.blobs[b].intro {
            let name = self.w.nodes[killer].name.clone();
            let idx = name.chars().last().and_then(|c| c.to_digit(10)).unwrap_or(1) as usize - 1;
            if let Some(s) = self.score_mgr_of_main() {
                self.increase_score(s, idx);
            }
        }
        false
    }

    fn increase_score(&mut self, s: usize, idx: usize) {
        self.scores[s].score[idx][1] += 1;
        let blob = self.menu.blob.clone();
        self.play(&blob, 1.0);
        let v = self.scores[s].score[idx][1];
        if v >= self.scores[s].max_score && v % 8 == 0 && !self.scores[s].ended {
            self.end_game(s);
        }
    }

    fn bubble_sort(sc: &mut [[i32; 2]; 8]) {
        for i in 0..8 {
            for j in i + 1..8 {
                if sc[i][1] < sc[j][1] {
                    sc.swap(i, j);
                }
            }
        }
    }

    fn end_game(&mut self, s: usize) {
        Self::bubble_sort(&mut self.scores[s].score);
        self.scores[s].ended = true;
        let ranking = self.scores[s].score;
        for &p in &self.scores[s].players.clone() {
            for c in 0..2 {
                if let Some(&ch) = self.w.nodes[p].children.get(c) {
                    if let Some(sr) = self.w.nodes[ch].sprite.as_mut() {
                        sr.enabled = false;
                    }
                }
            }
        }
        let count = self.scores[s].player_count;
        self.fade_end_screen(ranking, count);
    }

    pub(super) fn quit_game(&mut self, s: usize) {
        Self::bubble_sort(&mut self.scores[s].score);
        self.scores[s].ended = true;
        self.fade_quit_game();
    }

    /// ScoreManager.Start / OnEnable: show players again.
    pub(super) fn score_enable(&mut self, s: usize) {
        for k in 0..8 {
            self.scores[s].score[k] = [k as i32, 0];
        }
        for &p in &self.scores[s].players.clone() {
            self.w.nodes[p].lscale = v3(1.0, 1.0, 1.0);
            for c in 0..2 {
                if let Some(&ch) = self.w.nodes[p].children.get(c) {
                    if let Some(sr) = self.w.nodes[ch].sprite.as_mut() {
                        sr.enabled = true;
                    }
                }
            }
        }
    }

    pub(super) fn reset_game_scene(&mut self) {
        if let Some(d) = self.death_mgr_of_main() {
            self.reset_players(d);
        }
        self.tasks.push(Task::ResetScores { wait: 1 });
    }

    pub(super) fn reset_scores(&mut self) {
        if let Some(s) = self.score_mgr_of_main() {
            self.scores[s].ended = false;
            for k in 0..8 {
                self.scores[s].score[k] = [k as i32, 0];
            }
        }
    }

    pub(super) fn cam_follow_fixed(&mut self) {
        for c in 0..self.cam_follow.len() {
            let node = self.cam_follow[c].node;
            if !self.w.active_in_hierarchy(node) {
                continue;
            }
            if !self.cam_follow[c].started {
                self.cam_follow[c].started = true;
                self.cam_follow[c].players = [self.w.with_tag("PlayerOneScreen1"), self.w.with_tag("PlayerOneScreen2")];
                self.cam_follow[c].should_follow = true;
            }
            let [Some(a), Some(b)] = self.cam_follow[c].players else { continue };
            let pa = self.w.position(a);
            let pb = self.w.position(b);
            let cur = self.w.position(node);
            let mt = |x: f32, t: f32, d: f32| if (t - x).abs() <= d { t } else { x + (t - x).signum() * d };
            let np = if self.cam_follow[c].should_follow {
                let zx = (pa.x - pb.x).abs() * 1.1;
                let zy = (pa.y - pb.y).abs() * 1.6;
                let zoom = zx.max(zy);
                let mid = (pa + pb) * 0.5;
                v3(mt(cur.x, mid.x, 0.04), mt(cur.y, mid.y, 0.4), mt(cur.z, -5.75 - zoom / 3.0, 0.4))
            } else if pa.x == -666.0 {
                v3(mt(cur.x, pb.x, 0.03), mt(cur.y, pb.y, 0.3), mt(cur.z, pb.z - 8.0, 0.05))
            } else {
                v3(mt(cur.x, pa.x, 0.03), mt(cur.y, pa.y, 0.3), mt(cur.z, pa.z - 8.0, 0.05))
            };
            self.w.set_position(node, np);
        }
    }

    pub(super) fn shower_trigger(&mut self, s: usize) {
        self.showers[s].stop = false;
        if !self.showers[s].running {
            self.showers[s].running = true;
            self.tasks.push(Task::ShowerScaleUp { s, i: 0, wait: 0 });
        }
    }

    /// ScoreShowerBehaviour.delayedStartMethod + delayedInitialization.
    pub(super) fn shower_init(&mut self, s: usize) {
        let (two, p1) = (self.showers[s].two, self.showers[s].p1);
        if two {
            let (bar, txt) = if p1 { ("ScoreBanner1", "YourScore1") } else { ("ScoreBanner2", "YourScore2") };
            self.showers[s].bar = self.w.find(bar);
            self.showers[s].text = self.w.find(txt);
        }
        if let Some(b) = self.showers[s].bar {
            self.w.nodes[b].lscale.y = 0.0;
        }
        let name = self.w.nodes[self.showers[s].player].name.clone();
        self.showers[s].index = name.chars().last().and_then(|c| c.to_digit(10)).unwrap_or(1) as usize - 1;
        if let Some(t) = self.showers[s].text {
            self.set_text(t, "");
        }
    }

    pub(super) fn showers_fixed(&mut self) {
        let main_scores = self.score_mgr_of_main().map(|s| (self.scores[s].score, self.scores[s].ended));
        for s in 0..self.showers.len() {
            if !self.w.active_in_hierarchy(self.showers[s].player) {
                continue;
            }
            if let Some((sc, ended)) = main_scores {
                let idx = self.showers[s].index;
                self.showers[s].score = sc.iter().find(|r| r[0] as usize == idx).map(|r| r[1]).unwrap_or(0);
                if ended {
                    self.showers[s].stop = true;
                    if let Some(b) = self.showers[s].bar {
                        self.w.nodes[b].lscale.y = 0.0;
                    }
                    if let Some(t) = self.showers[s].text {
                        self.set_text(t, "");
                    }
                }
            }
        }
    }

    /// ScoreShowerBehaviour.scaleUp, one step. false = finished.
    pub(super) fn shower_step(&mut self, s: usize, i: &mut u32, wait: &mut u32) -> bool {
        let (Some(bar), Some(text)) = (self.showers[s].bar, self.showers[s].text) else {
            self.showers[s].running = false;
            return false;
        };
        let k = *i;
        let ys = &mut self.w.nodes[bar].lscale.y;
        if k == 0 {
            *ys = 0.0;
            self.set_text(text, "SCORE:");
        }
        let ys = &mut self.w.nodes[bar].lscale.y;
        if k < 25 {
            *ys = ((k + 1) * (k + 1)) as f32 * (120.0 / 625.0);
        } else if k < 33 {
            *ys -= 15.0 / 8.0;
        } else if k < 36 {
            *ys += 5.0 / 3.0;
            if k == 35 {
                *wait = ticks(0.25);
            }
        } else if k < 186 {
            let m = k - 36;
            if self.showers[s].stop {
                self.showers[s].running = false;
                return false;
            }
            let t = if [21, 31, 49, 101].contains(&m) { String::new() } else { format!("SCORE: {}/99", self.showers[s].score) };
            self.set_text(text, &t);
        } else if k < 196 {
            let j = (k - 186) as f32;
            *ys = 80.0 - (j + 1.0) * (j + 1.0) * 0.8;
        } else {
            self.set_text(text, "");
            self.showers[s].running = false;
            return false;
        }
        *i += 1;
        true
    }

    pub(super) fn charge_bars_update(&mut self) {
        for k in 0..self.charge_bars.len() {
            let (bar, player) = self.charge_bars[k];
            if self.w.active_in_hierarchy(bar) {
                let p = self.w.position(player);
                self.ui_set_position(bar, p);
            }
        }
    }

    pub(super) fn pause_fixed(&mut self) {
        let (sw, sh) = (self.screen.w, self.screen.h);
        for k in 0..self.pauses.len() {
            let p = self.pauses[k].clone();
            if !self.w.active_in_hierarchy(p.node) {
                continue;
            }
            let pos = if p.single { v3(sw / 2.0, 0.9 * sh, 0.0) } else { v3((8.0 * sw / 10.0).floor(), 0.85 * sh, 0.0) };
            self.ui_set_position(p.node, pos);
            if let Some(r) = p.resume {
                self.ui_set_position(r, v3((sw / 2.0).floor(), (6.0 * sh / 10.0).floor(), 0.0));
            }
            if let Some(q) = p.quit {
                self.ui_set_position(q, v3((sw / 2.0).floor(), (4.0 * sh / 10.0).floor(), 0.0));
            }
        }
    }

    pub(super) fn pause_button(&mut self, k: usize, button: &str) {
        let theme = self.menu.map_theme;
        match button {
            "PauseButton" => {
                self.tasks.push(Task::PauseFade { pause: k, fade_in: true, i: 0 });
                if let Some(t) = theme {
                    self.tasks.push(Task::FadeSound { src: t, fade_in: false, volume: 0.3, i: 0 });
                }
            }
            "ResumeButton" => {
                self.pause_elements(k, false);
                self.tasks.push(Task::PauseFade { pause: k, fade_in: false, i: 0 });
                if let Some(t) = theme {
                    self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
                }
            }
            "QuitButton" => {
                self.pauses[k].ended = true;
                self.tasks.push(Task::DelayedQuit { pause: k, wait: 2 });
            }
            _ => {}
        }
    }

    pub(super) fn pause_elements(&mut self, k: usize, b: bool) {
        if let Some(r) = self.pauses[k].resume {
            self.w.set_active(r, b);
        }
        if let Some(q) = self.pauses[k].quit {
            self.w.set_active(q, b);
        }
    }

    pub(super) fn delayed_quit(&mut self, k: usize) {
        if let Some(t) = self.menu.map_theme {
            self.play_stop(t, false);
        }
        if let Some(t) = self.menu.menu_theme {
            self.play_stop(t, true);
            self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
        }
        self.fade_intro(true, 0);
        self.reset_game_scene();
        self.pause_elements(k, false);
        if let Some(s) = self.score_mgr_of_main() {
            self.quit_game(s);
        }
    }

    pub(super) fn pause_enable(&mut self, k: usize) {
        self.pauses[k].ended = false;
    }

    pub(super) fn score_index_on_enable(&mut self, node: usize) {
        if let Some(s) = self.scores.iter().position(|x| x.node == node) {
            self.score_enable(s);
        }
    }

    pub(super) fn dm_index(&self, node: usize) -> Option<usize> {
        self.deaths.iter().position(|d| d.node == node)
    }
}
