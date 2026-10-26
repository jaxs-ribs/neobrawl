mod common;
use common::*;

fn match_with_ai(seed: u64, level: u8) -> neobrawl::Engine {
    let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    e.set_ai_level(1, level);
    e.set_ai(1, true);
    e
}

fn score(e: &neobrawl::Engine) -> (i64, i64) {
    let s = state(e);
    (s["score"]["p1"].as_i64().unwrap_or(0), s["score"]["p2"].as_i64().unwrap_or(0))
}

/// Against a soul that just stands there, the hard fighter must score.
#[test]
fn hard_fighter_kills_a_standing_target() {
    let mut kills = 0;
    for seed in [1u64, 2, 3] {
        let mut e = match_with_ai(seed, 3);
        for _ in 0..(50 * 30) {
            e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
            e.frame();
            if score(&e).1 > 0 {
                break;
            }
        }
        kills += (score(&e).1 > 0) as i32;
        eprintln!("seed {seed}: score {:?} frame {}", score(&e), state(&e)["frame"]);
    }
    assert!(kills >= 2, "the fighter scored in {kills}/3 matches against a sitting target");
}

/// Hard against easy, both fighting: hard should win over twelve minutes of play.
#[test]
fn hard_beats_easy() {
    let (mut hard, mut easy) = (0, 0);
    for seed in 20u64..32 {
        let mut e = match_with_ai(seed, 3);
        e.set_ai_level(0, 1);
        e.set_ai(0, true);
        for _ in 0..(50 * 60) {
            e.frame();
            if state(&e)["score"]["ended"].as_bool().unwrap_or(false) {
                break;
            }
        }
        let (p1, p2) = score(&e);
        let _ = (p1, p2);
        easy += p1;
        hard += p2;
    }
    // with human reaction times nobody dodges a close throw, so the margin
    // comes from aim and positioning: hard must still win overall
    assert!(hard as f32 > easy as f32 * 1.15, "hard {hard} vs easy {easy}");
}

/// MGE pacing: after a kill the victim is back within ~0.6 s, at the
/// spawn farthest from the killer, and the camera swoops.
#[test]
fn respawn_is_instant_and_far_from_the_killer() {
    let mut e = match_with_ai(2, 3);
    let p1 = e.game().players.iter().find(|p| &*e.game().w.nodes[p.node].tag == "PlayerOneScreen1").map(|p| p.node).unwrap();
    // let the fighter kill player 1
    let mut died_at = None;
    for f in 0..(50 * 20) {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
        e.frame();
        if e.game().w.position(p1).x == -666.0 {
            died_at = Some(f);
            break;
        }
    }
    let died_at = died_at.expect("the fighter got a kill");
    let mut back = None;
    for f in 0..60 {
        e.frame();
        if e.game().w.position(p1).x != -666.0 {
            back = Some(f);
            break;
        }
    }
    let back = back.expect("respawned");
    assert!(back <= 30, "back in {back} ticks after dying at {died_at}");
    let killer = e.game().last_killer[&p1];
    let me = e.game().w.position(p1);
    let them = e.game().w.position(killer);
    assert!((me - them).length() > 6.0, "respawned far from the killer: {}", (me - them).length());
    assert!(e.game().cam_follow.iter().any(|c| c.swoop.is_some()), "the camera is swooping");
}

/// Flanking: dashing around the bot at close range. Returns (deaths, of
/// which caught by a throw within ~0.25 s of a dash).
fn flank(seeds: std::ops::Range<u64>) -> (u32, u32) {
    let (mut caught, mut deaths) = (0u32, 0u32);
    for seed in seeds {
        let mut e = match_with_ai(seed, 3);
        let p1 = e.game().players.iter().find(|p| &*e.game().w.nodes[p.node].tag == "PlayerOneScreen1").map(|p| p.node).unwrap();
        let bot = e.game().players.iter().find(|p| &*e.game().w.nodes[p.node].tag == "PlayerOneScreen2").map(|p| p.node).unwrap();
        let mut h = seed.wrapping_mul(0x9e3779b97f4a7c15) | 1;
        let mut rnd = move || {
            h ^= h << 13;
            h ^= h >> 7;
            h ^= h << 17;
            (h % 10_000) as f32 / 10_000.0
        };
        let (mut side, mut dash_at, mut since_dash, mut alive) = (1.0f32, 30u32, 1000u32, true);
        for f in 0..(50 * 60) {
            let me = e.game().w.position(p1);
            let b = e.game().w.position(bot);
            let (mut mx, mut my) = (0.0, 0.0);
            let mut dash = false;
            if me.x != -666.0 && b.x != -666.0 {
                let (dx, dy) = (b.x - me.x, b.y - me.y);
                let d = (dx * dx + dy * dy).sqrt().max(1e-3);
                let (ux, uy) = (dx / d, dy / d);
                // hold ~4.5 units and circle; dash sideways around it now and then
                let radial = ((d - 4.5) * 0.8).clamp(-1.0, 1.0);
                mx = ux * radial - uy * side;
                my = uy * radial + ux * side;
                if f >= dash_at {
                    dash = true;
                    if rnd() < 0.5 {
                        side = -side;
                        mx = -uy * side;
                        my = ux * side;
                    }
                    dash_at = f + 30 + (rnd() * 50.0) as u32;
                }
            }
            e.direct(0, mx, my, 0.0, 0.0, false, true, true);
            e.dash(0, dash);
            e.frame();
            e.dash(0, false);
            since_dash = if dash { 0 } else { since_dash + 1 };
            let now_alive = e.game().w.position(p1).x != -666.0;
            // ricochets off walls are physics, not the bot's tracking: only
            // direct hits count here
            let direct = e.game().kill_log.last().map(|k| k.1 == 0).unwrap_or(true);
            if alive && !now_alive && direct {
                deaths += 1;
                if since_dash < 13 {
                    caught += 1;
                }
            }
            alive = now_alive;
        }
    }
    (deaths, caught)
}

/// A dash breaks the bot's tracking like a person's: dashing around it at
/// close range, it should rarely catch you mid-dash. (The old dash-reading
/// tracking caught about 60% more of these; see the numbers below.)
#[test]
fn a_dash_breaks_the_bots_lock() {
    let (deaths, caught) = flank(7..15);
    eprintln!("flanking: {deaths} deaths, {caught} caught around a dash");
    assert!(deaths >= 5, "the bot still fights: {deaths} kills");
    // measured: 11 of 78 (14%); the old dash-reading tracker: 19 of 84 (23%)
    assert!(caught as f32 <= deaths as f32 * 0.18, "caught {caught} of {deaths} around dashes");
}
