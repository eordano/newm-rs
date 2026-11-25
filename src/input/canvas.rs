use std::cell::Cell;
use std::time::Duration;

use newm_canvas::{Dir, Edge};
use niri_config::window_rule::Match;
use niri_config::{Action, ModKey, Modifiers, WindowRule};
use niri_ipc::{CanvasDirection, CanvasEdge, CanvasSide};
use smithay::backend::input::InputTime;
use smithay::input::keyboard::{xkb, Keycode, Keysym};
use smithay::utils::{Logical, Point, Serial};

use super::modifiers_from_state;
use crate::layout::canvas::{CanvasZoom, RuleSize};
use crate::niri::State;
use crate::utils::with_toplevel_role;
use crate::window::Mapped;

/// Longest press of Super alone that counts as a tap.
const SUPER_TAP_MS: u32 = 300;

/// How far the pointer moves with the mod key held before it pans the canvas, in logical
/// pixels, so jitter while pressing a Super shortcut never pans.
const PAN_THRESHOLD: f64 = 8.;

/// How long the mod key is held before pointer motion counts toward a pan, in milliseconds: the
/// hand settling as it presses the mod key, on a mouse button above all, pans nothing, and a
/// mod+click (move) or mod+right-drag (resize) that follows starts on the window under the
/// pointer with the camera where it was.
const PAN_DELAY_MS: u32 = 100;

/// Touchpad motion past which a canvas swipe takes its axis: GNOME's swipe threshold.
const SWIPE_THRESHOLD: f64 = 16.;

/// The least touchpad motion one step of a swipe takes: 1/12 of the 1200 a pan takes to scroll
/// across the working area, so small tiles on a wide screen do not step for every fraction of a
/// millimetre the fingers go.
const STEP_MIN: f64 = 100.;

/// The least touchpad motion that moves a window one tile on the camera pulled back to show the
/// whole canvas, where tiles are small: half of [`STEP_MIN`], still three times
/// [`SWIPE_THRESHOLD`], so fingers at rest never step.
const ZOOMED_STEP_MIN: f64 = 50.;

/// Pointer motion with the mod key held and no button.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SuperPan {
    /// Moved this far, short of the threshold.
    Pending(f64, f64),
    /// Panning the camera.
    Panning,
}

/// The touchpad motion one zoom step of a four-finger swipe takes.
const ZOOM_STEP_MOTION: f64 = 100.;

/// What a canvas touchpad swipe does, chosen when it begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwipeAction {
    /// Three fingers: the focus moves once to the window on the side the fingers go, as one
    /// press of Super+H/J/K/L moves it.
    Focus,
    /// Four fingers: sideways, the focus moves once as Super+H/L move it; up pulls the camera
    /// back and down brings it closer, a zoom step at a time, as Super+U and Super+N do.
    FocusOrZoom,
    /// The mod key and three fingers: the camera pans after the fingers, as the mod key with
    /// pointer motion pans it.
    Pan,
    /// The mod key and four fingers: the focused window moves one tile at a time, pushing what
    /// is in the way, as Super+Shift+H/J/K/L move it, while the camera shows the whole canvas.
    Move,
}

impl SwipeAction {
    fn new(fingers: u32, with_mod: bool) -> Option<Self> {
        match (fingers, with_mod) {
            (3, false) => Some(Self::Focus),
            (4, false) => Some(Self::FocusOrZoom),
            (3, true) => Some(Self::Pan),
            (4, true) => Some(Self::Move),
            _ => None,
        }
    }

    fn with_mod(self) -> bool {
        matches!(self, Self::Pan | Self::Move)
    }

    fn step_min(self) -> f64 {
        if self == Self::Move {
            ZOOMED_STEP_MIN
        } else {
            STEP_MIN
        }
    }
}

/// How far a canvas swipe went.
#[derive(Debug, Clone, Copy, PartialEq)]
enum SwipeState {
    /// Moved this far, short of the threshold.
    Pending(f64, f64),
    /// Stepping along one axis, horizontal or not; `rest` is the motion along it that makes no
    /// step yet.
    Stepping { horizontal: bool, rest: f64 },
    /// Zooming the camera; `rest` is the vertical motion that makes no zoom step yet.
    Zooming { rest: f64 },
    /// Panning the camera.
    Panning,
    /// Done: the focus made its one move, or the mod key it began with was released.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Swipe {
    action: SwipeAction,
    state: SwipeState,
}

/// What one update of a canvas swipe does.
#[derive(Debug, PartialEq)]
enum SwipeEffect {
    /// Steps of the swipe's action, in order.
    Steps(Vec<Dir>),
    /// Zoom steps: positive pulls the camera back, as `zoom-out` does.
    Zoom(i32),
    /// Starts a camera pan with the fingers' motion so far.
    PanBegin(f64, f64),
    /// The fingers' motion, for the camera pan.
    Pan(f64, f64),
}

/// A touchpad pinch the camera zooms with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pinch {
    /// The camera's zoom when the pinch began.
    start: i32,
}

thread_local! {
    /// When Super went down with no other key or button since.
    static SUPER_DOWN: Cell<Option<u32>> = const { Cell::new(None) };
    /// When the mod key went down, while it is held.
    static SUPER_HELD: Cell<Option<u32>> = const { Cell::new(None) };
    static SUPER_PAN: Cell<Option<SuperPan>> = const { Cell::new(None) };
    static SWIPE: Cell<Option<Swipe>> = const { Cell::new(None) };
    static PINCH: Cell<Option<Pinch>> = const { Cell::new(None) };
}

/// Whether a touchpad pinch with `fingers` zooms the camera: three or more fingers do, and any
/// pinch with the mod key held does. A plain two-finger pinch belongs to the app under the
/// pointer, as browsers and image viewers zoom with it.
fn pinch_zooms(fingers: u32, with_mod: bool) -> bool {
    fingers >= 3 || with_mod
}

