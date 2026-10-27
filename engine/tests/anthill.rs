//! The anthill: workers bring boxes home in unison, soldiers throw, splash
//! kills, wiped-out kinds overflow their stacks.

mod common;
use common::*;
use neobrawl::game::anthill::{AntCfg, SOLDIER, WORKER};
use neobrawl::shared::math::{v3, V3};
use neobrawl::Engine;

fn anthill(seed: u64, cfg: AntCfg) -> Engine {
    let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuArrowRight");
    run(&mut e, 30);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    assert!(e.game_mut().start_anthill(cfg), "anthill starts in a match");
    e
}

fn stored(e: &Engine) -> usize {
    e.game().anthill.stackers.iter().map(|s| s.boxes.len()).sum()
}

#[test]
fn an_idle_colony_feeds_itself_and_its_soldiers_come_for_you() {
    let mut e = anthill(5, AntCfg::default());
    let g = e.game();
    assert_eq!(g.anthill.stackers.len(), 3);
    let free = g.boxes.iter().filter(|b| g.w.active_in_hierarchy(b.node)).count();
    eprintln!("boxes on the floor: {free}");
    let mut first_home = None;
    let mut first_death = None;
    let mut most_pushers = 0;
    for f in 0..(50 * 90) {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, false, true);
        e.frame();
        let g = e.game();
        if first_home.is_none() && stored(&e) > 0 {
            first_home = Some(f);
        }
        if first_death.is_none() && g.anthill.deaths > 0 {
            first_death = Some(f);
        }
        let mut per: std::collections::HashMap<usize, usize> = Default::default();
        for a in g.anthill.ants.iter().filter(|a| !a.dead && a.kind == WORKER) {
            if let Some(t) = a.target {
                *per.entry(t).or_default() += 1;
            }
        }
        most_pushers = most_pushers.max(per.values().copied().max().unwrap_or(0));
        if f % 500 == 0 {
            let a = &g.anthill;
            eprintln!(
                "t={:>3}s ants {} (soldiers {}) stored {:?} brood {:?} deaths {}",
                f / 50,
                a.ants.iter().filter(|x| !x.dead).count(),
                a.ants.iter().filter(|x| !x.dead && x.kind == SOLDIER).count(),
                a.stackers.iter().map(|s| s.boxes.len()).collect::<Vec<_>>(),
                a.stackers.iter().map(|s| s.brood).collect::<Vec<_>>(),
                a.deaths
            );
        }
    }
    let s = stored(&e);
    eprintln!("first box home at {:?}s, first death at {:?}s, stored {s}, most on one box {most_pushers}", first_home.map(|f| f / 50), first_death.map(|f| f / 50));
    assert!(first_home.is_some(), "workers bring boxes home");
    assert!(most_pushers >= 2, "workers gather on a box");
    assert!(first_death.is_some(), "soldiers kill an idle player");
    // slow going: not the whole floor in a minute and a half
    assert!(s < free, "the colony can't strip the floor that fast: {s}/{free}");
}

