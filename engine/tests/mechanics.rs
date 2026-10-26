//! Charge by threat, dashing out of a wind-up, and boxes that bounce.

mod common;
use common::*;
use neobrawl::game::{charge_pace, jab_charge, CHARGE_EASE_TICKS, CHARGE_FLOOR, CHARGE_FREE_TICKS, FULL_CHARGE};
use neobrawl::shared::math::{v3, V3};
use neobrawl::Engine;

fn start_match(seed: u64) -> Engine {
    let mut e = engine(r#"{"FirstLaunch":1,"Sound":"ON"}"#, seed);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    e.set_ai(1, false);
    e
}

fn p1(e: &Engine) -> (usize, usize) {
    let g = e.game();
    let n = g.w.with_tag("PlayerOneScreen1").unwrap();
    (n, g.player_of[&n])
}

fn fetch_box(e: &mut Engine) -> bool {
    let (node, k) = p1(e);
    for _ in 0..400 {
        let g = e.game();
        if g.players[k].picked_box.is_some() {
            e.direct(0, 0.0, 0.0, 0.0, 0.0, false, false, true);
            run(e, 10);
            return true;
        }
        let me = g.w.position(node);
        let t = g
            .boxes
            .iter()
            .filter(|b| g.w.active_in_hierarchy(b.node) && !b.raving && g.w.nodes[b.node].collider.as_ref().unwrap().enabled)
            .map(|b| g.w.position(b.node))
            .min_by(|a, b| (*a - me).length().partial_cmp(&(*b - me).length()).unwrap())
            .unwrap();
        let d = t - me;
        let l = (d.x * d.x + d.y * d.y).sqrt().max(1e-3);
        e.direct(0, d.x / l, d.y / l, 0.0, 0.0, false, false, true);
        e.frame();
    }
    false
}

// ------------------------------------------------------------ charge ----

#[test]
fn the_jab_threshold_comes_from_the_box_physics() {
    // drag 5: a throw at charge c covers 0.132 * 1.11c units in 0.25 s
    let j = jab_charge(5.0);
    assert!((24.0..31.0).contains(&j), "jab at {j}");
    // more drag needs more charge to reach the same distance in time
    assert!(jab_charge(8.0) > j && jab_charge(2.0) < j);
}

#[test]
fn charging_is_free_for_three_seconds_then_eases_to_the_floor() {
    for h in [0, 10, 90, CHARGE_FREE_TICKS] {
        assert_eq!(charge_pace(h), 1.0, "held {h}");
    }
    assert_eq!(CHARGE_FREE_TICKS, 150, "3 s at 50 Hz");
    assert!((charge_pace(CHARGE_FREE_TICKS + CHARGE_EASE_TICKS) - CHARGE_FLOOR).abs() < 1e-4);
    assert!((charge_pace(10_000) - CHARGE_FLOOR).abs() < 1e-4);
    let mut last = 1.0;
    for h in CHARGE_FREE_TICKS..CHARGE_FREE_TICKS + CHARGE_EASE_TICKS + 5 {
        let p = charge_pace(h);
        assert!(p <= last + 1e-6, "pace rose at {h}");
        last = p;
    }
}

#[test]
fn charging_in_game_fills_fast_to_the_jab_says_so_once_and_only_slows_after_three_seconds() {
    let mut e = start_match(11);
    assert!(fetch_box(&mut e));
    let (_, k) = p1(&e);
    let j = e.game().jab_threshold();
    e.drain_cues();
    let mut crossed_at = None;
    let mut loaded = 0;
    let mut paces = Vec::new();
    for t in 0..200 {
        e.direct(0, 1.0, 0.0, 0.0, 1.0, true, false, true);
        e.frame();
        let c = e.game().players[k].charge_timer;
        if crossed_at.is_none() && c >= j {
            crossed_at = Some(t);
        }
        loaded += e.drain_cues().matches("\"loaded\"").count();
        paces.push((c, e.game().players[k].pace));
    }
    let crossed_at = crossed_at.expect("never reached the jab");
    // 1.5 per tick until the jab: there in ~j/1.5 ticks, not j
    assert!((crossed_at as f32) < j / 1.5 + 3.0, "jab after {crossed_at} ticks (threshold {j})");
    assert_eq!(loaded, 1, "the loaded cue fires exactly once");
    assert!(paces[..150].iter().all(|(_, p)| *p == 1.0), "the first 3 s of charging cost no speed");
    let (c, p) = *paces.last().unwrap();
    assert_eq!(c, FULL_CHARGE);
    assert!((p - CHARGE_FLOOR).abs() < 1e-3, "pace after sitting on it {p}");
}

#[test]
fn dashing_out_of_a_wind_up_keeps_the_box_and_two_thirds_of_the_charge() {
    let mut e = start_match(12);
    assert!(fetch_box(&mut e));
    let (node, k) = p1(&e);
    for _ in 0..50 {
        e.direct(0, 0.0, 0.0, 0.0, 1.0, true, false, true);
        e.frame();
    }
    let before = e.game().players[k].charge_timer;
    let start = e.game().w.position(node);
    // dash toward open floor (at least 5 units clear)
    let g = e.game();
    let all = 1u32 | (1 << 8) | (1 << neobrawl::physics::LAYER_BOX);
    let (dx, dy) = (0..16)
        .map(|i| (i as f32 * 22.5f32).to_radians())
        .map(|a| (a.cos(), a.sin()))
        .find(|&(x, y)| g.phys.raycast(&g.w, start, v3(x, y, 0.0), 5.0, all).is_none())
        .expect("open floor somewhere");
    e.direct(0, dx, dy, 0.0, 1.0, true, false, true);
    e.dash(0, true);
    e.frame();
    e.dash(0, false);
    let after = e.game().players[k].charge_timer;
    assert!(e.game().players[k].picked_box.is_some(), "still holding the box");
    assert!((after - before * 2.0 / 3.0).abs() <= 1.5, "charge {before} -> {after}");
    // and the dash still goes the full distance
    for _ in 0..8 {
        e.direct(0, dx, dy, 0.0, 1.0, true, false, true);
        e.frame();
    }
    let moved = (e.game().w.position(node) - start).length();
    assert!(moved > 2.6, "dash covered {moved}");
}

// ----------------------------------------------------------- bounces ----

/// Throw box `b` from `from` along `dir` at `speed`, as player 1's throw.
fn launch(e: &mut Engine, b: usize, from: V3, dir: V3, speed: f32, bounces: u8) {
    let (p1n, _) = p1(e);
    let g = e.game_mut();
    let n = g.boxes[b].node;
    g.w.set_position(n, from);
    let v = dir * speed;
    g.w.nodes[n].body.as_mut().unwrap().velocity = v;
    let x = &mut g.boxes[b];
    x.prev_vel = v;
    x.being_thrown = true;
    x.carried_by = Some(p1n);
    x.bounces = bounces;
    x.spent = false;
}

/// A clear lane to a wall: (start, direction, |cos| to the wall normal).
fn lane(e: &Engine, want_square: bool) -> Option<(V3, V3, f32)> {
    let g = e.game();
    let mask = 1u32 | (1 << 8);
    let live: Vec<V3> = g.boxes.iter().filter(|b| g.w.active_in_hierarchy(b.node)).map(|b| g.w.position(b.node)).collect();
    let centre = live.iter().fold(V3::ZERO, |a, p| a + *p) * (1.0 / live.len() as f32);
    for cell in 0..(21 * 21) {
        let o = centre + v3((cell % 21) as f32 - 10.0, (cell / 21) as f32 - 10.0, 0.0);
        // inside the arena, in the open: walls all around, none too close
        let solid = mask | (1 << neobrawl::physics::LAYER_BOX) | (1 << neobrawl::physics::LAYER_PLAYER);
        let open = (0..8).all(|i| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            g.phys.raycast(&g.w, o, v3(a.cos(), a.sin(), 0.0), 25.0, solid).map(|h| h.distance > 0.9).unwrap_or(false)
        });
        if !open {
            continue;
        }
        for i in 0..72 {
            let a = i as f32 * 5f32.to_radians();
            let d = v3(a.cos(), a.sin(), 0.0);
            let Some(h) = g.phys.raycast(&g.w, o, d, 30.0, mask) else { continue };
            // close enough that drag hasn't spent the throw (v drops 5.6 per unit)
            if h.distance < 2.5 || h.distance > if want_square { 6.0 } else { 10.0 } {
                continue;
            }
            // start clear of the other boxes
            if live.iter().any(|p| (*p - o).length() < 1.6) {
                continue;
            }
            // the wall's tangent from two neighbouring rays
            let side = v3(-d.y, d.x, 0.0) * 0.15;
            let (Some(h1), Some(h2)) = (g.phys.raycast(&g.w, o + side, d, 30.0, mask), g.phys.raycast(&g.w, o - side, d, 30.0, mask)) else { continue };
            if h1.node != h.node || h2.node != h.node {
                continue;
            }
            let t = h1.point - h2.point;
            let tl = t.length().max(1e-4);
            let n = v3(-t.y / tl, t.x / tl, 0.0);
            let cos = (n.x * d.x + n.y * d.y).abs();
            // the same flat face across the box's whole width (no wall ends)
            let flat = [-0.6f32, -0.3, 0.3, 0.6].iter().all(|s| {
                g.phys.raycast(&g.w, o + v3(-d.y, d.x, 0.0) * *s, d, 30.0, mask).map(|x| ((x.point - h.point).x * n.x + (x.point - h.point).y * n.y).abs() < 0.08).unwrap_or(false)
            });
            if !flat {
                continue;
            }
            // nothing but the wall in a box-wide corridor
            let clear = [-0.95f32, -0.45, 0.0, 0.45, 0.95].iter().all(|s| g.phys.raycast(&g.w, o + v3(-d.y, d.x, 0.0) * *s, d, 30.0, mask | (1 << neobrawl::physics::LAYER_BOX) | (1 << neobrawl::physics::LAYER_PLAYER)).map(|x| x.distance > h.distance - 0.8).unwrap_or(false));
            if !clear {
                continue;
            }
            if (want_square && cos > 0.97) || (!want_square && (0.25..0.6).contains(&cos)) {
                return Some((o, d, cos));
            }
        }
    }
    None
}

