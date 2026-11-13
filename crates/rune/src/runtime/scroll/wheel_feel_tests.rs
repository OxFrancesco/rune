use super::*;
use crate::{GesturePhase, WheelEvent};

/// A 300 pt page over 800 pt of rows: it scrolls 500 pt.
fn page(_: &(), _: &mut Cx<()>) -> El {
    let rows = (0..20).fold(crate::col().w_full(), |c, i| c.child(crate::el().key(format!("row{i}")).w_full().h(40.0)));
    crate::col().w_full().h_full().child(crate::col().key("page").w_full().h(300.0).scroll(crate::ScrollSpec { stick_bottom: false }).child(rows))
}

fn runtime(feel: WheelFeel) -> (Runtime<()>, NodeId) {
    let mut rt = Runtime::new((), page, (300.0, 300.0), Env::default());
    rt.set_wheel_feel(feel);
    rt.frame(0.0);
    let id = rt.nodes.iter().find(|n| n.key.as_deref() == Some("page")).unwrap().id;
    (rt, id)
}

const ON: WheelFeel = WheelFeel { glide: true, pan: true };

fn notch(rt: &mut Runtime<()>, dy: f32) {
    rt.wheel_event(WheelEvent::new(0.0, dy).at(150.0, 150.0));
}

fn trackpad(rt: &mut Runtime<()>, dy: f32, phase: GesturePhase) {
    rt.wheel_event(WheelEvent::new(0.0, dy).at(150.0, 150.0).precise(true).phase(phase));
}

/// Runs frames of `dt` for `secs`, returning the offset after each.
fn run(rt: &mut Runtime<()>, id: NodeId, secs: f64, dt: f64) -> Vec<f64> {
    (0..(secs / dt).round() as usize).map(|_| { rt.frame(dt); rt.scrolls[&id].offset }).collect()
}

#[test]
fn a_wheel_notch_glides_to_its_aim_instead_of_jumping() {
    let (mut rt, page) = runtime(ON);
    notch(&mut rt, -120.0);
    assert_eq!(rt.scrolls[&page].offset, 0.0, "nothing moves before a frame");
    let path = run(&mut rt, page, 0.6, 1.0 / 60.0);
    assert!(path[0] > 0.0 && path[0] < 60.0, "the first frame moves part of the way: {}", path[0]);
    assert!(path.windows(2).all(|w| w[1] >= w[0] - 1e-9), "it never turns back: {path:?}");
    assert_eq!(*path.last().unwrap(), 120.0);
    assert!(rt.scrolls[&page].glide().is_none(), "and lands");
}

#[test]
fn notches_during_a_glide_add_to_its_aim_and_stop_at_the_end() {
    let (mut rt, page) = runtime(ON);
    notch(&mut rt, -120.0);
    rt.frame(1.0 / 60.0);
    notch(&mut rt, -120.0);
    run(&mut rt, page, 0.6, 1.0 / 60.0);
    assert_eq!(rt.scrolls[&page].offset, 240.0, "two notches, however soon the second came");
    for _ in 0..4 { notch(&mut rt, -120.0); }
    let path = run(&mut rt, page, 0.6, 1.0 / 60.0);
    assert!(path.iter().all(|&o| o <= 500.0), "a wheel notch never overshoots the end: {path:?}");
    assert_eq!(rt.scrolls[&page].offset, 500.0);
    assert!(rt.scrolls[&page].spring.is_none(), "the wheel's spring doesn't outlive its glide");
}

#[test]
fn off_the_wheel_moves_one_to_one() {
    let (mut rt, page) = runtime(WheelFeel::default());
    notch(&mut rt, -120.0);
    assert_eq!(rt.scrolls[&page].offset, 120.0);
    trackpad(&mut rt, 40.0, GesturePhase::Began);
    trackpad(&mut rt, 200.0, GesturePhase::Moved);
    assert_eq!(rt.scrolls[&page].offset, 0.0, "clamped at the top");
}

