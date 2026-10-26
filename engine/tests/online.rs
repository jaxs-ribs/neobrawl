//! Two Engines as two browsers: each builds the online match itself from
//! the shared seed (on different screen sizes), packets cross with ~40 ms
//! each way, both people play (scripted hands), and GGRS's desync
//! detection compares checksums every 10 frames for the whole match.

mod common;
use common::*;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[test]
fn two_browsers_play_an_online_match_in_sync() {
    let read = |f: &str| std::fs::read_to_string(format!("{}/../web/assets/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let (scene, assets) = (read("scene.json"), read("assets.json"));
    let mut a = neobrawl::Engine::new(&scene, &assets, "{}", 1, 1280.0, 720.0);
    let mut b = neobrawl::Engine::new(&scene, &assets, r#"{"Sound":"OFF"}"#, 2, 1600.0, 1000.0);
    a.net_begin(777, 1, 0, 1);
    b.net_begin(777, 1, 1, 1);
    let mut wire: [VecDeque<(Instant, Vec<u8>)>; 2] = [VecDeque::new(), VecDeque::new()];
    let lag = Duration::from_millis(40);
    let mut events = [String::new(), String::new()];
    let mut minds = [a.game().ai[0].clone(), b.game().ai[1].clone()];
    for m in minds.iter_mut() {
        m.on = true;
        m.level = 4;
    }
    let mut next = Instant::now();
    let mut synced_at = None;
    for t in 0..2500u32 {
        for (k, e) in [&mut a, &mut b].into_iter().enumerate() {
            // a strong bot plays each side, but only through that machine's
            // own controls, from what that machine sees
            let mut brain = e.game().clone();
            brain.ai = Default::default();
            brain.ai[k] = minds[k].clone();
            let d = brain.ai_decide()[k];
            minds[k] = brain.ai[k].clone();
            e.direct(0, d.mv.x, d.mv.y, d.aim.x, d.aim.y, d.aiming, false, true);
            e.dash(0, d.dash);
            e.frame();
            loop {
                let p = e.net_pop();
                if p.is_empty() {
                    break;
                }
                wire[k].push_back((Instant::now() + lag, p));
            }
            let ev = e.net_events();
            if !ev.is_empty() {
                events[k] += &(ev + ",");
            }
        }
        let now = Instant::now();
        for (k, e) in [&mut b, &mut a].into_iter().enumerate() {
            while wire[k].front().map(|(at, _)| *at <= now).unwrap_or(false) {
                e.net_recv(&wire[k].pop_front().unwrap().1);
            }
        }
        if synced_at.is_none() && a.net_stats()[1] == 1.0 && b.net_stats()[1] == 1.0 {
            synced_at = Some(t);
        }
        next += Duration::from_millis(20);
        if let Some(d) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
    }
    let (sa, sb) = (a.net_stats(), b.net_stats());
    eprintln!("synced at tick {synced_at:?}; events A [{}] B [{}]", events[0], events[1]);
    eprintln!("A ping {} rb/s {:.1} depth {:.1}; B ping {} rb/s {:.1} depth {:.1}", sa[2], sa[3], sa[4], sb[2], sb[3], sb[4]);
    let ka: serde_json::Value = serde_json::from_str(&a.state()).unwrap();
    let kb: serde_json::Value = serde_json::from_str(&b.state()).unwrap();
    eprintln!("A score {} B score {}", ka["score"], kb["score"]);
    let kills = |v: &serde_json::Value| v["score"]["p1"].as_i64().unwrap_or(0) + v["score"]["p2"].as_i64().unwrap_or(0);
    assert!(kills(&ka) + kills(&kb) >= 2 || ka["score"]["ended"] == true, "too few kills to mean anything");
    assert!(synced_at.map(|t| t < 100).unwrap_or(false), "never synced");
    assert!(!events[0].contains("desync") && !events[1].contains("desync"), "desync!");
    assert!(!events[0].contains("disconnected") && !events[1].contains("disconnected"));
}

/// A match plays to its end on both machines (one kill here), both see it
/// end, then a rematch on a new seed syncs and plays in step again.
#[test]
fn a_match_ends_on_both_sides_and_a_rematch_syncs() {
    let read = |f: &str| std::fs::read_to_string(format!("{}/../web/assets/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let (scene, assets) = (read("scene.json"), read("assets.json"));
    let mut a = neobrawl::Engine::new(&scene, &assets, "{}", 1, 1280.0, 720.0);
    let mut b = neobrawl::Engine::new(&scene, &assets, "{}", 2, 1440.0, 900.0);
    let mut events = [String::new(), String::new()];
    for round in 0..2u64 {
        a.net_begin(100 + round, round as u32, 0, 1);
        b.net_begin(100 + round, round as u32, 1, 1);
        for e in [&mut a, &mut b] {
            for s in e.game_mut().scores.iter_mut() {
                s.max_score = 8;
            }
        }
        let mut minds = [a.game().ai[0].clone(), b.game().ai[1].clone()];
        for m in minds.iter_mut() {
            m.on = true;
            m.level = 4;
        }
        let mut wire: [VecDeque<(Instant, Vec<u8>)>; 2] = [VecDeque::new(), VecDeque::new()];
        let mut next = Instant::now();
        let mut ended = [false; 2];
        for _ in 0..3000u32 {
            for (k, e) in [&mut a, &mut b].into_iter().enumerate() {
                if !ended[k] {
                    let mut brain = e.game().clone();
                    brain.ai = Default::default();
                    brain.ai[k] = minds[k].clone();
                    let d = brain.ai_decide()[k];
                    minds[k] = brain.ai[k].clone();
                    e.direct(0, d.mv.x, d.mv.y, d.aim.x, d.aim.y, d.aiming, false, true);
                    e.dash(0, d.dash);
                }
                e.frame();
                for p in std::iter::from_fn(|| Some(e.net_pop()).filter(|p| !p.is_empty())) {
                    wire[k].push_back((Instant::now() + Duration::from_millis(30), p));
                }
                let ev = e.net_events();
                if ev.contains("ended") {
                    ended[k] = true;
                }
                events[k] += &ev;
            }
            let now = Instant::now();
            for (k, e) in [&mut b, &mut a].into_iter().enumerate() {
                while wire[k].front().map(|(at, _)| *at <= now).unwrap_or(false) {
                    e.net_recv(&wire[k].pop_front().unwrap().1);
                }
            }
            if ended == [true; 2] {
                break;
            }
            next += Duration::from_millis(20);
            if let Some(d) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(d);
            }
        }
        let sa: serde_json::Value = serde_json::from_str(&a.state()).unwrap();
        let sb: serde_json::Value = serde_json::from_str(&b.state()).unwrap();
        eprintln!("round {round}: ended {ended:?}; A {} B {}", sa["score"], sb["score"]);
        assert_eq!(ended, [true; 2], "both machines saw the match end");
        assert_eq!(sa["score"], sb["score"], "same final score");
    }
    assert!(!events[0].contains("desync") && !events[1].contains("desync"), "{events:?}");
}