/// A box of the arena in play.
fn live_box(e: &Engine) -> usize {
    let g = e.game();
    (0..g.boxes.len()).find(|&b| g.w.active_in_hierarchy(g.boxes[b].node) && !g.boxes[b].raving).unwrap()
}

fn shattered(e: &Engine, b: usize) -> bool {
    e.game().boxes[b].raving
}

#[test]
fn a_square_fast_hit_shatters_the_box() {
    let mut e = start_match(13);
    let (o, d, cos) = lane(&e, true).expect("a square lane");
    let b = live_box(&e);
    launch(&mut e, b, o, d, 70.0, 0);
    run(&mut e, 40);
    assert!(shattered(&e, b), "cos {cos}: box survived a square hit");
}

#[test]
fn a_glancing_hit_bounces_with_a_spark_and_loses_speed() {
    let mut e = start_match(14);
    let (o, d, cos) = lane(&e, false).expect("a glancing lane");
    let b = live_box(&e);
    launch(&mut e, b, o, d, 70.0, 0);
    e.drain_cues();
    let mut cues = String::new();
    for _ in 0..40 {
        e.frame();
        cues += &e.drain_cues();
        if e.game().boxes[b].bounces > 0 {
            break;
        }
    }
    let x = &e.game().boxes[b];
    assert_eq!(x.bounces, 1, "cos {cos}: no bounce");
    assert!(!x.raving, "a glancing hit must not shatter");
    assert!(cues.contains("\"bounce\""), "the bounce is heard: {cues}");
    let v = e.game().w.nodes[x.node].body.as_ref().unwrap().velocity.length();
    assert!(v < 70.0 * 0.75 && v > 15.0, "speed after the bounce {v}");
    assert!(!x.spent, "one bounce is still live");
}