#[test]
fn a_trackpad_pulls_past_the_top_on_a_rubber_band_and_springs_back() {
    let (mut rt, page) = runtime(ON);
    trackpad(&mut rt, 0.0, GesturePhase::Began);
    for _ in 0..10 {
        trackpad(&mut rt, 20.0, GesturePhase::Moved);
        rt.frame(1.0 / 60.0);
    }
    let pulled = rt.scrolls[&page].offset;
    assert!(pulled < 0.0 && pulled > -200.0 * 0.55, "past the top, less than the fingers moved: {pulled}");
    // The fingers rest before lifting: no fling, only the edge return.
    run(&mut rt, page, 0.2, 1.0 / 60.0);
    assert_eq!(rt.scrolls[&page].offset, pulled, "held while the fingers stay down");
    trackpad(&mut rt, 0.0, GesturePhase::Ended);
    let path = run(&mut rt, page, 1.0, 1.0 / 60.0);
    assert!(path.windows(2).all(|w| w[1] >= w[0] - 1e-9), "springs back without overshooting: {path:?}");
    assert_eq!(rt.scrolls[&page].offset, 0.0);
}

#[test]
fn a_trackpad_swipe_coasts_after_the_fingers_lift_and_bounces_off_the_end() {
    let (mut rt, page) = runtime(ON);
    trackpad(&mut rt, 0.0, GesturePhase::Began);
    for _ in 0..6 {
        trackpad(&mut rt, -30.0, GesturePhase::Moved);
        rt.frame(1.0 / 120.0);
    }
    let lifted = rt.scrolls[&page].offset;
    assert_eq!(lifted, 180.0, "1:1 while the fingers are down");
    trackpad(&mut rt, 0.0, GesturePhase::Ended);
    let path = run(&mut rt, page, 3.0, 1.0 / 60.0);
    let furthest = path.iter().cloned().fold(f64::MIN, f64::max);
    assert!(furthest > 500.0, "coasts on and past the end: {furthest}");
    assert_eq!(rt.scrolls[&page].offset, 500.0, "then springs back to it");
}

#[test]
fn a_pan_the_window_loses_lets_go() {
    let (mut rt, page) = runtime(ON);
    trackpad(&mut rt, 0.0, GesturePhase::Began);
    trackpad(&mut rt, 60.0, GesturePhase::Moved);
    assert!(rt.scrolls[&page].offset < 0.0);
    rt.wheel_lost();
    assert_eq!(rt.wheel_lock.pan, Pan::Idle);
    run(&mut rt, page, 1.0, 1.0 / 60.0);
    assert_eq!(rt.scrolls[&page].offset, 0.0, "springs back without its end event");
}

#[test]
fn the_release_speed_reads_the_last_deltas_and_none_after_a_rest() {
    let (g, id) = (Attempt(1), NodeId(7));
    let mut pan = Pan::Idle;
    let _ = step(&mut pan, PanEvent::Caught { gesture: g, id, horizontal: false, now: 0.0 });
    // A slow start, then 10 pt every 10 ms: 1000 pt/s.
    let _ = step(&mut pan, PanEvent::Delta { gesture: g, d: 1.0, now: 0.1 });
    for i in 1..=10 { let _ = step(&mut pan, PanEvent::Delta { gesture: g, d: 10.0, now: 0.1 + 0.01 * f64::from(i) }); }
    let mut fast = pan;
    let released: Vec<_> = step(&mut fast, PanEvent::Release { now: 0.2 }).into_iter().collect();
    let [PanEffect::Release { velocity, .. }] = released[..] else { panic!("{released:?}") };
    assert!((velocity - 1000.0).abs() < 1.0, "{velocity}");
    assert_eq!(fast, Pan::Idle);
    let rested: Vec<_> = step(&mut pan, PanEvent::Release { now: 0.2 + FLING_HOLD + 0.01 }).into_iter().collect();
    assert_eq!(rested, [PanEffect::Release { id, velocity: 0.0 }]);
}