#[test]
fn one_worker_barely_shifts_a_box_and_a_few_walk_it() {
    let mut e = anthill(9, AntCfg::default());
    // box speed seen with n pushers in their slots
    let mut seen: [Vec<f32>; 5] = Default::default();
    for _ in 0..(50 * 60) {
        e.frame();
        let g = e.game();
        for &(bi, n) in &g.anthill.pushers {
            let v = g.w.nodes[g.boxes[bi].node].body.as_ref().unwrap().velocity;
            seen[(n as usize).min(4)].push((v.x * v.x + v.y * v.y).sqrt());
        }
    }
    let med = |v: &Vec<f32>| { let mut v = v.clone(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v.get(v.len() / 2).copied().unwrap_or(0.0) };
    for n in 1..5 {
        eprintln!("{n} pushing: median speed {:.2} over {} ticks", med(&seen[n]), seen[n].len());
    }
    assert!(med(&seen[1]) < 0.4, "alone, a worker only nudges a box");
    assert!(!seen[3].is_empty() && med(&seen[3]) > 3.0 * med(&seen[1]), "three in unison walk it");
}

/// A plain fighter: fetch the nearest box, throw it at the nearest ant.
fn fight(e: &mut Engine, secs: u32) -> (u32, u32) {
    let mut charge = 0;
    for _ in 0..(50 * secs) {
        let g = e.game();
        let me_n = g.w.with_tag("PlayerOneScreen1").unwrap();
        let k = g.player_of[&me_n];
        let me = g.w.position(me_n);
        if me.x == -666.0 {
            e.frame();
            continue;
        }
        let carrying = g.players[k].picked_box.is_some();
        let target = g.anthill.ants.iter().filter(|a| !a.dead).map(|a| g.w.position(a.node)).min_by(|a, b| (*a - me).length().partial_cmp(&(*b - me).length()).unwrap());
        if !carrying {
            charge = 0;
            let bp = g
                .boxes
                .iter()
                .filter(|b| g.w.active_in_hierarchy(b.node) && !b.raving && g.w.nodes[b.node].collider.as_ref().map(|c| c.enabled).unwrap_or(false))
                .map(|b| g.w.position(b.node))
                .min_by(|a, b| (*a - me).length().partial_cmp(&(*b - me).length()).unwrap());
            let d = bp.map(|p| p - me).unwrap_or(V3::ZERO);
            let l = d.length().max(1e-3);
            e.direct(0, d.x / l, d.y / l, 0.0, 0.0, false, false, true);
        } else if let Some(tp) = target {
            let d = tp - me;
            let l = d.length().max(1e-3);
            let dir = v3(d.x / l, d.y / l, 0.0);
            if l > 6.0 {
                e.direct(0, dir.x, dir.y, 0.0, 0.0, false, false, true);
                charge = 0;
            } else {
                charge += 1;
                let release = charge > 25;
                e.direct(0, 0.0, 0.0, dir.x, dir.y, !release, false, true);
                if release {
                    charge = 0;
                }
            }
        } else {
            e.direct(0, 0.0, 0.0, 0.0, 0.0, false, false, true);
        }
        e.frame();
    }
    let a = &e.game().anthill;
    (a.kills, a.deaths)
}

#[test]
fn wiping_out_a_kind_spills_its_stack_and_hardens_you() {
    // a small colony so a scripted fighter can finish it
    let cfg = AntCfg { brood: [5, 2], out: [4, 1], ..Default::default() };
    let mut e = anthill(11, cfg);
    let (kills, deaths) = fight(&mut e, 240);
    let a = &e.game().anthill;
    let over = a.stackers.iter().filter(|s| s.overflowed).count();
    eprintln!("kills {kills} deaths {deaths} overflowed {over}/3 shield {} delivered {} won {:?}", a.shield, a.delivered, a.won_at.map(|t| (t - a.started_at) / 50));
    assert!(kills >= 5, "the fighter kills ants");
    assert!(over >= 1, "a wiped-out kind overflows");
    assert!(a.shield as usize + deaths as usize >= over.min(1), "an overflow hardens you");
    // spilled boxes are back on the floor
    for s in a.stackers.iter().filter(|s| s.overflowed) {
        assert!(s.boxes.is_empty());
    }
}

#[test]
fn a_shattering_box_kills_every_ant_near_it() {
    let mut e = anthill(13, AntCfg { hatch: 0.1, ..Default::default() });
    run(&mut e, 120);
    let g = e.game_mut();
    let at = g.anthill.ants.iter().find(|a| !a.dead).map(|a| g.w.position(a.node)).expect("ants out");
    let near = g.anthill.ants.iter().filter(|a| !a.dead && (g.w.position(a.node) - at).length() < g.anthill.cfg.splash).count();
    let kills = g.anthill.kills;
    g.anthill_splash_pub(at);
    assert_eq!(g.anthill.kills - kills, near as u32, "all {near} near the break die");
}

#[test]
fn stacks_never_pass_their_cap_across_settings() {
    for (cap, brood, out) in [(3u32, [12u32, 4u32], [4u32, 2u32]), (10, [20, 2], [8, 1]), (60, [12, 4], [4, 2])] {
        let mut e = anthill(21, AntCfg { cap, brood, out, ..Default::default() });
        let mut most = 0;
        for _ in 0..(50 * 60) {
            e.direct(0, 0.0, 0.0, 0.0, 0.0, false, false, true);
            e.frame();
            most = most.max(e.game().anthill.stackers.iter().map(|s| s.boxes.len()).max().unwrap_or(0));
        }
        let g = e.game();
        eprintln!("cap {cap} brood {brood:?} out {out:?}: tallest stack {most}, stored {}, deaths {}", stored(&e), g.anthill.deaths);
        assert!(most as u32 <= cap);
    }
}

#[test]
fn a_throw_flies_at_the_cursor_even_on_the_run() {
    let (w, h) = (1024.0, 768.0);
    for (i, (cx, cy, mx, my)) in [(900.0, 400.0, 0.0, 1.0), (100.0, 100.0, 1.0, 0.0), (520.0, 700.0, -1.0, -1.0), (300.0, 600.0, 0.0, -1.0)].into_iter().enumerate() {
        let mut e = anthill(30 + i as u64, AntCfg::default());
        e.set_screen(w, h);
        e.cursor(cx, cy);
        // walk to a box and pick it up
        for _ in 0..(50 * 20) {
            let g = e.game();
            let me_n = g.w.with_tag("PlayerOneScreen1").unwrap();
            let k = g.player_of[&me_n];
            if g.players[k].picked_box.is_some() {
                break;
            }
            let me = g.w.position(me_n);
            let bp = g.boxes.iter().filter(|b| g.w.active_in_hierarchy(b.node) && g.w.nodes[b.node].collider.as_ref().map(|c| c.enabled).unwrap_or(false)).map(|b| g.w.position(b.node)).min_by(|a, b| (*a - me).length().partial_cmp(&(*b - me).length()).unwrap()).unwrap();
            let d = bp - me;
            let l = d.length().max(1e-3);
            e.direct(0, d.x / l, d.y / l, 0.0, 0.0, false, true, true);
            e.frame();
        }
        let g = e.game();
        let me_n = g.w.with_tag("PlayerOneScreen1").unwrap();
        let k = g.player_of[&me_n];
        let b = g.players[k].picked_box.expect("picked a box");
        // charge on the run, then let go
        let mut want = V3::ZERO;
        for _ in 0..30 {
            e.direct(0, mx, my, 0.0, 0.0, true, true, true);
            e.frame();
            let g = e.game();
            let c = g.cursor_world(k).unwrap();
            want = c - g.w.position(me_n);
        }
        for _ in 0..2 {
            e.direct(0, mx, my, 0.0, 0.0, false, true, true);
            e.frame();
        }
        let g = e.game();
        let v = g.w.nodes[b].body.as_ref().unwrap().velocity;
        let ang = (v.y.atan2(v.x) - want.y.atan2(want.x)).sin().asin().to_degrees().abs();
        eprintln!("cursor ({cx},{cy}) running ({mx},{my}): off by {ang:.1} deg, speed {:.1}", (v.x * v.x + v.y * v.y).sqrt());
        assert!(ang < 4.0, "the box flies at the cursor: off by {ang:.1} deg");
    }
}
