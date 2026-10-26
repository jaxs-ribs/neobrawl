mod common;
use common::*;

/// Not a gate: a wider sample of hard vs easy and of dash catches, for tuning.
#[test]
#[ignore]
fn sweep() {
    let (mut hard, mut easy) = (0, 0);
    let mut throws = 0;
    for seed in 20u64..32 {
        let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
        run(&mut e, 40);
        tap(&mut e, "StartingScreenButton");
        run(&mut e, 70);
        tap(&mut e, "MapMenuGameStart");
        run(&mut e, 100);
        e.set_desktop(true);
        e.set_ai_level(1, 3);
        e.set_ai(1, true);
        e.set_ai_level(0, 1);
        e.set_ai(0, true);
        for _ in 0..(50 * 60) {
            e.frame();
        }
        let s = state(&e);
        easy += s["score"]["p1"].as_i64().unwrap_or(0);
        hard += s["score"]["p2"].as_i64().unwrap_or(0);
        throws += e.game().ai[1].stats[0];
    }
    eprintln!("12 seeds: hard {hard} vs easy {easy} (points, 8 per kill); hard throws {:.1} per minute", throws as f32 / 12.0);
}

/// Not a gate: how kills split between jabs and loaded throws (and bank
/// shots) when strong bots fight, for tuning the charge curve.
#[test]
#[ignore]
fn kill_split() {
    for level in [3u8, 4] {
        let (mut jab, mut loaded, mut banked, mut kills) = (0, 0, 0, 0);
        let (mut jab_throws, mut throws) = (0, 0);
        let mut charges = Vec::new();
        for seed in 40u64..52 {
            let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
            run(&mut e, 40);
            tap(&mut e, "StartingScreenButton");
            run(&mut e, 70);
            tap(&mut e, "MapMenuGameStart");
            run(&mut e, 100);
            e.set_desktop(true);
            for s in 0..2 {
                e.set_ai_level(s, level);
                e.set_ai(s, true);
            }
            run(&mut e, 50 * 60);
            let g = e.game();
            let j = g.jab_threshold();
            throws += g.throw_log.len();
            jab_throws += g.throw_log.iter().filter(|&&c| c < j).count();
            for &(c, b) in &g.kill_log {
                kills += 1;
                charges.push(c);
                if c < j { jab += 1 } else { loaded += 1 }
                if b > 0 { banked += 1 }
            }
        }
        charges.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = |p: f32| charges.get(((charges.len() as f32 - 1.0) * p) as usize).copied().unwrap_or(0.0);
        eprintln!("level {level}: {throws} throws, {jab_throws} jabs ({:.0}%); jab hit rate {:.0}%, loaded {:.0}%", jab_throws as f32 * 100.0 / throws.max(1) as f32, jab as f32 * 100.0 / jab_throws.max(1) as f32, loaded as f32 * 100.0 / (throws - jab_throws).max(1) as f32);
        eprintln!("level {level}, 12 min: {kills} kills - jab {jab} ({:.0}%), loaded {loaded}, off a wall {banked}; kill charge p25 {:.0} p50 {:.0} p75 {:.0}", jab as f32 * 100.0 / kills.max(1) as f32, q(0.25), q(0.5), q(0.75));
    }
}
