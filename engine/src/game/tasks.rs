//! The original's coroutines, as resumable tasks. Each task runs once per
//! fixed step after physics (WaitForFixedUpdate); `wait` counts down first
//! (WaitForSeconds = seconds / 0.02 steps).

use crate::shared::math::{v3, V3};

use super::menu::Fade;
use super::Game;

#[derive(Clone, Debug)]
pub enum Task {
    TouchReset { player: usize, mv: bool, aim: bool, wait: u32 },
    ResetChargeTimer { player: usize, wait: u32 },
    ScaleBackChargeBar { player: usize },
    Unignore { a: usize, b: usize, wait: u32 },
    ThrowRotation { node: usize, i: i32, n: i32 },
    ResetBeingThrown { b: usize, stage: u32, wait: u32 },
    ReinstateCollision { player: usize, wait: u32 },
    ScalePlayer { player: usize, i: u32 },
    BoxRespawn { b: usize, wait: u32 },
    RaveLine { b: usize, index: usize, j: u32, a: f32, r1: usize, r2: usize },
    Respawn { dm: usize, player: usize, wait: u32 },
    CamShake { cam: usize, i: u32, d: V3 },
    OrbFly { blob: usize, orb: usize, killer: usize, v: V3, i: u32 },
    OrbSeek { blob: usize, orb: usize, killer: usize, step: f32, v: V3 },
    OrbScale { orb: usize, j: u32 },
    ShowerScaleUp { s: usize, i: u32, wait: u32 },
    PauseFade { pause: usize, fade_in: bool, i: u32 },
    DelayedQuit { pause: usize, wait: u32 },
    ResetScores { wait: u32 },
    ResetGameScene { wait: u32 },
    WhiteFade { dir: i32, i: u32 },
    SwitchMenu { prev: usize, new: usize, i: u32 },
    SwitchMenuOnOff { on: bool, i: u32 },
    FadeIntroEnd { time: u32, wait: u32 },
    Fade { what: Fade, dur: f32, dir: i32, i: u32, deactivate: bool },
    MoveMap { dir: i32, i: u32, acc: f32 },
    Failsafe { wait: u32 },
    FadeInTutorial { wait: u32 },
    FlipOnOff { music: bool, i: u32 },
    FadeSound { src: usize, fade_in: bool, volume: f32, i: u32 },
    StopMusic { src: usize, wait: u32 },
    EndScreen { ranking: [[i32; 2]; 8], count: i32, step: u32, wait: u32 },
    ReenableMenu { wait: u32 },
    Deactivate { node: usize, wait: u32 },
    TutorialPhase { phase: i32, wait: u32 },
    ScaleDownVelocity { node: usize, i: u32 },
}

impl Game {
    pub(super) fn step_tasks(&mut self) {
        let tasks = std::mem::take(&mut self.tasks);
        let mut keep = Vec::with_capacity(tasks.len());
        for t in tasks {
            if let Some(t) = self.step_task(t) {
                keep.push(t);
            }
        }
        // tasks started while stepping run from the next step
        keep.append(&mut self.tasks);
        self.tasks = keep;
    }

