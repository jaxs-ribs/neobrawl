//! The play log: what happened in a match, as events a highlight finder can
//! read (throws, kills, mid-air clashes, banks, dashes, refills, near
//! misses) plus what the bots meant to do (their declared intents). Only
//! kept while `recording` is on: rollback clones the game every tick.

use serde::Serialize;

use super::Game;
use crate::shared::math::V3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Throw,
    Kill,
    /// two thrown boxes met in the air
    Clash,
    Bounce,
    Dash,
    /// a kill gave the dash back
    Refill,
    Pickup,
    /// a live box passed close to a soul without hitting it
    Graze,
    /// a bot announced what it is going for
    Intent,
}

#[derive(Clone, Debug, Serialize)]
pub struct Play {
    /// game tick
    pub t: u64,
    pub kind: Kind,
    /// player index of whoever did it (thrower, killer, dasher), or -1
    pub who: i32,
    /// the other one involved (victim, the other thrower, the grazed), or -1
    pub other: i32,
    pub at: [f32; 2],
    /// which throw this belongs to (0: none)
    pub throw: u32,
    /// kind-specific: charge (throw, kill), miss distance (graze), speed (bounce)
    pub a: f32,
    /// kind-specific: bounces before the kill, flight ticks
    pub b: f32,
    /// intents: the strategy's name
    #[serde(skip_serializing_if = "str::is_empty")]
    pub note: &'static str,
}

impl Game {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn log_play(&mut self, kind: Kind, who: Option<usize>, other: Option<usize>, at: V3, throw: u32, a: f32, b: f32, note: &'static str) {
        if !self.recording {
            return;
        }
        let i = |k: Option<usize>| k.map(|k| k as i32).unwrap_or(-1);
        self.plays.push(Play { t: self.frame, kind, who: i(who), other: i(other), at: [at.x, at.y], throw, a, b, note });
    }

    /// The player index owning a soul node.
    pub(super) fn k_of(&self, node: Option<usize>) -> Option<usize> {
        node.and_then(|n| self.player_of.get(&n).copied())
    }

    /// Near misses: a live box within reach of a soul that isn't its
    /// thrower, logged once per throw and soul at its closest pass.
    pub(super) fn watch_grazes(&mut self) {
        if !self.recording {
            return;
        }
        let souls: Vec<(usize, V3)> = (0..self.players.len())
            .filter(|&k| !self.players[k].playback && !self.players[k].single)
            .map(|k| (k, self.w.position(self.players[k].node)))
            .filter(|(_, p)| p.x != -666.0)
            .collect();
        let mut out = Vec::new();
        for bi in 0..self.boxes.len() {
            let b = &self.boxes[bi];
            if !b.being_thrown || b.throw_id == 0 {
                continue;
            }
            let bp = self.w.position(b.node);
            let thrower = self.k_of(b.carried_by);
            for &(k, p) in &souls {
                if Some(k) == thrower {
                    continue;
                }
                let d = ((bp.x - p.x).powi(2) + (bp.y - p.y).powi(2)).sqrt();
                let key = (b.throw_id, k);
                let best = self.graze_best.get(&key).map(|g| g.0).unwrap_or(f32::MAX);
                if d < 1.2 && d < best {
                    self.graze_best.insert(key, (d, bp, self.frame));
                } else if best < 1.2 && d > best + 0.4 && !self.graze_done.contains(&key) {
                    self.graze_done.insert(key);
                    let (md, at, _) = self.graze_best[&key];
                    out.push((thrower, k, at, b.throw_id, md));
                }
            }
        }
        for (thrower, k, at, id, md) in out {
            self.log_play(Kind::Graze, thrower, Some(k), at, id, md, 0.0, "");
        }
    }

    /// The log as JSON, for the page and the highlight finder.
    pub fn plays_json(&self) -> String {
        serde_json::to_string(&self.plays).unwrap_or_default()
    }
}
