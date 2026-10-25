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
            // the lane must be clear for the box's full width, not just its centre
            let side = neobrawl::shared::math::v3(-s, c, 0.0);
            [-0.48f32, 0.0, 0.48].iter().all(|&o| g.phys.raycast(&g.w, a + d * 0.4 + side * o, d, 6.0, 1 | (1 << 8) | (1 << 11)).is_none())
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

#[test]
fn shift_dash_bursts_then_cools_down() {
    let mut e = start_match(4);
    e.set_desktop(true);
    let (p1, _) = player_nodes(&e);
    run(&mut e, 5);
    let x0 = e.game().w.position(p1);
    // dash right while standing still: uses the facing / default direction
    e.direct(0, 1.0, 0.0, 0.0, 0.0, false, true, true);
    e.dash(0, true);
    e.frame();
    e.dash(0, false);
    run(&mut e, 6);
    let x1 = e.game().w.position(p1);
    let moved = (x1 - x0).length();
    // walking 7 ticks covers ~1 unit; the dash adds ~2.9
    assert!(moved > 2.4, "dash distance {moved}");
    let k = e.game().player_of[&p1];
    if e.game().players[k].picked_box.is_some() {
        // dashed into a box: picking it up refills the dash (pick-swing)
        assert_eq!(e.game().players[k].dash_cd, 0, "pickup refills the dash");
        return;
    }
    assert!(e.game().players[k].dash_cd > 0, "cooldown running");
    // pressing again during cooldown does nothing
    e.dash(0, true);
    e.frame();
    e.dash(0, false);
    assert_eq!(e.game().players[k].dash_t, 0, "no dash during cooldown");
    run(&mut e, 50);
    e.dash(0, true);
    e.frame();
    assert!(e.game().players[k].dash_t > 0, "dash available again after 1 s");
}

#[test]
fn orbit_aim_throws_along_the_reticle_regardless_of_camera() {
    let mut e = start_match(5);
    e.set_desktop(true);
    assert!(fetch_box_desktop(&mut e));
    let (p1, _) = player_nodes(&e);
    let k = e.game().player_of[&p1];
    let b = e.game().players[k].picked_box.unwrap();
    // charge for 40 ticks while the camera does whatever it does
    for _ in 0..40 {
        e.direct(0, 0.0, 0.0, 0.0, 1.0, true, false, true);
        e.frame();
    }
    // then swing the reticle onto a lane that is clear right now for the
    // spinning box's sweep (~0.6 from its centre): grazing a neighbour's
    // corner rightly shatters both
    let a = e.game().w.position(p1);
    let (sy, cx) = (0..32)
        .map(|k| (k as f32 / 32.0 * std::f32::consts::TAU).sin_cos())
        .find(|&(s, c)| {
            let g = e.game();
            let d = neobrawl::shared::math::v3(c, s, 0.0);
            let side = neobrawl::shared::math::v3(-s, c, 0.0);
            [-0.65f32, -0.3, 0.0, 0.3, 0.65].iter().all(|&o| g.phys.raycast(&g.w, a + side * o, d, 3.5, 1 | (1 << 8) | (1 << 11)).is_none())
        })
        .unwrap();
    for _ in 0..3 {
        e.direct(0, 0.0, 0.0, cx, sy, true, false, true);
        e.frame();
    }
    e.direct(0, 0.0, 0.0, cx, sy, false, false, true);
    e.frame();
    e.frame();
    let v = e.game().w.nodes[b].body.as_ref().unwrap().velocity;
    let along = v.x * cx + v.y * sy;
    let across = (v.x * -sy + v.y * cx).abs();
    // within 5 degrees of the reticle
    assert!(along > 15.0 && across < along * 0.09, "thrown along the reticle ({cx}, {sy}): {v:?}");
}

#[test]
fn picking_up_a_box_refills_the_dash() {
    let mut e = start_match(6);
    e.set_desktop(true);
    let (p1, _) = player_nodes(&e);
    let k = e.game().player_of[&p1];
    let b = {
        let g = e.game();
        g.boxes.iter().find(|b| g.w.active_in_hierarchy(b.node)).unwrap().node
    };
    e.game_mut().players[k].dash_cd = 40;
    e.game_mut().pick_box_up_pub(k, b);
    assert_eq!(e.game().players[k].dash_cd, 0);
    // and the moment is drawn
    assert!(!e.game().motion.fx.is_empty(), "pickup leaves motion graphics");
}

#[test]
fn souls_stretch_when_moving_and_squash_when_stopping() {
    let mut e = start_match(8);
    e.set_desktop(true);
    let (p1, _) = player_nodes(&e);
    let k = e.game().player_of[&p1];
    let mut max_st = 0.0f32;
    for _ in 0..25 {
        e.direct(0, 0.0, 1.0, 0.0, 0.0, false, true, true);
        e.frame();
        max_st = max_st.max(e.game().bodies[k].st);
    }
    assert!(max_st > 0.08, "stretches along its run: {max_st}");
    let mut min_st = 0.0f32;
    for _ in 0..25 {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
        e.frame();
        min_st = min_st.min(e.game().bodies[k].st);
    }
    assert!(min_st < -0.03, "squashes as it stops: {min_st}");
    // and the deformation reaches the renderer
    assert!(!e.game().body_deforms().is_empty());
}

