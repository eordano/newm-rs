//! Pointer motion with the mod key held pans the canvas camera, but not in the mod key's first
//! 100 ms: the hand settling as it presses the mod key moves the camera nowhere, and a mod+right
//! press (resize) or mod+left press (move) in that time lands on the window under the pointer.

use niri_config::Config;
use smithay::backend::input::{InputEvent, InputTime, KeyState, Keycode};
use smithay::input::pointer::PointerHandle;
use smithay::utils::{Logical, Point};
use wayland_client::protocol::wl_pointer;

use crate::input::move_grab::MoveGrab;
use crate::input::resize_grab::ResizeGrab;
use crate::niri::State;
use crate::tests::client::ClientId;
use crate::tests::fixture::Fixture;
use crate::tests::test_input_backend::{TestInputBackend, TestKeyboardKeyEvent};

/// Super_L, the headless backend's mod key.
const SUPER: u32 = 133;
/// Alt_L.
const ALT: u32 = 64;
const LEFT: u32 = 0x110;
const RIGHT: u32 = 0x111;
/// When the mod key goes down, in milliseconds.
const T0: u32 = 1000;

fn set_up(config: &str) -> (Fixture, ClientId) {
    let mut f = Fixture::with_config(Config::parse_mem(config).unwrap());
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    // The test client's buffer is one pixel: the viewport gives the window the size the
    // compositor asked for, so that it fills its tile.
    let window = f.client(id).window(&surface);
    let (w, h) = window
        .configures_received
        .last()
        .map(|(_, configure)| configure.size)
        .filter(|&(w, h)| w > 0 && h > 0)
        .unwrap_or((800, 600));
    window.attach_new_buffer();
    window.set_size(w as u16, h as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    f.niri_complete_animations();
    (f, id)
}

/// Puts the pointer on the window, in its bottom-right third so that a resize there has edges
/// to drag, with room for `slack` more pixels of motion down and right.
fn point_at_window_corner(f: &mut Fixture, slack: f64) {
    let output = f.niri_output(1);
    let state = f.niri_state();
    let has_edges = |state: &State, pos: Point<f64, Logical>| {
        state.niri.window_under(pos).is_some()
            && state
                .niri
                .layout
                .resize_edges_under(&output, pos)
                .is_some_and(|edges| !edges.is_empty())
    };
    let pos = (0..1080 / 8)
        .rev()
        .flat_map(|y| (0..1920 / 8).rev().map(move |x| (x * 8, y * 8)))
        .map(|(x, y)| Point::from((f64::from(x), f64::from(y))))
        .find(|&pos| has_edges(state, pos) && has_edges(state, pos + Point::from((slack, slack))))
        .expect("the window has a bottom-right third on the output");
    state.move_cursor(pos);
    assert!(state.niri.window_under_cursor().is_some());
}

fn key(f: &mut Fixture, code: u32, state: KeyState, ms: u32) {
    f.niri_state()
        .process_input_event(InputEvent::<TestInputBackend>::Keyboard {
            event: TestKeyboardKeyEvent {
                time: InputTime::from_millis(ms),
                code: Keycode::new(code),
                state,
                count: 1,
            },
        });
}

fn motion(f: &mut Fixture, id: ClientId, ms: u32, dx: f64, dy: f64) {
    let pointer = f.client(id).state.virtual_pointer();
    pointer.motion(ms, dx, dy);
    pointer.frame();
    f.roundtrip(id);
}

fn button(f: &mut Fixture, id: ClientId, ms: u32, code: u32) {
    let pointer = f.client(id).state.virtual_pointer();
    pointer.button(ms, code, wl_pointer::ButtonState::Pressed);
    pointer.frame();
    f.roundtrip(id);
}

/// Where the camera looks on the active canvas.
fn view(f: &mut Fixture) -> (f64, f64) {
    let ws = f.niri().layout.active_workspace().unwrap();
    ws.scrolling().canvas().view()
}

fn pointer(f: &mut Fixture) -> PointerHandle<State> {
    f.niri().seat.get_pointer().unwrap()
}

fn grab_is<G: 'static>(f: &mut Fixture) -> bool {
    pointer(f)
        .with_grab(|_, grab| grab.as_any().is::<G>())
        .unwrap_or(false)
}

/// The hand settling in the mod key's first 100 ms: 14 px down and right, past the 8 px a pan
/// takes once the mod key has settled.
fn settle(f: &mut Fixture, id: ClientId) {
    motion(f, id, T0 + 20, 10., 0.);
    motion(f, id, T0 + 60, 0., 10.);
}

