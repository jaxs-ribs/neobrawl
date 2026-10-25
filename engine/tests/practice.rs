mod common;
use common::*;
use neobrawl::shared::math::v3;

#[test]
fn target_practice_arena_targets_and_respawns() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 3);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuArrowRight"); // Map 2
    run(&mut e, 30);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    assert!(e.start_practice(), "practice starts in a match");
    let g = e.game();
    assert_eq!(g.practice.targets.len(), 6);
    let c = g.practice.center;
    // the boxes stand in a grid around the centre
    let near = g.boxes.iter().filter(|b| g.w.active_in_hierarchy(b.node) && (g.w.position(b.node) - c).length() < 4.5).count();
    assert!(near >= 30, "box field in the middle: {near}");
    let p1 = g.players.iter().find(|p| g.w.nodes[p.node].tag == "PlayerOneScreen1").unwrap().node;
    let k1 = g.player_of[&p1];
    // a simple shooter: walk into the field for a box, aim at the nearest live target, throw
    let mut charge_ticks = 0;
    let mut deaths_seen = 0;
    let mut back_in = Vec::new();
    let mut down_at: Vec<Option<u64>> = vec![None; 6];
    for f in 0..(50 * 60) {
        let g = e.game();
        let me = g.w.position(p1);
        let carrying = g.players[k1].picked_box.is_some();
        let target = g.practice.targets.iter().filter(|t| t.dead_until.is_none()).map(|t| g.w.position(t.node)).min_by(|a, b| (*a - me).length().partial_cmp(&(*b - me).length()).unwrap());
        if !carrying {
            charge_ticks = 0;
            let d = c - me;
            let l = d.length().max(1e-3);
            e.direct(0, d.x / l, d.y / l, 0.0, 0.0, false, false, true);
        } else if let Some(tp) = target {
            let d = tp - me;
            let l = d.length().max(1e-3);
            let dir = v3(d.x / l, d.y / l, 0.0);
            let side = v3(-dir.y, dir.x, 0.0);
            // a lane clear for the box's width (the field is dense)
            let clear = [-0.45f32, 0.0, 0.45].iter().all(|&o| g.phys.raycast(&g.w, me + dir * 0.6 + side * o, dir, l - 0.6, 1 | (1 << 8) | (1 << 11)).is_none());
            if !clear || l > 7.0 {
                charge_ticks = 0;
                // step out of the field toward the target
                e.direct(0, dir.x, dir.y, 0.0, 0.0, false, false, true);
            } else {
                charge_ticks += 1;
                let release = charge_ticks > 30;
                e.direct(0, 0.0, 0.0, dir.x, dir.y, !release, false, true);
                if release {
                    charge_ticks = 0;
                }
            }
        }
        e.frame();
        let g = e.game();
        for (k, t) in g.practice.targets.iter().enumerate() {
            let dead = t.dead_until.is_some();
            match (dead, down_at[k]) {
                (true, None) => {
                    down_at[k] = Some(f);
                    deaths_seen += 1;
                }
                (false, Some(d)) => {
                    back_in.push(f - d);
                    down_at[k] = None;
                }
                _ => {}
            }
            // targets keep to their zones (dodges may overshoot a little)
            if !dead {
                let p = g.w.position(t.node);
                let off = (v3(p.x, p.y, 0.0) - v3(t.zone.x, t.zone.y, 0.0)).length();
                assert!(off < t.radius + 2.5, "target {k} strayed {off:.1} from a {:.1} zone", t.radius);
            }
        }
    }
    let g = e.game();
    eprintln!("hits {}, best streak {}, respawn ticks {:?}", g.practice.hits, g.practice.streak, &back_in[..back_in.len().min(8)]);
    assert!(g.practice.hits >= 3 && deaths_seen as u32 >= g.practice.hits, "the shooter scores: {}", g.practice.hits);
    assert!(back_in.iter().all(|&t| t <= 30), "back within ~0.6 s: {back_in:?}");
}