/// The zoom a pinch that began at zoom `start` reaches when the fingers are `scale` times as far
/// apart as when it began: `start` plus the whole zoom steps nearest to `scale`, within
/// `zoom-min` and `zoom-max`. The camera draws at `zoom-step` to the power of the zoom, so
/// pinching in (`scale` below 1) pulls the camera back and spreading the fingers brings it
/// closer, and the camera's scale follows the fingers' to the nearest step. A `zoom-step` of 1
/// draws every zoom at 1:1, and the pinch keeps `start`.
fn pinch_zoom(start: i32, scale: f64, zoom: CanvasZoom) -> i32 {
    let steps = (scale.ln() / zoom.step.ln()).round();
    if !steps.is_finite() {
        return start;
    }
    (f64::from(start) + steps).clamp(f64::from(zoom.min), f64::from(zoom.max)) as i32
}

/// A touchpad swipe with `fingers` began, with the mod key held or not: the canvas swipe it is,
/// if any. One with the mod key is no Super tap.
fn swipe_begin(fingers: u32, with_mod: bool) -> Option<Swipe> {
    let action = SwipeAction::new(fingers, with_mod)?;
    if with_mod {
        State::canvas_cancel_super_tap();
    }
    Some(Swipe {
        action,
        state: SwipeState::Pending(0., 0.),
    })
}

/// One update of a canvas swipe by the fingers' motion `(dx, dy)`: the swipe after it and what
/// it does. The focus or the window goes where the fingers go, whatever the scroll direction
/// setting: the deltas are the fingers' own motion. Once its motion passes [`SWIPE_THRESHOLD`]
/// a swipe with the mod key and three fingers pans the camera with all of that motion and every
/// motion after it. Any other swipe takes the axis it went furthest along, with all of that
/// motion. A vertical four-finger swipe without the mod key then zooms the camera, one step for
/// each [`ZOOM_STEP_MOTION`] the fingers go, up pulling it back. Otherwise each `step` of motion
/// along the axis (`step.0` across, `step.1` down, at least the action's least step) is one
/// step, and what is left carries over; a focus swipe is done after its first step, so one swipe
/// moves the focus one window.
fn swipe_update(swipe: Swipe, dx: f64, dy: f64, step: (f64, f64)) -> (Swipe, SwipeEffect) {
    let with = |state| Swipe { state, ..swipe };
    let (horizontal, mut rest) = match swipe.state {
        SwipeState::Ignored => return (swipe, SwipeEffect::Steps(Vec::new())),
        SwipeState::Panning => return (swipe, SwipeEffect::Pan(dx, dy)),
        SwipeState::Zooming { rest } => return zoom_steps(swipe, rest + dy),
        SwipeState::Pending(x, y) => {
            let (x, y) = (x + dx, y + dy);
            if x.hypot(y) < SWIPE_THRESHOLD {
                return (
                    with(SwipeState::Pending(x, y)),
                    SwipeEffect::Steps(Vec::new()),
                );
            }
            if swipe.action == SwipeAction::Pan {
                return (with(SwipeState::Panning), SwipeEffect::PanBegin(x, y));
            }
            let horizontal = x.abs() >= y.abs();
            if !horizontal && swipe.action == SwipeAction::FocusOrZoom {
                return zoom_steps(swipe, y);
            }
            (horizontal, if horizontal { x } else { y })
        }
        SwipeState::Stepping { horizontal, rest } => {
            (horizontal, rest + if horizontal { dx } else { dy })
        }
    };
    let (step, back, forth) = if horizontal {
        (step.0, Dir::Left, Dir::Right)
    } else {
        (step.1, Dir::Up, Dir::Down)
    };
    let step = step.max(swipe.action.step_min());
    let mut steps = Vec::new();
    while rest.abs() >= step {
        steps.push(if rest > 0. { forth } else { back });
        rest -= step.copysign(rest);
    }
    let once = matches!(swipe.action, SwipeAction::Focus | SwipeAction::FocusOrZoom);
    if once && !steps.is_empty() {
        steps.truncate(1);
        return (with(SwipeState::Ignored), SwipeEffect::Steps(steps));
    }
    (
        with(SwipeState::Stepping { horizontal, rest }),
        SwipeEffect::Steps(steps),
    )
}

/// A zooming swipe whose vertical motion short of a step is `rest`: the zoom steps in it, one
/// for each [`ZOOM_STEP_MOTION`], fingers going up (negative) pulling the camera back, and the
/// swipe with what is left.
fn zoom_steps(swipe: Swipe, rest: f64) -> (Swipe, SwipeEffect) {
    let steps = (rest / ZOOM_STEP_MOTION).trunc();
    let swipe = Swipe {
        state: SwipeState::Zooming {
            rest: rest - steps * ZOOM_STEP_MOTION,
        },
        ..swipe
    };
    (swipe, SwipeEffect::Zoom(-steps as i32))
}

/// The motion a mod key pan takes from one pointer event: the unaccelerated delta plus `share`
/// of what the pointer's acceleration adds to it (0: the hand's motion, 1: the cursor's).
fn pan_delta(
    accel: Point<f64, Logical>,
    unaccel: Point<f64, Logical>,
    share: f64,
) -> Point<f64, Logical> {
    unaccel + (accel - unaccel).upscale(share)
}

/// Whether the mod key, down since `held_since` (key event milliseconds, `None` when its press
/// went unseen), has been held [`PAN_DELAY_MS`] at `now`: motion before then pans nothing.
fn pan_settled(held_since: Option<u32>, now: u32) -> bool {
    held_since.is_none_or(|down| now.wrapping_sub(down) >= PAN_DELAY_MS)
}

/// One pointer motion by `(dx, dy)` with the mod key held: the pan it makes, if any, and
/// whether the pan starts with it. A pan starts once the motion since the mod key settled
/// passes [`PAN_THRESHOLD`], with all of that motion, and cancels the mod key tap. Motion before
/// the mod key has been held [`PAN_DELAY_MS`] (`settled` false, see [`pan_settled`]) counts for
/// nothing; a pan under way goes on whatever `settled` says.
fn super_pan_motion(dx: f64, dy: f64, settled: bool) -> Option<(bool, f64, f64)> {
    let (x, y) = match SUPER_PAN.get() {
        Some(SuperPan::Panning) => return Some((false, dx, dy)),
        _ if !settled => {
            SUPER_PAN.set(None);
            return None;
        }
        Some(SuperPan::Pending(x, y)) => (x + dx, y + dy),
        None => (dx, dy),
    };
    if x.hypot(y) < PAN_THRESHOLD {
        SUPER_PAN.set(Some(SuperPan::Pending(x, y)));
        return None;
    }
    SUPER_PAN.set(Some(SuperPan::Panning));
    State::canvas_cancel_super_tap();
    Some((true, x, y))
}