    /// Advance one task; Some(task) keeps it alive.
    fn step_task(&mut self, t: Task) -> Option<Task> {
        use Task::*;
        macro_rules! waiting {
            ($w:expr, $t:expr) => {
                if $w > 0 {
                    $w -= 1;
                    return Some($t);
                }
            };
        }
        match t {
            TouchReset { player, mv, aim, mut wait } => {
                waiting!(wait, TouchReset { player, mv, aim, wait });
                self.touch_reset(player, mv, aim);
                None
            }
            ResetChargeTimer { player, mut wait } => {
                waiting!(wait, ResetChargeTimer { player, wait });
                self.reset_charge_timer(player);
                None
            }
            ScaleBackChargeBar { player } => self.scale_back_charge_bar(player).then_some(ScaleBackChargeBar { player }),
            Unignore { a, b, mut wait } => {
                waiting!(wait, Unignore { a, b, wait });
                self.phys.set_ignore(a, b, false);
                None
            }
            ThrowRotation { node, i, n } => {
                if i >= n {
                    return None;
                }
                let frozen = self.w.nodes[node].body.as_ref().map(|b| b.constraints == crate::scene::FREEZE_ALL).unwrap_or(true);
                if !frozen {
                    self.w.rotate_self(node, 0.0, 0.0, (20 - i / 10) as f32);
                }
                Some(ThrowRotation { node, i: i + 1, n })
            }
            ResetBeingThrown { b, stage, mut wait } => {
                waiting!(wait, ResetBeingThrown { b, stage, wait });
                if stage == 0 {
                    self.boxes[b].being_thrown = true;
                    Some(ResetBeingThrown { b, stage: 1, wait: super::ticks(0.5) })
                } else {
                    self.boxes[b].being_thrown_pickup = false;
                    self.boxes[b].being_thrown = false;
                    None
                }
            }
            ReinstateCollision { player, mut wait } => {
                waiting!(wait, ReinstateCollision { player, wait });
                self.reinstate_collision(player);
                None
            }
            ScalePlayer { player, i } => self.scale_player_step(player, i).then_some(ScalePlayer { player, i: i + 1 }),
            BoxRespawn { b, mut wait } => {
                waiting!(wait, BoxRespawn { b, wait });
                self.box_respawn(b);
                None
            }
            RaveLine { b, index, mut j, mut a, mut r1, mut r2 } => {
                self.rave_line_step(b, index, &mut j, &mut a, &mut r1, &mut r2).then_some(RaveLine { b, index, j, a, r1, r2 })
            }
            Respawn { dm, player, mut wait } => {
                if wait == super::ticks(super::matchplay::RESPAWN_SECS) {
                    self.set_aimer(false);
                }
                waiting!(wait, Respawn { dm, player, wait });
                self.respawn(dm, player);
                None
            }
            CamShake { cam, i, d } => {
                if i >= 8 {
                    return None;
                }
                self.w.translate_world(cam, d);
                Some(CamShake { cam, i: i + 1, d: -d })
            }
            OrbFly { blob, orb, killer, v, i } => {
                // physical: thrown back along the hit, braked by drag, while
                // the killer's pull quietly starts to win
                const PHYSICAL: u32 = 24;
                let p = self.w.position(orb);
                let k = self.w.position(killer);
                let to = v3(k.x - p.x, k.y - p.y, 0.0);
                let l = to.length().max(1e-4);
                // the pull only bites at the very end of the flight
                let pull = to * ((i as f32 / PHYSICAL as f32).powi(4) * 0.02 / l);
                let v = v * 0.885 + pull;
                self.w.translate_world(orb, v);
                if i >= PHYSICAL {
                    // the change of domain: a breath of ink, then it is a soul
                    let col = self.soul_color(killer);
                    let mut c = col;
                    c[3] = 0.5;
                    self.motion.ring(false, p, 0.03, 0.22, 0.012, 0.25, c);
                    self.cue("soul", p, (orb % 5) as f32);
                    self.tasks.push(OrbSeek { blob, orb, killer, step: 0.02, v });
                    return None;
                }
                Some(OrbFly { blob, orb, killer, v, i: i + 1 })
            }
            OrbSeek { blob, orb, killer, mut step, mut v } => self.orb_seek(blob, orb, killer, &mut step, &mut v).then_some(OrbSeek { blob, orb, killer, step, v }),
            OrbScale { orb, j } => {
                let s = self.w.nodes[orb].lscale;
                if j < 20 {
                    self.w.nodes[orb].lscale = s + v3(0.01, 0.01, 0.01);
                } else if j < 90 {
                    if s != V3::ZERO {
                        self.w.nodes[orb].lscale = s - v3(0.01, 0.01, 0.01);
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
                Some(OrbScale { orb, j: j + 1 })
            }
            ShowerScaleUp { s, mut i, mut wait } => {
                waiting!(wait, ShowerScaleUp { s, i, wait });
                self.shower_step(s, &mut i, &mut wait).then_some(ShowerScaleUp { s, i, wait })
            }
            PauseFade { pause, fade_in, i } => {
                if i >= 30 {
                    if fade_in {
                        self.pause_elements(pause, true);
                    }
                    return None;
                }
                let a = if fade_in { (i + 1) as f32 / 30.0 } else { 1.0 - (i + 1) as f32 / 30.0 };
                if let Some(wf) = self.pauses[pause].white_fade {
                    self.image_alpha(wf, a);
                }
                Some(PauseFade { pause, fade_in, i: i + 1 })
            }
            DelayedQuit { pause, mut wait } => {
                waiting!(wait, DelayedQuit { pause, wait });
                self.delayed_quit(pause);
                None
            }
            ResetScores { mut wait } => {
                waiting!(wait, ResetScores { wait });
                self.reset_scores();
                None
            }
            ResetGameScene { mut wait } => {
                waiting!(wait, ResetGameScene { wait });
                self.reset_game_scene();
                None
            }
            WhiteFade { dir, i } => self.white_fade_step(dir, i).then_some(WhiteFade { dir, i: i + 1 }),
            SwitchMenu { prev, new, i } => {
                if i + 1 < 30 {
                    return Some(SwitchMenu { prev, new, i: i + 1 });
                }
                self.w.set_active(prev, false);
                self.w.set_active(new, true);
                self.fx_form_subtree(new);
                self.tasks.push(WhiteFade { dir: -1, i: 0 });
                None
            }
            SwitchMenuOnOff { on, i } => {
                if i == 29 {
                    if let Some(e) = self.menu.event_system {
                        self.w.set_active(e, on);
                    }
                }
                if i == 89 {
                    if let Some(c) = self.menu.camera {
                        self.w.set_active(c, on);
                    }
                    return None;
                }
                Some(SwitchMenuOnOff { on, i: i + 1 })
            }
            FadeIntroEnd { time, mut wait } => {
                waiting!(wait, FadeIntroEnd { time, wait });
                self.set_intro(false);
                if let Some(wf) = self.menu.white_fade_general {
                    self.tasks.push(Fade { what: super::menu::Fade::Sprite(wf), dur: time.max(1) as f32, dir: -1, i: 0, deactivate: true });
                }
                None
            }
            Fade { what, dur, dir, i, deactivate } => self.fade_step(what, dur, dir, i, deactivate).then_some(Fade { what, dur, dir, i: i + 1, deactivate }),
            MoveMap { dir, i, acc } => {
                // eased slide with a soft overshoot (was a 9-step linear brake)
                const N: u32 = 16;
                if i >= N {
                    self.menu.move_map_started = false;
                    return None;
                }
                let e = |u: f32| {
                    let v = u - 1.0;
                    1.0 + 2.4 * v * v * v + 1.4 * v * v
                };
                let step = e((i + 1) as f32 / N as f32) - e(i as f32 / N as f32);
                let acc = step * 9.0 + acc * 0.0;
                if let Some(m) = self.menu.maps {
                    if let Some(r) = self.w.nodes[m].rect.as_mut() {
                        r.pos[0] += acc * dir as f32 * 600.0 / 9.0;
                    } else {
                        self.w.nodes[m].lpos.x += acc * dir as f32 * 600.0 / 9.0;
                    }
                }
                Some(MoveMap { dir, i: i + 1, acc: 0.0 })
            }
            Failsafe { mut wait } => {
                waiting!(wait, Failsafe { wait });
                self.menu.start_round_failsafe = false;
                None
            }
            FadeInTutorial { mut wait } => {
                waiting!(wait, FadeInTutorial { wait });
                self.fade_in_tutorial();
                None
            }
            FlipOnOff { music, i } => self.flip_step(music, i).then_some(FlipOnOff { music, i: i + 1 }),
            FadeSound { src, fade_in, volume, i } => self.fade_sound_step(src, fade_in, volume, i).then_some(FadeSound { src, fade_in, volume, i: i + 1 }),
            StopMusic { src, mut wait } => {
                waiting!(wait, StopMusic { src, wait });
                self.play_stop(src, false);
                None
            }
            EndScreen { ranking, count, step, mut wait } => {
                waiting!(wait, EndScreen { ranking, count, step, wait });
                self.end_screen_stage(ranking, count, step).map(|next| EndScreen { ranking, count, step: next, wait: super::ticks(1.0) })
            }
            ReenableMenu { mut wait } => {
                waiting!(wait, ReenableMenu { wait });
                self.reenable_menu();
                None
            }
            Deactivate { node, mut wait } => {
                waiting!(wait, Deactivate { node, wait });
                self.w.set_active(node, false);
                None
            }
            TutorialPhase { phase, mut wait } => {
                waiting!(wait, TutorialPhase { phase, wait });
                if let Some(t) = self.tutorial.as_mut() {
                    t.phase = phase;
                }
                None
            }
            ScaleDownVelocity { node, i } => {
                if i >= 30 {
                    return None;
                }
                if let Some(b) = self.w.nodes[node].body.as_mut() {
                    b.velocity = b.velocity * 0.5;
                }
                Some(ScaleDownVelocity { node, i: i + 1 })
            }
        }
    }
}