fn resize_in_first_moments(mod_code: u32, config: &str) {
    let (mut f, id) = set_up(config);
    point_at_window_corner(&mut f, 10.);
    let before = view(&mut f);
    key(&mut f, mod_code, KeyState::Pressed, T0);
    settle(&mut f, id);
    assert_eq!(view(&mut f), before, "the hand settling pans nothing");
    button(&mut f, id, T0 + 90, RIGHT);
    assert!(grab_is::<ResizeGrab>(&mut f), "mod+right resizes");
    assert!(
        f.niri().layout.canvas_cancel_resize(),
        "a canvas resize drag"
    );
    assert_eq!(view(&mut f), before, "the camera stays put");
    key(&mut f, mod_code, KeyState::Released, T0 + 200);
}

#[test]
fn mod_right_in_the_first_100_ms_resizes_and_pans_nothing() {
    resize_in_first_moments(SUPER, "");
}

#[test]
fn mod_left_in_the_first_100_ms_moves_and_pans_nothing() {
    let (mut f, id) = set_up("");
    point_at_window_corner(&mut f, 10.);
    let before = view(&mut f);
    key(&mut f, SUPER, KeyState::Pressed, T0);
    settle(&mut f, id);
    button(&mut f, id, T0 + 90, LEFT);
    assert!(grab_is::<MoveGrab>(&mut f), "mod+left moves");
    assert_eq!(view(&mut f), before, "the camera stays put");
}

#[test]
fn mod_motion_after_100_ms_pans_past_8_px() {
    let (mut f, id) = set_up("");
    point_at_window_corner(&mut f, 10.);
    let before = view(&mut f);
    key(&mut f, SUPER, KeyState::Pressed, T0);
    settle(&mut f, id);
    motion(&mut f, id, T0 + 120, 5., 0.);
    assert_eq!(view(&mut f), before, "5 px after settling is jitter");
    motion(&mut f, id, T0 + 140, 5., 0.);
    let panned = view(&mut f);
    assert_ne!(panned, before, "10 px after settling pans");
    assert!(!pointer(&mut f).is_grabbed(), "a pan holds no grab");
    key(&mut f, SUPER, KeyState::Released, T0 + 200);
}

#[test]
fn a_pan_under_way_goes_on_1_to_1() {
    let (mut f, id) = set_up("");
    point_at_window_corner(&mut f, 10.);
    key(&mut f, SUPER, KeyState::Pressed, T0);
    motion(&mut f, id, T0 + 120, 10., 0.);
    let started = view(&mut f);
    motion(&mut f, id, T0 + 130, 2., 0.);
    let on = view(&mut f);
    assert_ne!(on, started, "2 px pans once the pan is under way");
    motion(&mut f, id, T0 + 140, 2., 0.);
    let now = view(&mut f);
    assert!(
        (now.0 - on.0 - (on.0 - started.0)).abs() < 1e-9 && (now.1 - on.1).abs() < 1e-9,
        "the same motion pans the same: {started:?} {on:?} {now:?}"
    );
    // A second mod key going down mid-pan keeps the first one's time: the pan goes on.
    key(&mut f, SUPER + 1, KeyState::Pressed, T0 + 150);
    motion(&mut f, id, T0 + 160, 2., 0.);
    assert_ne!(view(&mut f), now, "the pan goes on");
    key(&mut f, SUPER + 1, KeyState::Released, T0 + 170);
    key(&mut f, SUPER, KeyState::Released, T0 + 180);
}

const ALT_MOD: &str = r#"input { mod-key "Alt"; }"#;

#[test]
fn alt_as_the_mod_key_resizes_in_its_first_100_ms() {
    resize_in_first_moments(ALT, ALT_MOD);
}

#[test]
fn alt_as_the_mod_key_pans_once_settled() {
    let (mut f, id) = set_up(ALT_MOD);
    point_at_window_corner(&mut f, 10.);
    let before = view(&mut f);
    key(&mut f, ALT, KeyState::Pressed, T0);
    settle(&mut f, id);
    assert_eq!(view(&mut f), before);
    motion(&mut f, id, T0 + 120, 10., 0.);
    assert_ne!(view(&mut f), before, "Alt pans once settled");
    key(&mut f, ALT, KeyState::Released, T0 + 200);
}