/// Whether a key whose first-level keysyms are `syms` is the mod key: Super by default, Alt
/// when nested, or whichever `mod-key` the config names.
fn is_mod_key(mod_key: ModKey, syms: &[Keysym]) -> bool {
    let keys: &[Keysym] = match mod_key {
        ModKey::Super => &[Keysym::Super_L, Keysym::Super_R],
        ModKey::Alt => &[Keysym::Alt_L, Keysym::Alt_R, Keysym::Meta_L, Keysym::Meta_R],
        ModKey::Ctrl => &[Keysym::Control_L, Keysym::Control_R],
        ModKey::Shift => &[Keysym::Shift_L, Keysym::Shift_R],
        ModKey::IsoLevel3Shift => &[Keysym::ISO_Level3_Shift],
        ModKey::IsoLevel5Shift => &[Keysym::ISO_Level5_Shift],
    };
    syms.iter().any(|sym| keys.contains(sym))
}

/// Whether `held` has no modifier besides the mod key's own: a mod key press then starts a tap.
fn mod_alone(held: Modifiers, mod_key: ModKey) -> bool {
    (held - mod_key.to_modifiers()).is_empty()
}

impl State {
    /// A tap of the mod key alone zooms the camera out to `canvas { super-tap-zoom }` from 1:1 and
    /// back to 1:1 from any other zoom; without it, it toggles the overview
    /// (`canvas { super-tap-overview }`). The
    /// mod key pressed while another modifier is held starts no tap. Its release also ends a
    /// mod key pan and the camera's pull-back to the whole canvas.
    pub(super) fn canvas_super_tap(&mut self, code: Keycode, pressed: bool, time: u32) {
        let mod_key = self.backend.mod_key(&self.niri.config.borrow());
        let keyboard = self.niri.seat.get_keyboard().unwrap();
        let syms = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            // SAFETY: the state is only read, and dropped before the lock.
            let state = unsafe { xkb.state() };
            let layout = state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
            state
                .get_keymap()
                .key_get_syms_by_level(code, layout, 0)
                .to_vec()
        });
        let is_mod = is_mod_key(mod_key, &syms);
        let alone = mod_alone(modifiers_from_state(keyboard.modifier_state()), mod_key);
        if is_mod {
            let down = if pressed {
                Some(SUPER_HELD.get().unwrap_or(time))
            } else {
                None
            };
            SUPER_HELD.set(down);
        }
        let start = SUPER_DOWN
            .with(|down| down.replace(is_mod.then_some(time).filter(|_| pressed && alone)));
        if !pressed && is_mod {
            self.canvas_end_super_pan();
            self.canvas_mod_released();
        }
        let tapped = !pressed
            && is_mod
            && start.is_some_and(|start| time.wrapping_sub(start) <= SUPER_TAP_MS);
        if !tapped {
            return;
        }
        let (tap_zoom, tap_overview) = {
            let config = self.niri.config.borrow();
            let canvas = &config.layout.canvas;
            (canvas.super_tap_zoom, canvas.super_tap_overview)
        };
        if let Some(scale) = tap_zoom {
            if let Some(state) = self.niri.layout.canvas_zoom_state() {
                if state.zoom == 0 {
                    self.niri.layout.canvas_zoom(pinch_zoom(0, scale, state));
                } else {
                    self.niri.layout.canvas_zoom_reset();
                }
                self.niri.queue_redraw_all();
            }
        } else if tap_overview {
            self.niri.layout.toggle_overview();
            self.niri.queue_redraw_all();
        }
    }

    /// Esc during a drag that stayed on its canvas cancels it: the others glide back, the window
    /// goes home and nothing drops; during a resize drag, the window keeps its size. The key
    /// reaches no app, its release included.
    pub(super) fn canvas_cancel_drag(
        &mut self,
        code: Keycode,
        pressed: bool,
        serial: Serial,
        time: InputTime,
    ) -> bool {
        if !pressed
            || code.raw() != 9
            || !(self.niri.layout.canvas_cancel_move() || self.niri.layout.canvas_cancel_resize())
        {
            return false;
        }
        self.niri.suppressed_keys.insert(code);
        let pointer = self.niri.seat.get_pointer().unwrap();
        if pointer.is_grabbed() {
            pointer.unset_grab(self, serial, time);
        }
        self.niri.queue_redraw_all();
        true
    }

    pub(super) fn canvas_cancel_super_tap() {
        SUPER_DOWN.with(|down| down.set(None));
    }

    /// Pointer motion with the mod key held, no button and no grab pans the canvas camera
    /// 1:1, as if the content were held, once past [`PAN_THRESHOLD`] of motion after the first
    /// [`PAN_DELAY_MS`]: by the motion [`pan_delta`] takes from the accelerated and the
    /// unaccelerated pointer delta. Returns whether this motion panned: the pointer then
    /// focuses no window.
    pub(super) fn canvas_super_pan(
        &mut self,
        accel: Point<f64, Logical>,
        unaccel: Point<f64, Logical>,
        time: Duration,
    ) -> bool {
        let share = self.niri.config.borrow().layout.canvas.pan_acceleration;
        let delta = pan_delta(accel, unaccel, share);
        if !self.mod_held()
            || self.niri.seat.get_pointer().unwrap().is_grabbed()
            || self.niri.is_locked()
            || self.niri.layout.is_overview_open()
        {
            self.canvas_end_super_pan();
            return false;
        }
        let settled = pan_settled(SUPER_HELD.get(), time.as_millis() as u32);
        let Some((start, dx, dy)) = super_pan_motion(delta.x, delta.y, settled) else {
            return false;
        };
        if start {
            let Some(output) = self.niri.output_under_cursor() else {
                SUPER_PAN.set(None);
                return false;
            };
            self.niri
                .layout
                .view_offset_gesture_begin(&output, None, false);
        }
        match self.niri.layout.canvas_pan_update(-dx, -dy, time, false) {
            Some(output) => {
                if let Some(output) = output {
                    self.niri.queue_redraw(&output);
                }
                true
            }
            None => {
                SUPER_PAN.set(None);
                false
            }
        }
    }

    /// Ends a mod key pan, settling the view the way a touchpad pan settles: the window
    /// nearest the middle of the screen takes the focus and is shown. Returns whether a pan
    /// ended.
    pub(super) fn canvas_end_super_pan(&mut self) -> bool {
        if SUPER_PAN.replace(None) != Some(SuperPan::Panning) {
            return false;
        }
        self.niri.layout.view_offset_gesture_end(Some(false));
        self.niri.queue_redraw_all();
        true
    }

    fn mod_held(&self) -> bool {
        let mod_key = self.backend.mod_key(&self.niri.config.borrow());
        let mods = self.niri.seat.get_keyboard().unwrap().modifier_state();
        modifiers_from_state(mods).contains(mod_key.to_modifiers())
    }

    /// A touchpad swipe began. Outside the overview, three and four fingers move the focus or
    /// zoom the camera, or with the mod key held pan the camera or move the focused window (see
    /// [`SwipeAction`]); a four-finger swipe with the mod key first pulls the camera back to
    /// show the whole canvas, until the mod key is released. Returns whether this swipe is the
    /// canvas's.
    pub(super) fn canvas_swipe_begin(&mut self, fingers: u32) -> bool {
        let with_mod = self.mod_held();
        if !with_mod {
            self.canvas_mod_released();
        }
        let swipe = swipe_begin(fingers, with_mod).filter(|_| !self.niri.layout.is_overview_open());
        SWIPE.set(swipe);
        let Some(swipe) = swipe else {
            return false;
        };
        if swipe.action == SwipeAction::Move && !self.niri.is_locked() {
            self.niri.layout.canvas_zoom_to_fit();
            self.niri.queue_redraw_all();
        }
        true
    }

    /// The fingers' motion `(dx, dy)` in a touchpad swipe (see [`swipe_update`]). A step is the
    /// motion a pan takes to scroll one tile, so the window moved with the mod key follows the
    /// fingers across the zoomed-out canvas. Nothing steps in the overview or while locked, and a
    /// swipe begun with the mod key does nothing more once the key is up. Returns whether the
    /// motion was a canvas swipe's.
    pub(super) fn canvas_swipe_update(&mut self, dx: f64, dy: f64, time: Duration) -> bool {
        let Some(swipe) = SWIPE.get() else {
            return false;
        };
        if swipe.action.with_mod() && !self.mod_held() {
            self.canvas_mod_released();
            return true;
        }
        let step = self
            .niri
            .layout
            .canvas_touchpad_tile_motion()
            .unwrap_or_default();
        let (swipe, effect) = swipe_update(swipe, dx, dy, step);
        SWIPE.set(Some(swipe));
        if self.niri.is_locked() {
            return true;
        }
        match effect {
            SwipeEffect::Zoom(0) => {}
            SwipeEffect::Zoom(delta) => {
                self.niri.layout.canvas_zoom(delta);
                self.niri.queue_redraw_all();
            }
            SwipeEffect::PanBegin(dx, dy) => {
                if let Some(output) = self.niri.output_under_cursor() {
                    self.niri
                        .layout
                        .view_offset_gesture_begin(&output, None, true);
                    self.canvas_swipe_pan(dx, dy, time);
                }
            }
            SwipeEffect::Pan(dx, dy) => self.canvas_swipe_pan(dx, dy, time),
            SwipeEffect::Steps(steps) => self.canvas_swipe_steps(swipe.action, &steps),
        }
        true
    }

    /// The camera goes after the fingers of a panning swipe, as if they held the canvas.
    fn canvas_swipe_pan(&mut self, dx: f64, dy: f64, time: Duration) {
        if let Some(Some(output)) = self.niri.layout.canvas_pan_update(-dx, -dy, time, true) {
            self.niri.queue_redraw(&output);
        }
    }

    /// Ends the camera pan of a panning swipe, with the touchpad's momentum.
    fn canvas_swipe_pan_end(&mut self, swipe: Swipe) {
        if swipe.state == SwipeState::Panning
            && self
                .niri
                .layout
                .view_offset_gesture_end(Some(true))
                .is_some()
        {
            self.niri.queue_redraw_all();
        }
    }

    /// Steps of a canvas swipe, each down the path its key binding takes; none when there is no
    /// focused tiled window.
    fn canvas_swipe_steps(&mut self, action: SwipeAction, steps: &[Dir]) {
        if steps.is_empty() || self.niri.layout.is_overview_open() {
            return;
        }
        let layout = &mut self.niri.layout;
        let mut stepped = false;
        for &dir in steps {
            stepped |= match action {
                SwipeAction::Focus | SwipeAction::FocusOrZoom => layout.canvas_focus(dir),
                SwipeAction::Pan => false,
                SwipeAction::Move => layout.canvas_move_focus(dir),
            };
        }
        if !stepped {
            return;
        }
        match action {
            SwipeAction::Focus | SwipeAction::FocusOrZoom => {
                self.niri.layer_shell_on_demand_focus = None;
            }
            SwipeAction::Move => self.niri.layout.canvas_zoom_to_fit(),
            SwipeAction::Pan => {}
        }
        self.maybe_warp_cursor_to_focus();
        self.niri.queue_redraw_all();
    }

    /// A touchpad swipe ended. Returns whether it was a canvas swipe.
    pub(super) fn canvas_swipe_end(&mut self) -> bool {
        let Some(swipe) = SWIPE.replace(None) else {
            return false;
        };
        self.canvas_swipe_pan_end(swipe);
        true
    }

    /// A touchpad pinch with `fingers` began. Outside the overview, one that zooms (see
    /// [`pinch_zooms`]) is the canvas's: no app receives it, and one with the mod key held is no
    /// Super tap. Returns whether this pinch is the canvas's.
    pub(super) fn canvas_pinch_begin(&mut self, fingers: u32) -> bool {
        let with_mod = self.mod_held();
        let zooms = pinch_zooms(fingers, with_mod) && !self.niri.layout.is_overview_open();
        let start = self
            .niri
            .layout
            .canvas_zoom_state()
            .map_or(0, |zoom| zoom.zoom);
        let pinch = zooms.then_some(Pinch { start });
        PINCH.set(pinch);
        if pinch.is_some() && with_mod {
            State::canvas_cancel_super_tap();
        }
        pinch.is_some()
    }

    /// The fingers of a touchpad pinch are `scale` times as far apart as when it began. A pinch
    /// the canvas took moves the camera to the zoom [`pinch_zoom`] gives, by the steps `zoom-in`
    /// and `zoom-out` take, so it zooms about the middle of the screen with the camera's
    /// animation. Nothing zooms while locked. Returns whether the pinch is the canvas's.
    pub(super) fn canvas_pinch_update(&mut self, scale: f64) -> bool {
        let Some(pinch) = PINCH.get() else {
            return false;
        };
        if self.niri.is_locked() {
            return true;
        }
        let Some(zoom) = self.niri.layout.canvas_zoom_state() else {
            return true;
        };
        let target = pinch_zoom(pinch.start, scale, zoom);
        if target != zoom.zoom {
            self.niri.layout.canvas_zoom(target - zoom.zoom);
            self.maybe_warp_cursor_to_focus();
            self.niri.queue_redraw_all();
        }
        true
    }

    /// A touchpad pinch ended. The camera keeps the zoom a canvas pinch reached. Returns whether
    /// the pinch is the canvas's.
    pub(super) fn canvas_pinch_end(&mut self) -> bool {
        PINCH.replace(None).is_some()
    }

    /// The mod key went up: a swipe begun with it does nothing more, and a camera a four-finger
    /// swipe with it pulled back returns to its zoom, showing the focused window.
    fn canvas_mod_released(&mut self) {
        if let Some(swipe) = SWIPE.get().filter(|swipe| swipe.action.with_mod()) {
            self.canvas_swipe_pan_end(swipe);
            SWIPE.set(Some(Swipe {
                state: SwipeState::Ignored,
                ..swipe
            }));
        }
        if self.niri.layout.canvas_zoom_back() {
            self.maybe_warp_cursor_to_focus();
            self.niri.queue_redraw_all();
        }
    }

    pub(super) fn do_canvas_action(&mut self, action: Action) {
        match action {
            Action::ResizeEdge(edge, delta) => {
                let edge = match edge {
                    CanvasEdge::Left => Edge::Left,
                    CanvasEdge::Right => Edge::Right,
                    CanvasEdge::Top => Edge::Top,
                    CanvasEdge::Bottom => Edge::Bottom,
                };
                self.niri.layout.canvas_resize_edge(edge, delta);
            }
            Action::ZoomIn => self.niri.layout.canvas_zoom(-1),
            Action::ZoomOut => self.niri.layout.canvas_zoom(1),
            Action::ZoomReset => self.niri.layout.canvas_zoom_reset(),
            Action::Undo => {
                self.niri.layout.canvas_undo();
            }
            Action::Redo => {
                self.niri.layout.canvas_redo();
            }
            Action::GatherIslands => {
                self.niri.layout.canvas_gather_islands();
            }
            Action::SendToEdge(side) => {
                let dir = match side {
                    CanvasSide::Left => Dir::Left,
                    CanvasSide::Right => Dir::Right,
                };
                self.niri.layout.canvas_send_to_edge(dir);
            }
            Action::SwapNeighbour(direction) => {
                let dir = match direction {
                    CanvasDirection::Left => Dir::Left,
                    CanvasDirection::Right => Dir::Right,
                    CanvasDirection::Up => Dir::Up,
                    CanvasDirection::Down => Dir::Down,
                };
                self.niri.layout.canvas_swap_neighbour(dir);
            }
            Action::LaunchPlaceholder { app_id, icon } => {
                let size = placeholder_size(&self.niri.config.borrow().window_rules, &app_id);
                self.niri
                    .layout
                    .canvas_launch_placeholder(app_id, icon, size);
            }
            Action::QuitApp => {
                self.quit_focused_app();
                return;
            }
            Action::CycleFloatingFollow => self.niri.layout.cycle_floating_follow(),
            Action::SetFloatingFollow(mode) => self.niri.layout.set_floating_follow(mode),
            _ => return,
        }
        self.maybe_warp_cursor_to_focus();
        self.niri.queue_redraw_all();
    }

    /// Closes every window with the focused window's PID and app ID.
    fn quit_focused_app(&mut self) {
        let key = |mapped: &Mapped| {
            let pid = mapped.credentials().map(|c| c.pid);
            let app_id = with_toplevel_role(mapped.toplevel(), |role| role.app_id.clone());
            (pid, app_id)
        };
        let Some(focus) = self.niri.layout.focus() else {
            return;
        };
        let target = key(focus);
        if target.0.is_none() {
            focus.toplevel().send_close();
            return;
        }
        for (_, mapped) in self.niri.layout.windows() {
            if key(mapped) == target {
                mapped.toplevel().send_close();
            }
        }
    }
}

