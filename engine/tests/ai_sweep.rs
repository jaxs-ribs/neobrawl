mod common;
use common::*;

/// Not a gate: a wider sample of hard vs easy and of dash catches, for tuning.
#[test]
#[ignore]
fn sweep() {
    let (mut hard, mut easy) = (0, 0);
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
    }
    eprintln!("12 seeds: hard {hard} vs easy {easy}");
}
