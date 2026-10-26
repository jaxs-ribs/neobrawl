//! How smooth each element looks: sample exactly what the renderer draws
//! at 120 Hz (souls, blades, trail heads, camera), playing the same script
//! with netcode off and through the rollback lab at 110 ms round trip, and
//! compare the jerk (second difference per frame) of each.

mod common;
use common::*;
use neobrawl::shared::math::V3;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Series {
    pts: Vec<V3>,
}

impl Series {
    fn push(&mut self, p: V3) {
        self.pts.push(p);
    }
    fn break_here(&mut self) {
        self.pts.push(V3 { x: f32::NAN, y: 0.0, z: 0.0 });
    }
    /// mean and 99th percentile of |p[i] - 2p[i-1] + p[i-2]|, skipping gaps
    /// and teleports
    fn jerk(&self) -> (f32, f32) {
        let mut j = Vec::new();
        for w in self.pts.windows(3) {
            if w.iter().any(|p| !p.x.is_finite()) {
                continue;
            }
            if (w[1] - w[0]).length() > 1.0 || (w[2] - w[1]).length() > 1.0 {
                continue;
            }
            j.push((w[2] - w[1] * 2.0 + w[0]).length());
        }
        if j.is_empty() {
            return (0.0, 0.0);
        }
        j.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (j.iter().sum::<f32>() / j.len() as f32, j[(j.len() as f32 * 0.99) as usize])
    }
}

fn play(lag: Option<f32>) -> Vec<(&'static str, Series)> {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 21);
    run(&mut e, 40);
    tap(&mut e, "StartingScreenButton");
    run(&mut e, 70);
    tap(&mut e, "MapMenuGameStart");
    run(&mut e, 100);
    e.set_desktop(true);
    e.set_ai(1, true);
    e.set_ai_level(1, 4);
    if let Some(rtt) = lag {
        e.set_lag(rtt / 2.0, rtt / 11.0, 0.01, 1);
    }
    let names = ["my soul", "my blade dir", "my blade reach", "their soul", "their blade dir", "their blade reach", "my trail head", "their trail head", "camera"];
    let mut s: Vec<(&'static str, Series)> = names.iter().map(|n| (*n, Series::default())).collect();
    let me = e.game().players.iter().position(|p| &*e.game().w.nodes[p.node].tag == "PlayerOneScreen1").unwrap();
    let them = e.game().players.iter().position(|p| &*e.game().w.nodes[p.node].tag == "PlayerOneScreen2").unwrap();
    let start = Instant::now();
    let mut ticks = 0u32;
    let mut frame = 0u32;
    // a 120 Hz display; the game ticks at 50 Hz in real time
    while ticks < 1100 {
        let t = frame as f32 / 120.0;
        let target = Duration::from_secs_f32(t);
        if let Some(d) = target.checked_sub(start.elapsed()) {
            std::thread::sleep(d);
        }
        // the hand: circle, charge for a second and a half, release; the
        // mouse drifts smoothly round the screen
        let a = t * 1.4;
        e.direct(0, a.cos(), a.sin(), 0.0, 0.0, (t * 0.6).fract() < 0.7, true, true);
        e.cursor(640.0 + 300.0 * (t * 0.7).cos(), 360.0 + 200.0 * (t * 0.9).sin());
        while (ticks as f32) < t * 50.0 {
            e.frame();
            ticks += 1;
        }
        e.set_subtick(t * 50.0 - ticks as f32 + 1.0);
        let (souls, cam) = e.game().shown();
        let push = |s: &mut Series, v: Option<V3>| match v {
            Some(p) if p.x.is_finite() && p.x > -600.0 => s.push(p),
            _ => s.break_here(),
        };
        for (who, base) in [(me, 0usize), (them, 3)] {
            let (c, blade, head) = souls[who];
            push(&mut s[base].1, Some(c));
            push(&mut s[base + 1].1, blade.map(|(d, _)| d));
            push(&mut s[base + 2].1, blade.map(|(_, h)| V3 { x: h.min(12.0), y: 0.0, z: 0.0 }));
            push(&mut s[6 + base / 3].1, head);
        }
        push(&mut s[8].1, cam);
        frame += 1;
    }
    s
}

#[test]
fn every_element_stays_smooth_through_rollback() {
    let off = play(None);
    let net = play(Some(110.0));
    eprintln!("{:<18} {:>14} {:>14}   ratio (mean)", "element", "off mean/p99", "110ms mean/p99");
    let mut worst = (0.0f32, "");
    for ((name, a), (_, b)) in off.iter().zip(net.iter()) {
        let (am, ap) = a.jerk();
        let (bm, bp) = b.jerk();
        // below ~0.004 per frame (units or radians) nobody can see it, and
        // the real-time bot makes tiny numbers noisy: those pass outright
        let r = if bm < 0.004 { 1.0 } else if am > 1e-5 { bm / am } else { 1.0 };
        eprintln!("{:<18} {:>6.4}/{:<7.4} {:>6.4}/{:<7.4}   {:.2}", name, am, ap, bm, bp, r);
        // a blade's reach jumps by nature as it slides off an edge, and the
        // edges it meets depend on the bot's path: judge it more loosely
        let r = if name.ends_with("reach") { r * 0.64 } else { r };
        if r > worst.0 {
            worst = (r, name);
        }
    }
    eprintln!("worst: {} at {:.2}x", worst.1, worst.0);
    assert!(worst.0 < 1.6, "{} is {:.2}x as jerky through rollback", worst.1, worst.0);
}