/// Whether `action` only switches to, moves something to, reorders or names a workspace: all of
/// them do nothing with `canvas { one-workspace }`. The `...OrWorkspace...` actions still act on
/// windows; the layout keeps their workspace half from going anywhere.
pub(super) fn is_workspace_action(action: &Action) -> bool {
    matches!(
        action,
        Action::FocusWorkspace(_)
            | Action::FocusWorkspaceDown
            | Action::FocusWorkspaceDownUnderMouse
            | Action::FocusWorkspaceUp
            | Action::FocusWorkspaceUpUnderMouse
            | Action::FocusWorkspacePrevious
            | Action::MoveWindowToWorkspace(..)
            | Action::MoveWindowToWorkspaceById { .. }
            | Action::MoveWindowToWorkspaceDown(_)
            | Action::MoveWindowToWorkspaceUp(_)
            | Action::MoveColumnToWorkspace(..)
            | Action::MoveColumnToWorkspaceDown(_)
            | Action::MoveColumnToWorkspaceUp(_)
            | Action::MoveWorkspaceDown
            | Action::MoveWorkspaceUp
            | Action::MoveWorkspaceToIndex(_)
            | Action::MoveWorkspaceToIndexByRef { .. }
            | Action::MoveWorkspaceToMonitor(_)
            | Action::MoveWorkspaceToMonitorByRef { .. }
            | Action::MoveWorkspaceToMonitorDown
            | Action::MoveWorkspaceToMonitorLeft
            | Action::MoveWorkspaceToMonitorNext
            | Action::MoveWorkspaceToMonitorPrevious
            | Action::MoveWorkspaceToMonitorRight
            | Action::MoveWorkspaceToMonitorUp
            | Action::SetWorkspaceName(_)
            | Action::SetWorkspaceNameByRef { .. }
            | Action::UnsetWorkspaceName
            | Action::UnsetWorkSpaceNameByRef(_)
    )
}

