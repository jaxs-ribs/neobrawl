//! The title starts a mode directly, without the menu screens.

mod common;
use common::*;

#[test]
fn the_title_goes_straight_into_each_mode() {
    for mode in ["practice", "anthill"] {
        let mut e = engine(r#"{"FirstLaunch":1}"#, 7);
        run(&mut e, 40);
        assert!(e.play_mode(mode), "{mode}: the title takes it");
        let mut up = None;
        for f in 0..300 {
            e.frame();
            let g = e.game();
            if up.is_none() && (g.practice.on || g.anthill.on) {
                up = Some(f);
            }
        }
        let g = e.game();
        eprintln!("{mode}: up after {up:?} frames, screens {}", e.state());
        assert!(up.is_some(), "{mode} starts");
        assert_eq!(g.practice.on, mode == "practice");
        assert_eq!(g.anthill.on, mode == "anthill");
    }
}