#[test]
fn a_softer_square_hit_bounces_too() {
    let mut e = start_match(15);
    let (o, d, _) = lane(&e, true).expect("a square lane");
    let b = live_box(&e);
    launch(&mut e, b, o, d, 20.0, 0);
    run(&mut e, 60);
    assert!(!shattered(&e, b), "a slow jab into a wall shouldn't shatter");
}

#[test]
fn the_third_bounce_leaves_it_harmless() {
    let mut e = start_match(16);
    let (o, d, _) = lane(&e, false).expect("a glancing lane");
    let b = live_box(&e);
    launch(&mut e, b, o, d, 70.0, 2);
    for _ in 0..40 {
        e.frame();
        if e.game().boxes[b].bounces > 2 {
            break;
        }
    }
    let x = &e.game().boxes[b];
    assert_eq!(x.bounces, 3);
    assert!(x.spent && !x.being_thrown, "spent after three");
    // and a spent box into a wall does nothing
    let (o2, d2, _) = lane(&e, true).unwrap();
    let mut keep = e.game().boxes[b].clone();
    keep.spent = true;
    let g = e.game_mut();
    let n = g.boxes[b].node;
    g.w.set_position(n, o2);
    g.w.nodes[n].body.as_mut().unwrap().velocity = d2 * 70.0;
    g.boxes[b].prev_vel = d2 * 70.0;
    run(&mut e, 40);
    assert!(!shattered(&e, b), "a spent box can't break");
}

