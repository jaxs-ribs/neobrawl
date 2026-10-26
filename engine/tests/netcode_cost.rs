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
    let t = std::time::Instant::now();
    for _ in 0..200 { g2.frame(); }
    eprintln!("tick: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 200.0);
    let t = std::time::Instant::now();
    for _ in 0..200 { std::hint::black_box(g2.checksum()); }
    eprintln!("checksum: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / 200.0);
}
