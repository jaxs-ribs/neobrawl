//! The lag simulator as the page drives it: a real match through the
//! Engine, you (a scripted hand on the controls) against the bot behind a
//! simulated 110 ms link. Real time, since GGRS keeps wall-clock time.

mod common;
use common::*;
use std::time::{Duration, Instant};

#[test]
fn lag_sim_runs_a_match_through_rollback() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 5);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    e.set_ai(1, true);
    e.set_ai_level(1, 3);
    e.set_lag(55.0, 8.0, 0.01, 2);
    let mut next = Instant::now();
    let mut stats = Vec::new();
    let mut live_at = None;
    for t in 0..750u32 {
        // circle, charge for a second, release; dash now and then
        let a = t as f32 * 0.05;
        e.direct(0, a.cos(), a.sin(), (a * 0.7).cos(), (a * 0.7).sin(), t % 80 < 50, false, true);
        e.dash(0, t % 97 == 0);
        e.frame();
        if t % 50 == 49 {
            let s = e.lag_stats();
            if s[0] == 2.0 && live_at.is_none() {
                live_at = Some(t);
            }
            stats.push(s);
        }
        next += Duration::from_millis(20);
        if let Some(d) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
    }
    let live_at = live_at.expect("the session never started");
    let s = stats.last().unwrap();
    for s in &stats {
        if s[0] == 2.0 {
            eprintln!("ping {:>3.0}  rollbacks/s {:>4.1}  depth {:.1} (max {})  resim/s {:>5.1}  stalls/s {:.1}  ahead {}  tick {:.2} ms (worst {:.2})", s[4], s[5], s[6], s[7], s[8], s[9], s[10], s[11], s[12]);
        }
    }
    assert!(live_at < 150, "handshake took {live_at} ticks");
    assert!(s[4] > 60.0 && s[4] < 200.0, "ping {}", s[4]);
    let roll: f32 = stats.iter().filter(|s| s[0] == 2.0).map(|s| s[5]).sum();
    assert!(roll > 0.0, "the bot's inputs never arrived late?");
    let deepest = stats.iter().filter(|s| s[0] == 2.0).map(|s| s[7]).fold(0.0, f32::max);
    assert!(deepest <= 9.0, "rollbacks too deep: {deepest}");
}

/// Rollback corrections are eased in: the worst on-screen jump of a box or
/// soul in one tick (a remote throw found late, a remote soul corrected)
/// is far smaller drawn with the easing than drawn raw.
#[test]
fn corrections_are_eased_in_on_screen() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 8);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    e.set_ai(1, true);
    e.set_ai_level(1, 4);
    e.set_lag(80.0, 10.0, 0.01, 1);
    let mut next = Instant::now();
    let mut last: std::collections::HashMap<usize, (neobrawl::shared::math::V3, neobrawl::shared::math::V3)> = Default::default();
    let (mut worst_raw, mut worst_eased) = (0f32, 0f32);
    let (mut sum_raw, mut sum_eased, mut n) = (0f32, 0f32, 0);
    for t in 0..1500u32 {
        let a = t as f32 * 0.05;
        e.direct(0, a.cos(), a.sin(), (a * 0.7).cos(), (a * 0.7).sin(), t % 70 < 40, false, true);
        e.frame();
        let g = e.game();
        for node in g.players.iter().map(|p| p.node).chain(g.boxes.iter().map(|b| b.node)) {
            if !g.w.active_in_hierarchy(node) {
                continue;
            }
            let raw = g.w.position(node);
            let eased = raw + g.net_smooth.get(&node).copied().unwrap_or_default();
            if let Some((r0, e0)) = last.get(&node) {
                let (jr, je) = ((raw - *r0).length(), (eased - *e0).length());
                if jr < 4.0 && raw.x != -666.0 && r0.x != -666.0 {
                    worst_raw = worst_raw.max(jr);
                    worst_eased = worst_eased.max(je);
                    if jr > 1.5 {
                        sum_raw += jr;
                        sum_eased += je;
                        n += 1;
                    }
                }
            }
            last.insert(node, (raw, eased));
        }
        next += Duration::from_millis(20);
        if let Some(d) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
    }
    eprintln!("worst jump per tick: raw {worst_raw:.2}, eased {worst_eased:.2}; over {n} big jumps, mean raw {:.2} eased {:.2}", sum_raw / n.max(1) as f32, sum_eased / n.max(1) as f32);
    assert!(n > 0, "no corrections big enough to judge");
    assert!(sum_eased < sum_raw * 0.7, "easing should soften the big jumps");
}
