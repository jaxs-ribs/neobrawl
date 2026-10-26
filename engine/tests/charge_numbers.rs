mod common;
use common::*;
#[test]
#[ignore]
fn numbers() {
    let mut e = engine(r#"{"FirstLaunch":1}"#, 5);
    run(&mut e, 40); tap(&mut e, "StartingScreenButton"); run(&mut e, 70); tap(&mut e, "MapMenuGameStart"); run(&mut e, 100);
    let g = e.game();
    let b = &g.boxes[0];
    let body = g.w.nodes[b.node].body.as_ref().unwrap();
    eprintln!("box drag {} mass? soul radius ...", body.drag);
}
