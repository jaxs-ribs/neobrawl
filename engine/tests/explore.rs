mod common;
use common::*;

#[test]
#[ignore]
fn explore() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 1);
    run(&mut e, 60);
    println!("boot: {}", e.state());
    println!("start btn at {:?}", e.locate("StartingScreenButton"));
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 80);
    println!("after start: {}", e.state());
    println!("go at {:?}", e.locate("MapMenuGameStart"));
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 120);
    println!("in game: {}", e.state());
    let r = e.render_vec();
    println!("records {}", r.len() / 28);
}

#[test]
#[ignore]
fn trace_throw() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 3);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    let g = e.game();
    let p1 = g.w.with_tag("PlayerOneScreen1").unwrap();
    let p2 = g.w.with_tag("PlayerOneScreen2").unwrap();
    // teleport a box into p1's hands
    let k = g.player_of[&p1];
    let b = g.boxes.iter().find(|b| g.w.active_in_hierarchy(b.node)).unwrap().node;
    e.game_mut().pick_box_up_pub(k, b);
    let a0 = e.game().w.position(p1);
    e.game_mut().w.set_position(p2, neobrawl::shared::math::v3(a0.x + 4.0, a0.y, a0.z));
    let a = e.game().w.position(p1);
    let c = e.game().w.position(p2);
    println!("p1 {:?} p2 {:?}", a, c);
    let (dx, dy) = (c.x - a.x, c.y - a.y);
    let l = dx.hypot(dy);
    for t in 0..70 {
        e.stick(0, 0.0, 0.0, dx / l, dy / l, t < 60);
        e.frame();
        let g = e.game();
        let bp = g.w.position(b);
        let v = g.w.nodes[b].body.as_ref().unwrap().velocity;
        if t > 55 {
            println!("t{t} touch {:?} prev {:?} pp {:?} carry {:?} charge {} box {:.2},{:.2} v {:.1},{:.1} col {}", g.players[k].touch[1], g.players[k].prev[1], g.players[k].prev2[1], g.players[k].picked_box, g.players[k].charge_timer, bp.x, bp.y, v.x, v.y, g.w.nodes[b].collider.as_ref().unwrap().enabled);
        }
    }
    for t in 0..20 {
        e.frame();
        let g = e.game();
        let bp = g.w.position(b);
        let v = g.w.nodes[b].body.as_ref().unwrap().velocity;
        let ev: Vec<_> = g.phys.events.iter().map(|e| (g.w.nodes[e.a].name.clone(), g.w.nodes[e.b].name.clone(), e.enter)).collect();
        println!("events {:?}", ev);
        println!("after t{t} box {:.2},{:.2} v {:.1},{:.1} p2 {:?} raving {}", bp.x, bp.y, v.x, v.y, g.w.position(p2), g.boxes[g.box_of[&b]].raving);
    }
}

#[test]
#[ignore]
fn list_visible_ui() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 1);
    run(&mut e, 120);
    let g = e.game();
    for &i in g.ui_draw_order().iter() {
        let n = &g.w.nodes[i];
        if !g.w.active_in_hierarchy(i) { continue; }
        if let Some(img) = n.image.as_ref().filter(|x| x.enabled && x.color[3] > 0.0) {
            println!("IMG {} sprite={:?} color={:?} kind={} quad={:?}", n.name, img.sprite.map(|s| g.w.sprites[s].name.clone()), img.color, img.kind, g.ui_screen_quad(i).map(|q| [q[0], q[2]]));
        }
        if let Some(t) = n.text.as_ref().filter(|x| x.enabled && x.color[3] > 0.0) {
            println!("TXT {} {:?} size={} quad={:?}", n.name, t.text, t.size, g.ui_screen_quad(i).map(|q| [q[0], q[2]]));
        }
    }
    // world sprites seen by the menu camera
    let cams: Vec<usize> = (0..g.w.nodes.len()).filter(|&i| g.w.nodes[i].camera.is_some() && g.w.active_in_hierarchy(i)).collect();
    for c in cams { println!("CAM {} pos {:?}", g.w.nodes[c].name, g.w.position(c)); }
    let mut cnt = 0;
    for i in 0..g.w.nodes.len() { if g.w.nodes[i].sprite.as_ref().map(|s| s.enabled).unwrap_or(false) && g.w.active_in_hierarchy(i) && !g.layout.canvas_of.contains_key(&i) { cnt += 1; if cnt < 6 { println!("SPR {} {:?}", g.w.nodes[i].name, g.w.position(i)); } } }
    println!("world sprites {cnt}");
}
