//! How the wheel and the trackpad feel where the system adds no motion of its own: on the Linux
//! and BSD desktops (Wayland, X11) a notch arrives as one jump and a trackpad stops dead when
//! the fingers lift. macOS sends a trackpad's momentum itself and its apps take a wheel's
//! notches as they come, and on the web the browser scrolls the page, so those keep the
//! system's motion. Windows keeps 1:1 for now: a precision touchpad's deltas arrive as the same
//! `WM_MOUSEWHEEL` line deltas a wheel's do, and gliding them is untested.
//!
//! - A mouse wheel's notch glides to where it aims on a short spring instead of jumping there,
//!   and notches that come while it glides add to the same aim, as Chromium's smooth wheel
//!   scrolling does.
//! - A trackpad gesture the platform phases pans its viewport as a touch does
//!   (`runtime/scroll/touch.rs`): past an end on a rubber band, and when the fingers lift it
//!   coasts on the swipe's speed and springs back from an end.
//!
//! Both are off unless the window turns them on (`Runtime::set_wheel_feel`), so tests and apps
//! that drive the wheel by hand keep its 1:1 path. Headless runs keep it too, the same on every
//! platform, unless `--wheel smooth` turns both on.
use super::*;
use crate::machine::{Attempt, Effects};
use crate::runtime::scroll_mode::ScrollMode;
use rune_motion::Spring;

/// The spring a mouse wheel's notch glides on (`GlideTo::Wheel`): about as long as Chromium's
/// wheel animation.
pub(super) const WHEEL_GLIDE: Spring = Spring::new(0.18, 0.0);
/// Seconds of trackpad deltas the release speed averages over, so one uneven delta doesn't
/// set it.
const SPEED_WINDOW: f64 = 0.06;
/// A gesture whose last delta is older than this when the fingers lift was held still first,
/// and doesn't coast (`runtime/input/touch.rs` holds a touch's fling the same).
const FLING_HOLD: f64 = 0.1;
/// The release speed's ceiling in points a second, as a touch fling's.
const MAX_SPEED: f32 = 8000.0;

/// What the wheel does where the system adds no motion of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct WheelFeel {
    /// A mouse wheel's notches glide.
    pub(crate) glide: bool,
    /// A phased trackpad gesture pans with a rubber band and coasts when it ends.
    pub(crate) pan: bool,
}

impl WheelFeel {
    /// Notches glide and trackpad gestures pan, coast and spring back.
    pub(crate) const SMOOTH: WheelFeel = WheelFeel { glide: true, pan: true };

    /// What this platform's windows ask for: `SMOOTH` on the Linux and BSD desktops.
    pub(crate) const NATIVE: WheelFeel =
        if cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd")) { Self::SMOOTH } else { WheelFeel { glide: false, pan: false } };
}

/// A trackpad gesture panning a viewport, with the deltas its release speed is read from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Pan {
    #[default]
    Idle,
    /// Gesture `gesture` pans viewport `id` along its axis: `travel` points from where it
    /// caught it, its last delta at `at`, and `from`, the travel and time the release speed
    /// is measured from.
    Panning { gesture: Attempt, id: NodeId, horizontal: bool, travel: f32, at: f64, from: (f32, f64) },
}

impl Pan {
    /// Whether gesture `gesture` is panning a viewport.
    pub(super) fn holds(self, gesture: Attempt) -> bool {
        matches!(self, Pan::Panning { gesture: g, .. } if g == gesture)
    }

    /// The axis the pan moves, while one runs.
    pub(super) fn horizontal(self) -> Option<bool> {
        match self {
            Pan::Panning { horizontal, .. } => Some(horizontal),
            Pan::Idle => None,
        }
    }
}

/// What happens to a trackpad pan.
#[derive(Clone, Copy, Debug)]
pub(super) enum PanEvent {
    /// Gesture `gesture`'s first delta caught viewport `id` at `now`.
    Caught { gesture: Attempt, id: NodeId, horizontal: bool, now: f64 },
    /// A delta of `d` points along the pan's axis arrived at `now`.
    Delta { gesture: Attempt, d: f32, now: f64 },
    /// The fingers lifted at `now`, or the window lost the gesture.
    Release { now: f64 },
}

/// What carrying out a pan's transition does to its viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PanEffect {
    /// Pan viewport `id` to `travel` points from where the gesture caught it.
    Move { id: NodeId, travel: f32 },
    /// Let viewport `id` go, coasting at `velocity` points a second along the pan's travel.
    Release { id: NodeId, velocity: f32 },
}

/// The pan's next state, and what to do to its viewport.
pub(super) fn step(state: &mut Pan, event: PanEvent) -> Effects<PanEffect> {
    match (*state, event) {
        (current, PanEvent::Caught { gesture, id, horizontal, now }) => {
            // A pan whose end never came lets go before the next gesture takes a viewport.
            let effects = release(current, now);
            *state = Pan::Panning { gesture, id, horizontal, travel: 0.0, at: now, from: (0.0, now) };
            effects
        }
        (Pan::Panning { gesture: g, id, horizontal, travel, at, from }, PanEvent::Delta { gesture, d, now }) if g == gesture => {
            // Once the window has run longer than SPEED_WINDOW, it starts again at the delta
            // before this one, so the speed reads the last few deltas, at least one interval.
            let from = if now - from.1 > SPEED_WINDOW { (travel, at) } else { from };
            let travel = travel + d;
            *state = Pan::Panning { gesture, id, horizontal, travel, at: now, from };
            Effects::one(PanEffect::Move { id, travel })
        }
        (current, PanEvent::Release { now }) => {
            *state = Pan::Idle;
            release(current, now)
        }
        (_, PanEvent::Delta { .. }) => Effects::none(),
    }
}