#[test]
fn snake_keeps_moving_turns_on_a_press_and_crawls_while_charging() {
    let mut e = start_match(9);
    e.set_desktop(true);
    e.set_snake(true);
    let (p1, _) = player_nodes(&e);
    let k = e.game().player_of[&p1];
    // no input at all: it still moves
    let a = e.game().w.position(p1);
    for _ in 0..10 {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
        e.frame();
    }
    let b = e.game().w.position(p1);
    assert!((b - a).length() > 0.5, "always moving: {}", (b - a).length());
    // a press of up turns the heading up within a few ticks, and it stays
    // up after the key is released
    e.direct(0, 0.0, 1.0, 0.0, 0.0, false, true, true);
    e.frame();
    for _ in 0..6 {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
        e.frame();
    }
    let h = e.game().players[k].heading;
    assert!(h.y > 0.95, "turned up: {h:?}");
    // a dash goes along the heading
    e.dash(0, true);
    e.frame();
    e.dash(0, false);
    let d = e.game().players[k].dash_dir;
    assert!(d.y > 0.95 && e.game().players[k].dash_t > 0, "dash along heading: {d:?}");
    // holding a box and charging slows to a crawl
    let bx = {
        let g = e.game();
        g.boxes.iter().find(|b| g.w.active_in_hierarchy(b.node)).unwrap().node
    };
    e.game_mut().pick_box_up_pub(k, bx);
    for _ in 0..20 {
        e.direct(0, 0.0, 0.0, 0.0, 1.0, true, false, true);
        e.frame();
    }
    let pace = e.game().players[k].pace;
    assert!(pace < 0.35, "crawling while charging: {pace}");
}

#[test]
fn snake_escapes_corners_and_turns_away_from_walls() {
    let mut e = start_match(10);
    e.set_desktop(true);
    e.set_snake(true);
    let (p1, _) = player_nodes(&e);
    // run with no input for a long time: it must never stay stuck
    let mut stuck = 0;
    let mut worst = 0;
    let mut last = e.game().w.position(p1);
    for _ in 0..600 {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
        e.frame();
        let now = e.game().w.position(p1);
        if (now - last).length() < 0.02 { stuck += 1; worst = worst.max(stuck); } else { stuck = 0; }
        last = now;
    }
    assert!(worst < 8, "stuck for {worst} ticks");
}

#[test]
fn a_point_blank_charged_throw_kills() {
    let mut e = start_match(4);
    e.set_desktop(true);
    assert!(fetch_box_desktop(&mut e));
    let (p1, p2) = player_nodes(&e);
    let k = e.game().player_of[&p1];
    // charge fully, aiming right
    for _ in 0..95 {
        e.direct(0, 0.0, 0.0, 1.0, 0.0, true, false, true);
        e.frame();
    }
    // put the enemy right against us, overlapping the carried box
    let a = e.game().w.position(p1);
    e.game_mut().w.set_position(p2, neobrawl::shared::math::v3(a.x + 0.45, a.y, a.z));
    e.game_mut().phys.forget(p2);
    assert!(e.game().players[k].picked_box.is_some());
    e.direct(0, 0.0, 0.0, 1.0, 0.0, false, false, true);
    let mut killed = false;
    for _ in 0..15 {
        e.frame();
        killed |= e.game().w.position(p2).x == -666.0;
    }
    assert!(killed, "point-blank charged throw must kill");
}

#[test]
fn the_wanderer_travels_and_never_attacks() {
    let mut e = start_match(11);
    e.set_desktop(true);
    e.set_ai(1, true);
    let (p1, p2) = player_nodes(&e);
    let k2 = e.game().player_of[&p2];
    let start = e.game().w.position(p2);
    let (mut lo, mut hi) = (start, start);
    let mut stuck = 0;
    let mut worst = 0;
    let mut last = start;
    for _ in 0..1000 {
        e.direct(0, 0.0, 0.0, 0.0, 0.0, false, true, true);
        e.frame();
        let p = e.game().w.position(p2);
        if p.x == -666.0 {
            continue;
        }
        lo = neobrawl::shared::math::v3(lo.x.min(p.x), lo.y.min(p.y), 0.0);
        hi = neobrawl::shared::math::v3(hi.x.max(p.x), hi.y.max(p.y), 0.0);
        if (p - last).length() < 0.01 { stuck += 1; worst = worst.max(stuck); } else { stuck = 0; }
        last = p;
        assert!(e.game().players[k2].charge_timer == 0.0, "it never charges a throw");
    }
    let span = (hi.x - lo.x).max(hi.y - lo.y);
    assert!(span > 6.0, "it roams the arena: span {span}");
    assert!(worst < 60, "never stuck for long: {worst} ticks");
    assert!(e.game().w.position(p1).x != -666.0, "and never kills us");
}
