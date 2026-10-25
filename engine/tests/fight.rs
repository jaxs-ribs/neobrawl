mod common;
use common::*;
use neobrawl::Engine;

fn start_match(seed: u64) -> Engine {
    let mut e = engine(r#"{"FirstLaunch":1}"#, seed);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e
}

fn pos(e: &Engine, node: usize) -> (f32, f32) {
    let p = e.game().w.position(node);
    (p.x, p.y)
}

fn player_nodes(e: &Engine) -> (usize, usize) {
    let g = e.game();
    let p1 = g.w.with_tag("PlayerOneScreen1").unwrap();
    let p2 = g.w.with_tag("PlayerOneScreen2").unwrap();
    (p1, p2)
}

/// Walk player 1 onto the nearest box until it is picked up.
fn fetch_box(e: &mut Engine) -> bool {
    let (p1, _) = player_nodes(e);
    for _ in 0..400 {
        let g = e.game();
        let k = g.player_of[&p1];
        if g.players[k].picked_box.is_some() {
            e.stick(0, 0.0, 0.0, 0.0, 0.0, false);
            return true;
        }
        let me = pos(e, p1);
        let target = g
            .boxes
            .iter()
            .filter(|b| g.w.active_in_hierarchy(b.node) && !b.raving && g.w.nodes[b.node].collider.as_ref().unwrap().enabled)
            .map(|b| pos(e, b.node))
            .min_by(|a, b| {
                let da = (a.0 - me.0).hypot(a.1 - me.1);
                let db = (b.0 - me.0).hypot(b.1 - me.1);
                da.partial_cmp(&db).unwrap()
            })
            .unwrap();
        let (dx, dy) = (target.0 - me.0, target.1 - me.1);
        let l = dx.hypot(dy).max(1e-3);
        e.stick(0, dx / l, dy / l, 0.0, 0.0, true);
        e.frame();
    }
    false
}

#[test]
fn match_starts_with_two_players_on_spawns() {
    let e = start_match(1);
    let s = state(&e);
    assert_eq!(s["screens"][0], "game");
    assert_eq!(s["players"].as_array().unwrap().len(), 2);
}

#[test]
fn walking_into_a_box_picks_it_up() {
    let mut e = start_match(2);
    assert!(fetch_box(&mut e), "picked a box: {}", e.state());
    let s = state(&e);
    assert!(s["players"].as_array().unwrap().iter().any(|p| p["carrying"] == true));
}

#[test]
fn charged_throw_kills_the_other_player_and_scores() {
    let mut e = start_match(3);
    assert!(fetch_box(&mut e));
    let (p1, p2) = player_nodes(&e);
    // put player 2 right next to player 1 so nothing is in between
    let a = e.game().w.position(p1);
    let dir = (0..32)
        .map(|k| (k as f32 / 32.0 * std::f32::consts::TAU).sin_cos())
        .find(|&(s, c)| {
            let g = e.game();
            let d = neobrawl::shared::math::v3(c, s, 0.0);
            let o = a + d * 0.4;
            g.phys.raycast(&g.w, o, d, 6.0, 1 | (1 << 8) | (1 << 11)).is_none()
        })
        .expect("an open direction");
    e.game_mut().w.set_position(p2, neobrawl::shared::math::v3(a.x + dir.1 * 4.0, a.y + dir.0 * 4.0, a.z));
    // clear every other box out of the lane
    let held = { let g = e.game(); g.players[g.player_of[&p1]].picked_box };
    let others: Vec<usize> = e.game().boxes.iter().map(|b| b.node).filter(|&n| Some(n) != held).collect();
    for (k, n) in others.into_iter().enumerate() {
        e.game_mut().w.set_position(n, neobrawl::shared::math::v3(50.0 + 3.0 * k as f32, 50.0, 0.0));
    }
    // aim at player 2 and charge
    let mut killed = false;
    for attempt in 0..6 {
        let a = pos(&e, p1);
        let b = pos(&e, p2);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx.hypot(dy).max(1e-3);
        for _ in 0..60 {
            e.stick(0, 0.0, 0.0, dx / l, dy / l, true);
            e.frame();
        }
        e.stick(0, 0.0, 0.0, 0.0, 0.0, false);
        for _ in 0..30 {
            e.frame();
            if pos(&e, p2).0 == -666.0 {
                killed = true;
            }
        }
        if killed {
            break;
        }
        assert!(fetch_box(&mut e), "refetch box (attempt {attempt}): {}", e.state());
    }
    assert!(killed, "player 2 was hit: {}", e.state());
    assert!(state(&e)["boxesShattered"].as_u64().unwrap() >= 1);
    // the 8 orbs fly to the killer: +8 points
    run(&mut e, 200);
    let s = state(&e);
    assert_eq!(s["score"]["p1"], 8, "{s}");
    // and player 2 respawns after 3 s
    run(&mut e, 100);
    assert_ne!(pos(&e, p2).0, -666.0);
}

#[test]
fn a_click_within_one_frame_still_presses_buttons() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 1);
    run(&mut e, 40);
    let p = e.locate("StartingScreenButton");
    e.pointer(3, p[0], p[1], 0);
    e.pointer(3, p[0], p[1], 2); // same frame
    run(&mut e, 100);
    assert_eq!(state(&e)["screens"][0], "maps", "{}", e.state());
}