/// The default width and height the window rules give a window with this app ID, from the rules
/// a placeholder can tell apply: those whose match tests nothing but the app ID. An axis no rule
/// sizes takes the canvas's default size.
fn placeholder_size(rules: &[WindowRule], app_id: &str) -> RuleSize {
    let matches = |m: &Match| {
        m.app_id.as_ref().is_some_and(|re| re.0.is_match(app_id))
            && Match {
                app_id: None,
                at_startup: None,
                ..m.clone()
            } == Match::default()
    };
    let (mut width, mut height) = (None, None);
    for rule in rules {
        if (rule.matches.is_empty() || rule.matches.iter().any(matches))
            && !rule.excludes.iter().any(matches)
        {
            if let Some(x) = rule.default_column_width {
                width = Some(x.0);
            }
            if let Some(x) = rule.default_window_height {
                height = Some(x.0);
            }
        }
    }
    (width, height)
}

#[cfg(test)]
mod tests {
    use newm_canvas::Dir;
    use niri_config::{Config, ModKey, Modifiers, PresetSize};
    use smithay::input::keyboard::{xkb, Keysym};
    use smithay::utils::Point;

    use super::{
        is_mod_key, mod_alone, pan_delta, pan_settled, pinch_zoom, pinch_zooms, placeholder_size,
        super_pan_motion, swipe_begin, swipe_update, CanvasZoom, SuperPan, SwipeAction,
        SwipeEffect, SwipeState, PAN_DELAY_MS, SUPER_DOWN, SUPER_PAN,
    };

