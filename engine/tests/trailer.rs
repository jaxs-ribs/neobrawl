//! God matches for the trailer: the bots play, the play log says what
//! happened, and the sweep finds the matches worth filming.

mod common;
use common::*;
use neobrawl::game::plays::Kind;

pub fn god(seed: u64, map: i32) -> neobrawl::Engine {
    let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
    run(&mut e, 40);
    assert!(e.god_match(map), "the title starts a god match");
    e
}

fn tally(e: &neobrawl::Engine) -> String {
    let g = e.game();
    let n = |k: Kind| g.plays.iter().filter(|p| p.kind == k).count();
    let mut intents: std::collections::BTreeMap<&str, usize> = Default::default();
    for p in g.plays.iter().filter(|p| p.kind == Kind::Intent) {
        *intents.entry(p.note).or_default() += 1;
    }
    format!("throws {} kills {} clashes {} bounces {} grazes {} dashes {} refills {} intents {:?}", n(Kind::Throw), n(Kind::Kill), n(Kind::Clash), n(Kind::Bounce), n(Kind::Graze), n(Kind::Dash), n(Kind::Refill), intents)
}

#[test]
fn gods_fight_and_the_log_says_how() {
    let mut e = god(1, 1);
    let t0 = std::time::Instant::now();
    run(&mut e, 50 * 60);
    eprintln!("60 s in {:?}: {}", t0.elapsed(), tally(&e));
    let g = e.game();
    assert!(g.plays.iter().any(|p| p.kind == Kind::Throw), "they throw");
    assert!(g.plays.iter().any(|p| p.kind == Kind::Kill), "they kill");
}

/// Every kill with what the killer said it was doing.
pub fn kills(e: &neobrawl::Engine) -> Vec<(u64, i32, &'static str, f32, f32)> {
    let g = e.game();
    g.plays
        .iter()
        .filter(|p| p.kind == Kind::Kill)
        .map(|p| {
            let why = if p.note.is_empty() { "-" } else { p.note };
            (p.t, p.who, why, p.a, p.b)
        })
        .collect()
}

#[test]
#[ignore]
fn kill_list() {
    for (seed, map) in [(1, 1), (2, 0), (3, 2), (4, 3)] {
    let mut e = god(seed, map);
    run(&mut e, 50 * 60);
    eprintln!("seed {seed} map {map}: {}", tally(&e));
    for k in kills(&e) {
        eprintln!("t {:.1}s p{} {:<8} charge {:.0} bounces+flight {:.3}", k.0 as f32 / 50.0, k.1, k.2, k.3, k.4);
    }
    }
}

/// NB_PLAYS=path seed map ticks: the native play log, to diff against the browser's.
#[test]
#[ignore]
fn dump_plays() {
    let a: Vec<String> = std::env::var("NB_PLAYS").unwrap().split_whitespace().map(String::from).collect();
    let mut e = god(a[1].parse().unwrap(), a[2].parse().unwrap());
    run(&mut e, a[3].parse().unwrap());
    std::fs::write(&a[0], e.plays()).unwrap();
}

#[test]
#[ignore]
fn map_probe() {
    let mut e = god(3, 2);
    for i in 0..6 {
        run(&mut e, 100);
        let g = e.game();
        let s: Vec<_> = g.players.iter().enumerate().filter(|(_, p)| !p.single && !p.playback).map(|(k, p)| (k, g.w.position(p.node), p.picked_box.is_some(), g.input.direct[g.direct_slot_pub(p).unwrap_or(0)].mv)).collect();
        let me = g.w.position(g.players[4].node);
        let hits: Vec<_> = (0..g.w.nodes.len()).filter(|&n| g.w.nodes[n].collider.as_ref().map(|c| c.enabled && !c.trigger).unwrap_or(false) && g.w.active_in_hierarchy(n)).filter_map(|n| { let geo = neobrawl::physics::geometry(&g.w, n)?; neobrawl::physics::ray_geo([me.x, me.y], [1.0, 0.0], &geo).filter(|t| *t < 1.0).map(|t| (g.w.nodes[n].name.to_string(), g.w.nodes[n].layer, t)) }).collect();
        eprintln!("blocking +x within 1: {hits:?} active {:?} map {}", g.players.iter().map(|p| (g.w.nodes[p.node].tag.to_string(), g.w.active_in_hierarchy(p.node), p.single, p.playback)).collect::<Vec<_>>(), g.menu.current_map);
        eprintln!("{i}: screens {} souls {:?} ai {:?}", e.state().chars().take(60).collect::<String>(), s, g.ai.iter().map(|a| (a.on, a.level)).collect::<Vec<_>>());
    }
}

#[test]
#[ignore]
fn map_probe2() {
    for map in 0..4 {
        let mut e = god(1, map);
        run(&mut e, 300);
        let g = e.game();
        let s: Vec<_> = g.players.iter().filter(|p| !p.single && !p.playback).map(|p| g.w.position(p.node)).collect();
        let st = e.state();
        let v: serde_json::Value = serde_json::from_str(&st).unwrap();
        let me = s[0];
        let near: Vec<_> = (0..g.w.nodes.len()).filter(|&n| g.w.nodes[n].collider.as_ref().map(|c| c.enabled && !c.trigger).unwrap_or(false) && g.w.active_in_hierarchy(n) && (g.w.position(n) - me).length() < 2.0).map(|n| (g.w.nodes[n].name.to_string(), g.w.nodes[n].layer)).collect();
        eprintln!("near: {near:?}");
        eprintln!("map {map}: souls {s:?} screens {} paused {}", v["screens"], g.paused);
    }
}

#[test]
fn a_god_match_ignores_the_window_and_drawing() {
    let play = |w: f32, h: f32, draw: bool| {
        let mut e = god(193, 1);
        e.set_screen(w, h);
        for i in 0..2200 {
            e.frame();
            if draw && i % 2 == 0 {
                e.set_subtick(0.5);
                e.render();
            }
        }
        e.plays()
    };
    let a = play(640.0, 360.0, false);
    assert_eq!(a, play(1920.0, 1080.0, false), "the window size changes the match");
    assert_eq!(a, play(640.0, 360.0, true), "drawing changes the match");
}

#[test]
fn both_souls_glide_between_ticks() {
    let mut e = god(193, 1);
    run(&mut e, 600);
    let mut moved = [0.0f32; 2];
    for _ in 0..50 {
        e.frame();
        e.set_subtick(0.1);
        let a = e.souls_on_screen();
        e.set_subtick(0.9);
        let b = e.souls_on_screen();
        for k in 0..2 {
            if a[k * 3 + 2] > 0.0 && b[k * 3 + 2] > 0.0 {
                moved[k] += (a[k * 3] - b[k * 3]).abs() + (a[k * 3 + 1] - b[k * 3 + 1]).abs();
            }
        }
    }
    eprintln!("drawn motion within ticks: {moved:?}");
    assert!(moved[0] > 20.0 && moved[1] > 20.0, "each soul moves between ticks as drawn: {moved:?}");
}
