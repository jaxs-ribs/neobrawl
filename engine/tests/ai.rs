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

/// Hard against easy, both fighting: hard should win over three minutes of play.
#[test]
fn hard_beats_easy() {
    let (mut hard, mut easy) = (0, 0);
    for seed in [4u64, 5, 6] {
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
        eprintln!("seed {seed}: easy {p1} - hard {p2}  hard stats [throws, dodges, predash, armed ticks, blocked ticks] {:?}  easy {:?}\n   ends [soul, box, other] {:?}", e.game().ai[1].stats, e.game().ai[0].stats, e.game().throw_ends.iter().map(|(n, v)| (e.game().w.nodes[*n].name.clone(), *v)).collect::<Vec<_>>());
        easy += p1;
        hard += p2;
    }
    // with human reaction times nobody dodges a close throw, so the margin
    // comes from aim and positioning: hard must still win overall
    assert!(hard > easy && hard >= 48, "hard {hard} vs easy {easy}");
}

/// MGE pacing: after a kill the victim is back within ~0.6 s, at the
/// spawn farthest from the killer, and the camera swoops.
#[test]
fn respawn_is_instant_and_far_from_the_killer() {
    let mut e = match_with_ai(2, 3);
    let p1 = e.game().players.iter().find(|p| e.game().w.nodes[p.node].tag == "PlayerOneScreen1").map(|p| p.node).unwrap();
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