    #[test]
    fn the_configured_mod_key_taps_and_ends_pans() {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap =
            xkb::Keymap::new_from_names(&context, "", "", "us", "", None, xkb::COMPILE_NO_FLAGS)
                .unwrap();
        let syms = |code: u32| keymap.key_get_syms_by_level(code.into(), 0, 0).to_vec();
        // xkb codes: 133/134 Super, 64/108 Alt, 37 Ctrl, 38 the A key.
        for code in [133, 134] {
            assert!(is_mod_key(ModKey::Super, &syms(code)));
            assert!(!is_mod_key(ModKey::Alt, &syms(code)));
        }
        assert!(is_mod_key(ModKey::Alt, &syms(64)));
        assert!(!is_mod_key(ModKey::Super, &syms(64)));
        assert!(is_mod_key(ModKey::Ctrl, &syms(37)));
        assert!(!is_mod_key(ModKey::Super, &syms(38)));
        assert!(!is_mod_key(ModKey::Alt, &Vec::<Keysym>::new()));

        assert!(mod_alone(Modifiers::empty(), ModKey::Alt));
        assert!(mod_alone(Modifiers::ALT, ModKey::Alt));
        assert!(!mod_alone(Modifiers::SUPER, ModKey::Alt));
        assert!(!mod_alone(Modifiers::ALT, ModKey::Super));
        assert!(!mod_alone(
            Modifiers::CTRL | Modifiers::SUPER,
            ModKey::Super
        ));
    }

    #[test]
    fn a_pan_takes_its_share_of_the_acceleration() {
        let (accel, unaccel) = (Point::from((30., -12.)), Point::from((10., -4.)));
        assert_eq!(pan_delta(accel, unaccel, 1.), accel);
        assert_eq!(pan_delta(accel, unaccel, 0.), unaccel);
        assert_eq!(pan_delta(accel, unaccel, 0.25), Point::from((15., -6.)));
    }

    #[test]
    fn super_motion_pans_past_the_threshold_and_cancels_the_tap() {
        SUPER_PAN.set(None);
        SUPER_DOWN.set(Some(1));
        assert_eq!(super_pan_motion(3., 4., true), None, "5 px is jitter");
        assert_eq!(super_pan_motion(-2., 1., true), None);
        assert_eq!(SUPER_DOWN.get(), Some(1), "jitter keeps the tap");
        assert_eq!(
            super_pan_motion(6., 0., true),
            Some((true, 7., 5.)),
            "the pan starts with all the motion so far"
        );
        assert_eq!(SUPER_DOWN.get(), None, "a pan is no tap");
        assert_eq!(
            super_pan_motion(-1., 2., true),
            Some((false, -1., 2.)),
            "then 1:1"
        );
        assert_eq!(SUPER_PAN.get(), Some(SuperPan::Panning));
        SUPER_PAN.set(None);
    }

    #[test]
    fn motion_in_the_mod_keys_first_moments_counts_for_nothing() {
        SUPER_PAN.set(None);
        SUPER_DOWN.set(Some(1));
        assert_eq!(super_pan_motion(30., 0., false), None, "the hand settling");
        assert_eq!(SUPER_PAN.get(), None, "and none of it is kept");
        assert_eq!(SUPER_DOWN.get(), Some(1), "nor does it cancel the tap");
        assert_eq!(
            super_pan_motion(5., 0., true),
            None,
            "the threshold counts from here"
        );
        assert_eq!(super_pan_motion(4., 0., true), Some((true, 9., 0.)));
        assert_eq!(
            super_pan_motion(1., 1., false),
            Some((false, 1., 1.)),
            "a pan under way goes on"
        );
        SUPER_PAN.set(None);
    }

    #[test]
    fn the_mod_key_settles_after_its_first_100_ms() {
        assert_eq!(PAN_DELAY_MS, 100);
        assert!(!pan_settled(Some(1000), 1000), "the press itself");
        assert!(!pan_settled(Some(1000), 1099), "99 ms in");
        assert!(pan_settled(Some(1000), 1100), "100 ms in");
        assert!(pan_settled(Some(1000), 5000));
        assert!(pan_settled(None, 0), "a press the compositor never saw");
        assert!(
            !pan_settled(Some(u32::MAX - 20), 30),
            "51 ms across the clock's wrap"
        );
        assert!(pan_settled(Some(u32::MAX - 20), 80));
    }