/// Letting go of the pan `current` holds at `now`, at the speed of its last deltas.
fn release(current: Pan, now: f64) -> Effects<PanEffect> {
    let Pan::Panning { id, travel, at, from, .. } = current else { return Effects::none() };
    let span = at - from.1;
    let velocity = if now - at > FLING_HOLD || span <= 0.0 { 0.0 } else { ((travel - from.0) as f64 / span) as f32 };
    Effects::one(PanEffect::Release { id, velocity: velocity.clamp(-MAX_SPEED, MAX_SPEED) })
}

impl<S: 'static> Runtime<S> {
    /// What the wheel does where the system adds no motion of its own: the window's choice,
    /// `WheelFeel::NATIVE` for its platform. A pan in progress lets go when panning turns off.
    pub(crate) fn set_wheel_feel(&mut self, feel: WheelFeel) {
        if !feel.pan { self.wheel_pan_release(); }
        self.wheel_lock.feel = feel;
    }

    /// Whether a delta scrolls through this module rather than 1:1: a mouse wheel's notch
    /// (`precise` false, no phases) glides, and a phased trackpad delta pans.
    pub(super) fn wheel_feels(&self, precise: bool, phased: bool) -> bool {
        let feel = self.wheel_lock.feel;
        (feel.glide && !precise && !phased) || (feel.pan && phased)
    }

    /// A delta of gesture `gesture` that `wheel_feels` took, on the axis its gesture locked to.
    pub(super) fn wheel_feel(&mut self, gesture: Attempt, x: f32, y: f32, dx: f32, dy: f32, horizontal: bool, phased: bool) {
        self.release_hover_hold();
        self.drop_scroll_holds();
        self.dismiss_help();
        if phased {
            self.wheel_pan(gesture, x, y, dx, dy, horizontal);
        } else if !horizontal && dy != 0.0 {
            self.wheel_glide(x, y, dy);
        } else if horizontal && dx != 0.0 {
            // A Shift + wheel notch on a horizontal viewport keeps the 1:1 path.
            self.wheel_x(gesture, x, y, dx);
        }
    }

    /// A mouse wheel's notch: the viewport under (x, y) glides `dy` points past where it was
    /// already gliding to (positive moves the content down), within its ends.
    fn wheel_glide(&mut self, x: f32, y: f32, dy: f32) {
        let Some(id) = self.scroll_at(x, y) else { return };
        self.touch_scroll_interrupt(id);
        let Some(st) = self.scrolls.get_mut(&id) else { return };
        let aim = match st.mode {
            ScrollMode::Glide { to: GlideTo::Wheel(to), .. } => to.min(st.max),
            _ => st.offset,
        };
        let to = (aim - f64::from(dy)).clamp(0.0, st.max);
        if to == aim && st.glide().is_none() { return; }
        let v = st.glide().unwrap_or_else(|| st.coast());
        st.mode = st.mode.to_glide(v, GlideTo::Wheel(to));
        st.moved();
        log_event!(self, "wheel-glide {dy:.1} to {to:.1}");
        self.scroll_cancels_press(id);
    }

    /// A phased trackpad delta: its gesture's first one catches the viewport under (x, y) as a
    /// touch does, and each pans it along the axis the catch picked.
    fn wheel_pan(&mut self, gesture: Attempt, x: f32, y: f32, dx: f32, dy: f32, horizontal: bool) {
        let now = self.now_clock();
        if !self.wheel_lock.pan.holds(gesture) {
            let vertical = self.touch_scroll_start(x, y);
            let (id, horizontal) = self.touch_scroll_pan(x, y, horizontal, vertical);
            let Some(id) = id else {
                self.wheel_pan_release();
                return;
            };
            self.step_wheel_pan(PanEvent::Caught { gesture, id, horizontal, now });
        }
        let d = if self.wheel_lock.pan.horizontal() == Some(true) { dx } else { dy };
        if d != 0.0 { self.step_wheel_pan(PanEvent::Delta { gesture, d, now }); }
    }

    /// The fingers lifted, or the window lost the gesture: the pan lets go, coasting at the
    /// speed of its last deltas.
    pub(super) fn wheel_pan_release(&mut self) {
        let now = self.now_clock();
        self.step_wheel_pan(PanEvent::Release { now });
    }

    fn step_wheel_pan(&mut self, event: PanEvent) {
        for effect in step(&mut self.wheel_lock.pan, event) {
            match effect {
                PanEffect::Move { id, travel } => self.touch_scroll_move(id, travel),
                PanEffect::Release { id, velocity } => {
                    log_event!(self, "wheel-pan release {velocity:.0}");
                    self.touch_scroll_end(Some(id), velocity);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "wheel_feel_tests.rs"]
mod tests;
