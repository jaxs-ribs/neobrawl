mod common;
use common::*;

fn showcase_match(seed: u64, secs: u32) -> (neobrawl::Engine, [u32; 2], u32) {
    let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    for s in 0..2 {
        e.set_ai_level(s, 4);
        e.set_ai(s, true);
    }
    let nodes: Vec<usize> = e.game().players.iter().filter(|p| !p.single && !p.playback).map(|p| p.node).collect();
    let mut still = [0u32; 2];
    let mut worst_still = 0u32;
    let mut last = [neobrawl::shared::math::V3::ZERO; 2];
    let mut dashes = [0u32; 2];
    for _ in 0..(50 * secs) {
        e.frame();
        for (k, &n) in nodes.iter().take(2).enumerate() {
            let p = e.game().w.position(n);
            if p.x == -666.0 {
                still[k] = 0;
                continue;
            }
            if (p - last[k]).length() < 0.01 { still[k] += 1; worst_still = worst_still.max(still[k]); } else { still[k] = 0; }
            last[k] = p;
            let pk = e.game().player_of[&n];
            if e.game().players[pk].dash_t == 6 {
                dashes[k] += 1;
            }
        }
    }
    (e, dashes, worst_still)
}

/// Not a gate: how a showcase fight goes, for tuning and before recording.
#[test]
#[ignore]
fn showcase_report() {
    for seed in 1u64..5 {
        let (e, dashes, still) = showcase_match(seed, 120);
        let g = e.game();
        let s = state(&e);
        let ends: Vec<_> = g.throw_ends.iter().map(|(n, v)| (g.w.nodes[*n].name.clone(), *v)).collect();
        eprintln!(
            "seed {seed}: score {}-{} (kills {}-{}), throws {:?}, dashes {:?}, longest standstill {} ticks, ends [soul, box, other] {:?}",
            s["score"]["p1"], s["score"]["p2"],
            s["score"]["p1"].as_i64().unwrap_or(0) / 8, s["score"]["p2"].as_i64().unwrap_or(0) / 8,
            [g.ai[0].stats[0], g.ai[1].stats[0]], dashes, still, ends
        );
    }
}