    #[test]
    fn placeholder_size_follows_app_id_rules() {
        let config = Config::parse_mem(
            r#"
            window-rule {
                match app-id="ghostty"
                default-column-width { fixed 1680; }
                default-window-height { fixed 1085; }
            }
            window-rule {
                match app-id="chromium" title="Settings"
                default-column-width { proportion 0.5; }
            }
            window-rule {
                match app-id="chromium"
                exclude app-id="chromium-dev"
                default-window-height { proportion 0.25; }
            }
            "#,
        )
        .unwrap();
        let size = |app_id| placeholder_size(&config.window_rules, app_id);
        assert_eq!(
            size("com.mitchellh.ghostty"),
            (
                Some(Some(PresetSize::Fixed(1680))),
                Some(Some(PresetSize::Fixed(1085)))
            )
        );
        assert_eq!(
            size("chromium"),
            (None, Some(Some(PresetSize::Proportion(0.25)))),
            "a placeholder has no title"
        );
        assert_eq!(size("chromium-dev"), (None, None));
    }

    /// What a swipe does over the fingers' motion: its steps, its zoom steps and its camera pan.
    #[derive(Debug, Default, PartialEq)]
    struct Swiped {
        steps: Vec<Dir>,
        zoom: Vec<i32>,
        pan: Option<Vec<(f64, f64)>>,
    }

    /// What a swipe with `fingers`, the mod key held or not, does over the fingers' `motion`, one
    /// event at a time.
    fn run_swipe(fingers: u32, with_mod: bool, motion: &[(f64, f64)], step: (f64, f64)) -> Swiped {
        let mut swipe = swipe_begin(fingers, with_mod).unwrap();
        let mut swiped = Swiped::default();
        for &(dx, dy) in motion {
            let (next, effect) = swipe_update(swipe, dx, dy, step);
            swipe = next;
            match effect {
                SwipeEffect::Steps(made) => swiped.steps.extend(made),
                SwipeEffect::Zoom(0) => {}
                SwipeEffect::Zoom(delta) => swiped.zoom.push(delta),
                SwipeEffect::PanBegin(dx, dy) => {
                    assert!(swiped.pan.is_none(), "the pan begins once");
                    swiped.pan = Some(vec![(dx, dy)]);
                }
                SwipeEffect::Pan(dx, dy) => swiped.pan.as_mut().unwrap().push((dx, dy)),
            }
        }
        swiped
    }

    fn steps(fingers: u32, with_mod: bool, motion: &[(f64, f64)], step: (f64, f64)) -> Vec<Dir> {
        let swiped = run_swipe(fingers, with_mod, motion, step);
        assert_eq!((&swiped.zoom, &swiped.pan), (&Vec::new(), &None));
        swiped.steps
    }

    #[test]
    fn swipe_actions_by_fingers_and_mod_key() {
        let action = |fingers, with_mod| swipe_begin(fingers, with_mod).map(|s| s.action);
        assert_eq!(action(3, false), Some(SwipeAction::Focus));
        assert_eq!(action(4, false), Some(SwipeAction::FocusOrZoom));
        assert_eq!(action(3, true), Some(SwipeAction::Pan));
        assert_eq!(action(4, true), Some(SwipeAction::Move));
        assert_eq!(action(2, false), None);
        assert_eq!(action(5, true), None);
        assert_eq!(
            swipe_begin(3, false).unwrap().state,
            SwipeState::Pending(0., 0.)
        );
    }

    #[test]
    fn swipe_with_the_mod_key_cancels_the_super_tap() {
        SUPER_DOWN.set(Some(1));
        swipe_begin(4, false);
        swipe_begin(3, false);
        assert_eq!(SUPER_DOWN.get(), Some(1), "no mod key, no tap to cancel");
        swipe_begin(4, true);
        assert_eq!(SUPER_DOWN.get(), None, "a swipe with Super is no tap");
        SUPER_DOWN.set(Some(1));
        swipe_begin(3, true);
        assert_eq!(SUPER_DOWN.get(), None);
    }

    #[test]
    fn swipe_below_the_threshold_does_nothing() {
        let swipe = swipe_begin(3, false).unwrap();
        let (swipe, effect) = swipe_update(swipe, 10., 10., (1., 1.));
        assert_eq!(swipe.state, SwipeState::Pending(10., 10.));
        assert_eq!(effect, SwipeEffect::Steps(Vec::new()));
        for (fingers, with_mod) in [(3, false), (4, false), (3, true), (4, true)] {
            assert_eq!(
                run_swipe(fingers, with_mod, &[(10., 0.), (-4., 5.)], (1., 1.)),
                Swiped::default()
            );
        }
    }

    #[test]
    fn three_fingers_move_the_focus_once_on_either_axis() {
        let right = steps(3, false, &[(20., 0.); 30], (150., 150.));
        assert_eq!(right, [Dir::Right], "600 of motion is still one window");
        let down = steps(3, false, &[(0., 25.); 20], (120., 125.));
        assert_eq!(down, [Dir::Down]);
        let up = steps(3, false, &[(0., -20.); 30], (100., 100.));
        assert_eq!(up, [Dir::Up]);
        let back = steps(
            3,
            false,
            &[(20., 0.), (200., 0.), (-400., 0.)],
            (100., 100.),
        );
        assert_eq!(
            back,
            [Dir::Right],
            "going back in the same swipe moves nothing"
        );
        let fast = steps(3, false, &[(450., 0.)], (100., 100.));
        assert_eq!(fast, [Dir::Right], "one fast event is one window too");
        let short = steps(3, false, &[(20., 0.); 4], (35., 35.));
        assert!(short.is_empty(), "a step is never below STEP_MIN, 100");
    }

    #[test]
    fn swipe_follows_the_dominant_axis() {
        for (fingers, with_mod, n) in [(3, false, 1), (4, true, 6)] {
            let moves = steps(fingers, with_mod, &[(-20., 12.); 30], (100., 100.));
            assert_eq!(moves, vec![Dir::Left; n]);
            let moves = steps(fingers, with_mod, &[(5., -30.); 20], (100., 100.));
            assert_eq!(moves, vec![Dir::Up; n]);
        }
    }

