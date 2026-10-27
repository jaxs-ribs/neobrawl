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
    /// a respawn swoop in flight: start time (s) and where it left from
    pub swoop: Option<(f32, V3)>,
    /// the solo camera's spring velocity (xy) and zoom distance
    pub vel: V3,
    pub dist: f32,
    pub wide_until: f32,
}

/// MGE pacing: back in the fight half a second after dying.
pub const RESPAWN_SECS: f32 = 0.55;
/// The respawn swoop's length (s).
const SWOOP_SECS: f32 = 0.85;

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
        self.fx_kill(player, killer);
        self.attach_camera(d, false);
        // showScore
        if self.deaths[d].double {
            if let Some(s) = self.showers.iter().position(|s| s.player == killer) {
                self.shower_trigger(s);
            }
        }
        let cam = self.deaths[d].node;
        let _ = cam; // shake is drawn at render time (motion::shake)
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
        self.last_killer.insert(player, killer);
        self.tasks.push(Task::Respawn { dm: d, player, wait: ticks(RESPAWN_SECS) });
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
                // the spawn farthest from whoever is alive (the killer first):
                // straight back in, but never into their lap
                let threats: Vec<V3> = self
                    .last_killer
                    .get(&player)
                    .map(|&k| vec![self.w.position(k)])
                    .unwrap_or_default()
                    .into_iter()
                    .chain(self.players.iter().filter(|p| p.node != player).map(|p| self.w.position(p.node)))
                    .filter(|p| p.x != -666.0)
                    .collect();
                let kids = self.w.nodes[sp].children.clone();
                let p = kids
                    .iter()
                    .map(|&c| self.w.position(c))
                    .max_by(|a, b| {
                        let da = threats.iter().map(|t| (*t - *a).length()).fold(f32::MAX, f32::min);
                        let db = threats.iter().map(|t| (*t - *b).length()).fold(f32::MAX, f32::min);
                        da.partial_cmp(&db).unwrap()
                    });
                if let Some(p) = p {
                    self.w.set_position(player, p);
                    self.phys.forget(player);
                }
            }
        }
        // the camera swoops from the kill to the new fight
        let now = self.motion.now;
        let cam = self.deaths[d].node;
        if let Some(c) = self.cam_follow.iter_mut().find(|c| c.node == cam) {
            let from = self.w.position(cam);
            c.swoop = Some((now, from));
        }
        self.fx_respawn(player);
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
            // blasted on along the hit: heavier hits throw them much further,
            // and one or two rocket off (heavy tail), plus a wide spread
            let hit = self.kill_impulse;
            let hs = hit.length();
            let tail = self.rng.rangef(0.02, 1.0).powf(-1.0 / 2.2).min(2.5);
            let along = if hs > 1e-3 { hit * ((0.14 + 0.01 * hs.min(90.0)) * self.rng.rangef(0.5, 1.2) * tail / hs) } else { V3::ZERO };
            let v = v3(x * k * 1.6, y * k * 1.6, 0.0) + along;
            self.tasks.push(Task::OrbFly { blob: b, orb, killer, v, i: 1 });
            self.tasks.push(Task::OrbScale { orb, j: 0 });
        }
    }

    /// moveTowardsNearestPlayer; returns false when the orb arrived.
    /// The soul phase: a dart that steers into the killer, faster and
    /// faster, leaving a fading streak.
    pub(super) fn orb_seek(&mut self, b: usize, orb: usize, killer: usize, step: &mut f32, v: &mut V3) -> bool {
        let t = self.w.position(orb);
        let k = self.w.position(killer);
        let target = k + v3(0.0, 0.0, 1.0);
        let d = v3(target.x - t.x, target.y - t.y, 0.0);
        let l = d.length();
        let want = if l > 1e-6 { d * (*step / l) } else { V3::ZERO };
        *v = *v + (want - *v) * 0.3;
        let reached = l <= (0.12f32).max(v.length()) || k.x == -666.0;
        let np = if reached { target } else { t + *v };
        self.w.set_position(orb, np);
        *step = (*step + 0.025).min(0.6);
        let col = self.soul_color(killer);
        self.motion.push(crate::motion::Kind::Streak { a: t, b: np, w: 0.05 }, false, 0.0, 0.16, col);
        if !reached {
            return true;
        }
        self.w.nodes[orb].lscale = V3::ZERO;
        // resetPositionAndScale
        self.blobs[b].checked += 1;
        let n = self.blobs[b].checked as f32;
        self.cue("home", k, n);
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
            if self.anthill.on {
                self.solo_camera(c);
                continue;
            }
            let [Some(a), Some(b)] = self.cam_follow[c].players else { continue };
            let pa = self.w.position(a);
            let pb = self.w.position(b);
            let cur = self.w.position(node);
            // Frame the living (both, or the survivor while the other is
            // dead) from the real field of view and aspect, with one smooth
            // exponential follow (the original crept at phone-tuned rates).
            let alive_a = pa.x != -666.0;
            let alive_b = pb.x != -666.0;
            let (pa, pb) = match (alive_a, alive_b) {
                (true, true) => (pa, pb),
                (true, false) => (pa, pa),
                (false, true) => (pb, pb),
                (false, false) => (cur, cur),
            };
            // target practice: frame you and every target instead
            let (pa, pb) = if self.practice.on {
                let pts: Vec<V3> = std::iter::once(pa).chain(self.practice.targets.iter().map(|t| self.w.position(t.node))).filter(|p| p.x != -666.0).collect();
                let lo = pts.iter().fold(v3(f32::MAX, f32::MAX, pa.z), |a, p| v3(a.x.min(p.x), a.y.min(p.y), pa.z));
                let hi = pts.iter().fold(v3(f32::MIN, f32::MIN, pa.z), |a, p| v3(a.x.max(p.x), a.y.max(p.y), pa.z));
                (lo, hi)
            } else {
                (pa, pb)
            };
            let lens = if self.w.nodes[node].camera.is_some() { Some(node) } else { self.camera_main() };
            let fov = lens.and_then(|l| self.w.nodes[l].camera.as_ref()).map(|k| k.fov).unwrap_or(60.0);
            // the lens may sit on a child offset from the follow node
            let off = lens.map(|l| self.w.position(l) - cur).unwrap_or(V3::ZERO);
            let t = (fov.to_radians() / 2.0).tan();
            let aspect = (self.screen.w / self.screen.h).max(0.5);
            // breathing room grows with the gap, so far-apart players never hug the frame
            let (hx, hy) = ((pa.x - pb.x).abs() / 2.0, (pa.y - pb.y).abs() / 2.0);
            let margin = 2.2 + 0.18 * hx.max(hy);
            let half_h = (hy + margin).max((hx + margin) / aspect);
            // holding breath: in a lull, or while someone charges, the
            // camera leans in a little; any action lets it go
            let idle = ((self.motion.now - self.motion.last_action - 1.2) / 2.5).clamp(0.0, 1.0);
            let charge = self.players.iter().map(|p| (p.charge_timer / 90.0).clamp(0.0, 1.0)).fold(0.0f32, f32::max);
            let want_breath = 1.0 - 0.05 * idle - 0.05 * charge;
            let rate = if want_breath > self.motion.breath { 0.25 } else { 0.02 };
            self.motion.breath = if self.motion.breath == 0.0 { 1.0 } else { self.motion.breath + (want_breath - self.motion.breath) * rate };
            // during a death: punch in on the killer
            let dist = if self.cam_follow[c].should_follow { (half_h / t).max(5.75) * self.motion.breath } else { 6.8 };
            let mid = (pa + pb) * 0.5;
            let want = v3(mid.x - off.x, mid.y - off.y, mid.z - dist - off.z);
            // quick to widen, slow to settle back in
            let dt = 0.02;
            let k_xy = 1.0 - (-dt / 0.14f32).exp();
            let punch = !self.cam_follow[c].should_follow;
            let k_z = 1.0 - (-dt / if want.z < cur.z { 0.1f32 } else if punch { 0.12 } else { 0.6 }).exp();
            let mut np = v3(cur.x + (want.x - cur.x) * k_xy, cur.y + (want.y - cur.y) * k_xy, cur.z + (want.z - cur.z) * k_z);
            // the respawn swoop: an eased arc from the kill to the new framing,
            // pulling back mid-flight and curving off the straight line
            if let Some((t0, from)) = self.cam_follow[c].swoop {
                let u = (self.motion.now - t0) / SWOOP_SECS;
                if u >= 1.0 {
                    self.cam_follow[c].swoop = None;
                } else {
                    let e = u * u * u * (u * (u * 6.0 - 15.0) + 10.0); // smootherstep
                    let travel = v3(want.x - from.x, want.y - from.y, 0.0);
                    let tl = (travel.x * travel.x + travel.y * travel.y).sqrt();
                    let side = if tl > 1e-3 { v3(-travel.y / tl, travel.x / tl, 0.0) } else { V3::ZERO };
                    let lift = (std::f32::consts::PI * u).sin();
                    np = from + (want - from) * e + side * (lift * tl * 0.18) - v3(0.0, 0.0, lift * (2.0 + tl * 0.25));
                }
            }
            self.w.set_position(node, np);
        }
    }

    /// The solo camera (anthill): you at the centre, leaning toward where
    /// you aim, nudged by threats close by; a dead zone so small steps don't
    /// drag the frame, a critically damped spring after it, and a zoom that
    /// widens quickly when soldiers close in and settles back slowly.
    fn solo_camera(&mut self, c: usize) {
        let node = self.cam_follow[c].node;
        let cur = self.w.position(node);
        let Some(k) = self.players.iter().position(|p| &*self.w.nodes[p.node].tag == "PlayerOneScreen1") else { return };
        let me = self.w.position(self.players[k].node);
        let me = if me.x == -666.0 { self.anthill.start } else { me };
        let lens = if self.w.nodes[node].camera.is_some() { Some(node) } else { self.camera_main() };
        let fov = lens.and_then(|l| self.w.nodes[l].camera.as_ref()).map(|k| k.fov).unwrap_or(60.0);
        let off = lens.map(|l| self.w.position(l) - cur).unwrap_or(V3::ZERO);
        let t = (fov.to_radians() / 2.0).tan();
        // lean: where you aim, further while charging
        let aim = self.players[k].touch[1];
        let charge = (self.players[k].charge_timer / 90.0).clamp(0.0, 1.0);
        let lean = v3(aim.x, aim.y, 0.0) * (1.2 + 1.3 * charge);
        // threats: soldiers within reach pull the frame a little their way
        let reach = self.anthill.cfg.reach + 3.0;
        let mut pull = V3::ZERO;
        let mut near = 0.0f32;
        for a in self.anthill.ants.iter().filter(|a| !a.dead && a.kind == super::anthill::SOLDIER) {
            let d = self.w.position(a.node) - me;
            let l = (d.x * d.x + d.y * d.y).sqrt();
            if l < reach {
                let wgt = if a.wind_until.is_some() { 0.3 } else { 0.15 } * (1.0 - l / reach);
                pull = pull + v3(d.x, d.y, 0.0) * wgt;
                near = near.max(l);
            }
        }
        let focus = me + lean + pull;
        // dead zone round the current centre
        let here = v3(cur.x + off.x, cur.y + off.y, 0.0);
        let d = v3(focus.x - here.x, focus.y - here.y, 0.0);
        let dl = (d.x * d.x + d.y * d.y).sqrt();
        let dz = 0.5;
        let mut goal = if dl > dz { here + d * ((dl - dz) / dl) } else { here };
        // keep the frame on the floor: no more than a sliver past the walls
        let aspect = (self.screen.w / self.screen.h).max(0.5);
        let shown_h = if self.cam_follow[c].dist > 0.0 { self.cam_follow[c].dist * t } else { 7.0 };
        let (lo, hi) = (self.anthill.lo, self.anthill.hi);
        for (g, lo, hi, half) in [(&mut goal.x, lo.x, hi.x, shown_h * aspect), (&mut goal.y, lo.y, hi.y, shown_h)] {
            let (a, b) = (lo + half - 1.5, hi - half + 1.5);
            *g = if a > b { (lo + hi) / 2.0 } else { g.clamp(a, b) };
        }
        // critically damped spring toward the goal
        let dt = 0.02;
        let w0 = 2.0 / 0.32;
        let mut v = self.cam_follow[c].vel;
        let x = v3(here.x - goal.x, here.y - goal.y, 0.0);
        let acc = x * (-w0 * w0) - v * (2.0 * w0);
        v = v + acc * dt;
        let np = here + v * dt;
        self.cam_follow[c].vel = v;
        // zoom: show about 7 units up and down, more with a soldier closing in
        let half_h = (7.0f32).max(if near > 0.0 { (near + 2.5) / aspect.min(1.6) } else { 0.0 }).min(12.0);
        let want = half_h / t;
        let now = self.motion.now;
        let cf = &mut self.cam_follow[c];
        if cf.dist == 0.0 {
            cf.dist = want;
        }
        if want > cf.dist {
            cf.dist += (want - cf.dist) * (1.0 - (-dt / 0.35f32).exp());
            cf.wide_until = now + 1.2;
        } else if now > cf.wide_until {
            cf.dist += (want - cf.dist) * (1.0 - (-dt / 1.6f32).exp());
        }
        let dist = cf.dist;
        self.w.set_position(node, v3(np.x - off.x, np.y - off.y, me.z - dist - off.z));
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
                if self.paused {
                    return;
                }
                self.paused = true;
                // a resume still fading out gives way to this pause
                self.tasks.retain(|t| !matches!(t, Task::PauseFade { fade_in: false, .. }));
                self.tasks.push(Task::PauseFade { pause: k, fade_in: true, i: 0 });
                if let Some(t) = theme {
                    self.tasks.push(Task::FadeSound { src: t, fade_in: false, volume: 0.3, i: 0 });
                }
            }
            "ResumeButton" => {
                if !self.paused {
                    return;
                }
                self.paused = false;
                // a pause still fading in gives way to this resume, from where it got to
                let reached = self.tasks.iter().find_map(|t| match t {
                    Task::PauseFade { fade_in: true, i, .. } => Some(*i),
                    _ => None,
                });
                self.tasks.retain(|t| !matches!(t, Task::PauseFade { fade_in: true, .. }));
                self.pause_elements(k, false);
                if let Some(i) = reached {
                    self.tasks.push(Task::PauseFade { pause: k, fade_in: false, i: 30 - i.min(30) });
                    if let Some(t) = theme {
                        self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
                    }
                    return;
                }
                self.tasks.push(Task::PauseFade { pause: k, fade_in: false, i: 0 });
                if let Some(t) = theme {
                    self.tasks.push(Task::FadeSound { src: t, fade_in: true, volume: 0.3, i: 0 });
                }
            }
            "QuitButton" => {
                self.paused = false;
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