#[test]
fn a_bank_shot_off_a_wall_still_kills() {
    let mut e = start_match(17);
    let (o, d, _) = lane(&e, false).expect("a glancing lane");
    let p2 = e.game().w.with_tag("PlayerOneScreen2").unwrap();
    let b = live_box(&e);
    launch(&mut e, b, o, d, 80.0, 0);
    let score = |e: &Engine| state(e)["score"]["p1"].as_i64().unwrap_or(0);
    let s0 = score(&e);
    let mut placed = false;
    for _ in 0..200 {
        e.frame();
        if !placed && e.game().boxes[b].bounces == 1 {
            // stand in the rebound's path
            let g = e.game_mut();
            let n = g.boxes[b].node;
            let bp = g.w.position(n);
            let v = g.w.nodes[n].body.as_ref().unwrap().velocity;
            let at = bp + v * (1.0 / v.length()) * 2.2;
            g.w.set_position(p2, v3(at.x, at.y, g.w.position(p2).z));
            placed = true;
        }
    }
    assert!(placed, "never bounced");
    assert!(score(&e) > s0, "the rebound killed: {}", e.state());
    assert!(e.game().kill_log.iter().any(|(_, b)| *b == 1), "logged as a one-bounce kill");
}


// ------------------------------------------------------------ aiming ----

/// The riding reticle's assist: a little heavier right over the enemy,
/// never below 0.7, and nothing at all a few degrees off.
#[test]
fn aim_friction_is_subtle_and_only_over_the_enemy() {
    let e = start_match(18);
    let g = e.game();
    let me = g.w.position(g.w.with_tag("PlayerOneScreen1").unwrap());
    let them = g.w.position(g.w.with_tag("PlayerOneScreen2").unwrap());
    let d = them - me;
    let on = e.aim_friction(d.x, d.y);
    assert!(on < 0.75 && on >= 0.7 - 1e-4, "right over the enemy: {on}");
    let a = d.y.atan2(d.x);
    let near = e.aim_friction((a + 0.03).cos() * 3.0, (a + 0.03).sin() * 3.0);
    assert!(near > on && near < 1.0, "the edge eases out: {near}");
    for off in [0.2f32, 1.0, 2.5] {
        assert_eq!(e.aim_friction((a + off).cos() * 3.0, (a + off).sin() * 3.0), 1.0, "{off} rad off");
    }
}