    #[test]
    fn four_fingers_sideways_move_the_focus_once() {
        let swiped = run_swipe(4, false, &[(-20., 12.); 30], (100., 100.));
        assert_eq!(swiped.steps, [Dir::Left]);
        assert!(swiped.zoom.is_empty());
        let swiped = run_swipe(4, false, &[(20., 0.), (0., 400.)], (100., 100.));
        assert_eq!(swiped, Swiped::default(), "a sideways swipe stays sideways");
    }

    #[test]
    fn four_fingers_up_pull_the_camera_back_and_down_bring_it_closer() {
        let swiped = run_swipe(4, false, &[(3., -10.), (2., -10.), (40., -85.)], (1., 1.));
        assert!(swiped.steps.is_empty());
        assert_eq!(
            swiped.zoom,
            [1],
            "105 up is one step back, whatever the tile"
        );
        let swiped = run_swipe(4, false, &[(0., -50.); 9], (1., 1.));
        assert_eq!(swiped.zoom, [1; 4], "450 up: a step for each 100");
        let swiped = run_swipe(4, false, &[(0., 30.), (100., 280.)], (1., 1.));
        assert_eq!(
            swiped.zoom,
            [-3],
            "310 down in one event is three steps closer"
        );
        let swiped = run_swipe(4, false, &[(0., -120.), (0., 240.)], (1., 1.));
        assert_eq!(swiped.zoom, [1, -2], "going back zooms back, and on");
    }

    #[test]
    fn mod_key_and_three_fingers_pan_the_camera() {
        let swiped = run_swipe(
            3,
            true,
            &[(6., 8.), (9., 12.), (-5., 30.), (0., -2.)],
            (1., 1.),
        );
        assert!(swiped.steps.is_empty() && swiped.zoom.is_empty());
        assert_eq!(
            swiped.pan,
            Some(vec![(15., 20.), (-5., 30.), (0., -2.)]),
            "it begins with all the motion so far, then takes every delta on both axes"
        );
    }

    #[test]
    fn mod_key_and_four_fingers_move_a_tile_a_step_after_the_fingers() {
        let moves = steps(4, true, &[(20., 0.); 30], (60., 40.));
        assert_eq!(moves, [Dir::Right; 10], "600 of motion at 60 a step");
        let moves = steps(4, true, &[(0., -20.); 30], (60., 40.));
        assert_eq!(
            moves,
            [Dir::Up; 12],
            "600 of motion at 40 a step, floored at ZOOMED_STEP_MIN, 50"
        );
        let moves = steps(4, true, &[(0., 20.); 30], (60., 160.));
        assert_eq!(
            moves,
            [Dir::Down; 3],
            "the step follows the zoomed-out tile"
        );
    }

    #[test]
    fn released_swipe_does_nothing_more() {
        for fingers in [3, 4] {
            let mut swipe = swipe_begin(fingers, true).unwrap();
            swipe.state = SwipeState::Ignored;
            let (next, effect) = swipe_update(swipe, 500., 0., (100., 100.));
            assert_eq!(next, swipe);
            assert_eq!(effect, SwipeEffect::Steps(Vec::new()));
        }
    }

    /// The defaults: `zoom-step 0.8`, `zoom-min -2`, `zoom-max 4`, at zoom 0.
    const ZOOM: CanvasZoom = CanvasZoom {
        zoom: 0,
        step: 0.8,
        min: -2,
        max: 4,
    };

    #[test]
    fn three_fingers_or_the_mod_key_pinch_the_camera() {
        assert!(
            !pinch_zooms(2, false),
            "a plain two-finger pinch is the app's"
        );
        assert!(pinch_zooms(3, false));
        assert!(pinch_zooms(4, false));
        assert!(pinch_zooms(5, false));
        for fingers in 2..=5 {
            assert!(
                pinch_zooms(fingers, true),
                "the mod key and {fingers} fingers"
            );
        }
    }

    #[test]
    fn pinching_in_pulls_the_camera_back_and_spreading_brings_it_closer() {
        assert_eq!(pinch_zoom(0, 1.0, ZOOM), 0);
        assert_eq!(pinch_zoom(0, 0.8, ZOOM), 1);
        assert_eq!(pinch_zoom(0, 0.6, ZOOM), 2, "0.6 is 2.3 steps of 0.8");
        assert_eq!(pinch_zoom(0, 1.25, ZOOM), -1);
        assert_eq!(pinch_zoom(1, 1.6, ZOOM), -1, "1.6 is 2.1 steps closer");
    }

    #[test]
    fn a_pinch_follows_the_fingers_in_whole_steps() {
        // Half a step either way rounds to the nearest whole step.
        assert_eq!(pinch_zoom(0, 0.8f64.powf(0.49), ZOOM), 0);
        assert_eq!(pinch_zoom(0, 0.8f64.powf(0.51), ZOOM), 1);
        assert_eq!(pinch_zoom(0, 0.8f64.powf(-0.51), ZOOM), -1);
    }

    #[test]
    fn a_pinch_stays_within_zoom_min_and_zoom_max() {
        assert_eq!(pinch_zoom(0, 0.01, ZOOM), 4);
        assert_eq!(pinch_zoom(3, 0.5, ZOOM), 4);
        assert_eq!(pinch_zoom(0, 100.0, ZOOM), -2);
        assert_eq!(pinch_zoom(-1, 2.0, ZOOM), -2);
    }

    #[test]
    fn a_pinch_back_to_where_the_fingers_began_returns_to_its_zoom() {
        for start in -2..=4 {
            for steps in -6..=6 {
                let scale = ZOOM.step.powi(steps);
                assert_eq!(pinch_zoom(start, scale, ZOOM), (start + steps).clamp(-2, 4));
                assert_eq!(pinch_zoom(start, 1.0, ZOOM), start);
            }
        }
    }

    #[test]
    fn a_pinch_with_zoom_step_one_keeps_the_zoom() {
        let flat = CanvasZoom { step: 1.0, ..ZOOM };
        assert_eq!(pinch_zoom(1, 0.5, flat), 1);
        assert_eq!(pinch_zoom(1, 1.0, flat), 1);
        assert_eq!(pinch_zoom(1, 2.0, flat), 1);
    }
}