#[test]
fn desktop_mouse_aims_at_the_cursor_and_click_release_throws() {
    let mut e = start_match(3);
    e.set_desktop(true);
    assert!(fetch_box_desktop(&mut e), "picked a box: {}", e.state());
    let (p1, p2) = player_nodes(&e);
    let held = { let g = e.game(); g.players[g.player_of[&p1]].picked_box };
    let others: Vec<usize> = e.game().boxes.iter().map(|b| b.node).filter(|&n| Some(n) != held).collect();
    for (k, n) in others.into_iter().enumerate() {
        e.game_mut().w.set_position(n, neobrawl::shared::math::v3(50.0 + 3.0 * k as f32, 50.0, 0.0));
    }
    let a = e.game().w.position(p1);
    let dir = (0..32)
        .map(|k| (k as f32 / 32.0 * std::f32::consts::TAU).sin_cos())
        .find(|&(s, c)| {
            let g = e.game();
            let d = neobrawl::shared::math::v3(c, s, 0.0);
            g.phys.raycast(&g.w, a + d * 0.4, d, 6.0, 1 | (1 << 8) | (1 << 11)).is_none()
        })
        .unwrap();
    e.game_mut().w.set_position(p2, neobrawl::shared::math::v3(a.x + dir.1 * 4.0, a.y + dir.0 * 4.0, a.z));
    let cam = e.game().camera_main().unwrap();
    let target = e.game().w.position(p2);
    let px = e.game().project(cam, target).unwrap();
    e.cursor(px[0], px[1]);
    // holding the button charges and must not move the player
    let before = e.game().w.position(p1);
    for _ in 0..50 {
        // keep the mouse on player 2 while the camera pans and zooms
        let cam = e.game().camera_main().unwrap();
        let t = e.game().w.position(p2);
        let px = e.game().project(cam, t).unwrap();
        e.cursor(px[0], px[1]);
        e.direct(0, 0.0, 0.0, 0.0, 0.0, true, true, true);
        e.frame();
    }
    let after = e.game().w.position(p1);
    assert!((after - before).length() < 0.05, "standing still while charging");
    e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
    let mut killed = false;
    let mut vel = Vec::new();
    for _ in 0..40 {
        e.frame();
        if let Some(b) = held { vel.push(e.game().w.nodes[b].body.as_ref().unwrap().velocity); }
        killed |= e.game().w.position(p2).x == -666.0;
    }
    assert!(killed, "throw went where the cursor was: dir {dir:?} vel {:?} {}", &vel[..4], e.state());
}

fn fetch_box_desktop(e: &mut Engine) -> bool {
    let (p1, _) = player_nodes(e);
    for _ in 0..400 {
        let g = e.game();
        let k = g.player_of[&p1];
        if g.players[k].picked_box.is_some() {
            e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
            for _ in 0..10 {
                e.frame();
            }
            return true;
        }
        let me = g.w.position(p1);
        let t = g
            .boxes
            .iter()
            .filter(|b| g.w.active_in_hierarchy(b.node) && !b.raving && g.w.nodes[b.node].collider.as_ref().unwrap().enabled)
            .map(|b| g.w.position(b.node))
            .min_by(|a, b| (*a - me).length().partial_cmp(&(*b - me).length()).unwrap())
            .unwrap();
        let d = t - me;
        let l = (d.x * d.x + d.y * d.y).sqrt().max(1e-3);
        e.direct(0, d.x / l, d.y / l, 0.0, 0.0, false, true, true);
        e.frame();
    }
    false
}
