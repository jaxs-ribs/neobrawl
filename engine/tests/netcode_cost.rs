mod common;
use common::*;
#[test]
#[ignore]
fn cost() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 5);
    run(&mut e, 40); tap(&mut e, "StartingScreenButton"); run(&mut e, 70); tap(&mut e, "MapMenuGameStart"); run(&mut e, 100);
    let g = e.game().clone();
    let t = std::time::Instant::now();
    for _ in 0..50 { let c = g.clone(); std::hint::black_box(&c); }
    eprintln!("Game clone: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 50.0);
    let t = std::time::Instant::now();
    for _ in 0..50 { let c = g.w.nodes.clone(); std::hint::black_box(&c); }
    eprintln!("  nodes clone: {:.3} ms ({} nodes)", t.elapsed().as_secs_f64() * 1000.0 / 50.0, g.w.nodes.len());
    let t = std::time::Instant::now();
    for _ in 0..50 { let c: Vec<_> = g.w.nodes.iter().map(|n| n.scripts.clone()).collect(); std::hint::black_box(&c); }
    eprintln!("  of which scripts: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 50.0);
    let t = std::time::Instant::now();
    for _ in 0..50 { let c = g.layout.clone(); std::hint::black_box(&c); }
    eprintln!("  layout clone: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 50.0);
    let mut g2 = g.clone();
    for _ in 0..5 { g2.frame(); }
    let snap = g2.clone();
    g2.frame();
    eprintln!("nodes copied by one tick after a snapshot: {} of {}", g2.w.nodes.len() - g2.w.nodes.shared(), g2.w.nodes.len());
    let t = std::time::Instant::now();
    for _ in 0..50 { let s = g2.clone(); g2.frame(); std::hint::black_box(&s); }
    eprintln!("snapshot + tick: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 50.0);
    drop(snap);
    let t = std::time::Instant::now();
    for _ in 0..200 { g2.frame(); }
    let pr = g2.prof.borrow_mut().take();
    let names = neobrawl::prof::PHASES;
    for (k, n) in names.iter().enumerate() { if 2 + k * 2 < pr.len() { eprintln!("  {n:10} {:.3}", pr[2 + k * 2]); } }
    eprintln!("tick: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 200.0);
    let t = std::time::Instant::now();
    for _ in 0..200 { std::hint::black_box(g2.checksum()); }
    eprintln!("checksum: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 200.0);
}
