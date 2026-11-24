//! Turns `org_kde_kwin_fake_input` requests into input events (see
//! [`crate::protocols::kde_fake_input`]).

use smithay::backend::input::{ButtonState, InputEvent, KeyState};
use smithay::input::keyboard::xkb;
use smithay::utils::{Logical, Point, Rectangle, Transform};

use crate::niri::State;
use crate::protocols::kde_fake_input::{
    FakeAbsoluteEvent, FakeAxisEvent, FakeButtonEvent, FakeInputBackend, FakeInputDevice,
    FakeInputRequest, FakeKeyEvent, FakeMotionEvent, FakeTouchEvent, Held, KdeFakeInputHandler,
};
use crate::utils::get_monotonic_time;

/// Linux evdev code of the left Shift key.
const KEY_LEFTSHIFT: u32 = 42;

type Event = InputEvent<FakeInputBackend>;

impl KdeFakeInputHandler for State {
    fn fake_input(&mut self, device: FakeInputDevice, request: FakeInputRequest) {
        let time = get_monotonic_time().as_millis() as u32;
        match request {
            FakeInputRequest::Key { code, pressed } => self.fake_held(&device).key(code, pressed),
            FakeInputRequest::Button { button, pressed } => {
                self.fake_held(&device).button(button, pressed)
            }
            FakeInputRequest::TouchDown { id, .. } => self.fake_held(&device).touch(id, true),
            FakeInputRequest::TouchUp { id } => self.fake_held(&device).touch(id, false),
            FakeInputRequest::TouchCancel => self.fake_held(&device).touch_cancel(),
            _ => (),
        }

        match request {
            FakeInputRequest::Motion { dx, dy } => self.process_input_event(Event::PointerMotion {
                event: FakeMotionEvent {
                    device,
                    time,
                    dx,
                    dy,
                },
            }),
            FakeInputRequest::MotionAbsolute { x, y } => {
                let Some(geo) = self.global_bounding_rectangle() else {
                    return;
                };
                let (x, y) = fraction(x, y, geo.loc.x, geo.loc.y, geo.size.w, geo.size.h);
                self.process_input_event(Event::PointerMotionAbsolute {
                    event: FakeAbsoluteEvent { device, time, x, y },
                });
            }
            FakeInputRequest::Button { button, pressed } => {
                self.process_input_event(Event::PointerButton {
                    event: FakeButtonEvent {
                        device,
                        time,
                        button,
                        state: if pressed {
                            ButtonState::Pressed
                        } else {
                            ButtonState::Released
                        },
                    },
                })
            }
            FakeInputRequest::Axis { axis, value } => {
                self.process_input_event(Event::PointerAxis {
                    event: FakeAxisEvent {
                        device,
                        time,
                        axis,
                        value,
                    },
                })
            }
            FakeInputRequest::Key { code, pressed } => {
                self.process_input_event(key_event(&device, time, code, pressed))
            }
            FakeInputRequest::Keysym { keysym, pressed } => {
                let resolved = if pressed {
                    self.key_for_keysym(keysym)
                } else {
                    self.fake_held(&device)
                        .keysym_released(keysym)
                        .or_else(|| self.key_for_keysym(keysym))
                };
                let Some((code, shift)) = resolved else {
                    debug!("fake input: no key in the keymap types keysym {keysym:#x}");
                    return;
                };
                let held = self.fake_held(&device);
                if pressed {
                    held.keysym_pressed(keysym, code, shift);
                }
                if shift {
                    held.key(KEY_LEFTSHIFT, pressed);
                }
                held.key(code, pressed);
                if shift && pressed {
                    self.process_input_event(key_event(&device, time, KEY_LEFTSHIFT, true));
                }
                self.process_input_event(key_event(&device, time, code, pressed));
                if shift && !pressed {
                    self.process_input_event(key_event(&device, time, KEY_LEFTSHIFT, false));
                }
            }
            FakeInputRequest::TouchDown { id, x, y }
            | FakeInputRequest::TouchMotion { id, x, y } => {
                let Some(output) = self.niri.output_for_touch() else {
                    return;
                };
                let transform = output.current_transform();
                let Some(geo) = self.niri.global_space.output_geometry(output) else {
                    return;
                };
                let (x, y) = touch_fraction(Point::from((x, y)), geo, transform);
                let event = FakeTouchEvent {
                    device,
                    time,
                    id: Some(id),
                    x,
                    y,
                };
                if matches!(request, FakeInputRequest::TouchDown { .. }) {
                    self.process_input_event(Event::TouchDown { event });
                } else {
                    self.process_input_event(Event::TouchMotion { event });
                }
            }
            FakeInputRequest::TouchUp { id } => self.process_input_event(Event::TouchUp {
                event: touch(device, time, Some(id)),
            }),
            FakeInputRequest::TouchCancel => self.process_input_event(Event::TouchCancel {
                event: touch(device, time, None),
            }),
            FakeInputRequest::TouchFrame => self.process_input_event(Event::TouchFrame {
                event: touch(device, time, None),
            }),
        }
    }

    fn fake_input_destroyed(&mut self, device: FakeInputDevice) {
        let Some(held) = self.niri.kde_fake_input_state.held.remove(&device) else {
            return;
        };
        let release = held.release();
        let time = get_monotonic_time().as_millis() as u32;
        for code in release.keys {
            self.process_input_event(key_event(&device, time, code, false));
        }
        for button in release.buttons {
            self.process_input_event(Event::PointerButton {
                event: FakeButtonEvent {
                    device: device.clone(),
                    time,
                    button,
                    state: ButtonState::Released,
                },
            });
        }
        if !release.touches.is_empty() {
            for id in release.touches {
                self.process_input_event(Event::TouchUp {
                    event: touch(device.clone(), time, Some(id)),
                });
            }
            self.process_input_event(Event::TouchFrame {
                event: touch(device, time, None),
            });
        }
    }
}

fn key_event(device: &FakeInputDevice, time: u32, code: u32, pressed: bool) -> Event {
    Event::Keyboard {
        event: FakeKeyEvent {
            device: device.clone(),
            time,
            code,
            state: if pressed {
                KeyState::Pressed
            } else {
                KeyState::Released
            },
        },
    }
}

impl State {
    fn fake_held(&mut self, device: &FakeInputDevice) -> &mut Held {
        self.niri
            .kde_fake_input_state
            .held
            .entry(device.clone())
            .or_default()
    }

    /// The evdev code of a key that types `keysym` in the effective layout, and whether it takes
    /// Shift (the keysym sits on the key's second level), as KWin resolves fake-input keysyms.
    fn key_for_keysym(&mut self, keysym: u32) -> Option<(u32, bool)> {
        let keyboard = self.niri.seat.get_keyboard()?;
        keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            // SAFETY: the state is only read, and dropped before the lock.
            let state = unsafe { xkb.state() };
            let layout = state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
            key_in_keymap(&state.get_keymap(), layout, xkb::Keysym::new(keysym))
        })
    }
}

/// The first key (lowest code) whose first or second level in `layout` is `keysym`, as an evdev
/// code, with whether it is the second level.
fn key_in_keymap(
    keymap: &xkb::Keymap,
    layout: xkb::LayoutIndex,
    keysym: xkb::Keysym,
) -> Option<(u32, bool)> {
    let mut found = None;
    keymap.key_for_each(|keymap, code| {
        if found.is_some() || code.raw() < 8 {
            return;
        }
        for level in 0..2 {
            if keymap
                .key_get_syms_by_level(code, layout, level)
                .contains(&keysym)
            {
                found = Some((code.raw() - 8, level == 1));
                return;
            }
        }
    });
    found
}

fn touch(device: FakeInputDevice, time: u32, id: Option<u32>) -> FakeTouchEvent {
    FakeTouchEvent {
        device,
        time,
        id,
        x: 0.,
        y: 0.,
    }
}

/// `point`, global logical coordinates on the output at `geo` shown with `transform`, as the
/// fractions of the output's untransformed (panel) area that touch input maps back to `point`:
/// touch events carry panel coordinates, which the input path turns by the output's transform
/// (see `compute_absolute_location`), so these fractions undo that turn first.
fn touch_fraction(
    point: Point<f64, Logical>,
    geo: Rectangle<i32, Logical>,
    transform: Transform,
) -> (f64, f64) {
    let panel = transform.invert().transform_size(geo.size);
    if panel.w <= 0 || panel.h <= 0 {
        return (0., 0.);
    }
    // The input path's map from panel to output coordinates is affine: `origin + x * ex + y *
    // ey`. Solve it for the panel point that lands on `point`.
    let area = panel.to_f64();
    let turn = |x: f64, y: f64| transform.transform_point_in(Point::from((x, y)), &area);
    let origin = turn(0., 0.);
    let (ex, ey) = (turn(1., 0.) - origin, turn(0., 1.) - origin);
    let det = ex.x * ey.y - ey.x * ex.y;
    let p = point - geo.loc.to_f64() - origin;
    let x = (p.x * ey.y - ey.x * p.y) / det;
    let y = (ex.x * p.y - p.x * ex.y) / det;
    ((x / area.w).clamp(0., 1.), (y / area.h).clamp(0., 1.))
}

/// `(x, y)` in the area at `(ax, ay)` of size `(w, h)`, as fractions of it.
fn fraction(x: f64, y: f64, ax: i32, ay: i32, w: i32, h: i32) -> (f64, f64) {
    let f = |v: f64, a: i32, s: i32| {
        if s <= 0 {
            0.
        } else {
            ((v - f64::from(a)) / f64::from(s)).clamp(0., 1.)
        }
    };
    (f(x, ax, w), f(y, ay, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us_keymap() -> xkb::Keymap {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        xkb::Keymap::new_from_names(&context, "", "", "us", "", None, xkb::COMPILE_NO_FLAGS)
            .unwrap()
    }

    #[test]
    fn keysyms_find_their_keys_and_shift() {
        let keymap = us_keymap();
        // KEY_A is 30, KEY_1 is 2, KEY_SPACE is 57.
        assert_eq!(key_in_keymap(&keymap, 0, xkb::Keysym::a), Some((30, false)));
        assert_eq!(key_in_keymap(&keymap, 0, xkb::Keysym::A), Some((30, true)));
        assert_eq!(
            key_in_keymap(&keymap, 0, xkb::Keysym::exclam),
            Some((2, true))
        );
        assert_eq!(
            key_in_keymap(&keymap, 0, xkb::Keysym::space),
            Some((57, false))
        );
        assert_eq!(key_in_keymap(&keymap, 0, xkb::Keysym::Cyrillic_a), None);
    }

    #[test]
    fn touch_lands_where_the_client_points_on_turned_outputs() {
        let geo = Rectangle::new(Point::from((1920, 100)), (1080, 1920).into());
        let point = Point::<f64, Logical>::from((1920. + 270., 100. + 480.));
        for transform in [
            Transform::Normal,
            Transform::_90,
            Transform::_180,
            Transform::_270,
            Transform::Flipped,
            Transform::Flipped90,
            Transform::Flipped180,
            Transform::Flipped270,
        ] {
            let (fx, fy) = touch_fraction(point, geo, transform);
            // What the input path does with a touch event's fractions.
            let panel = transform.invert().transform_size(geo.size);
            let panel_point =
                Point::<f64, Logical>::from((fx * f64::from(panel.w), fy * f64::from(panel.h)));
            let landed =
                transform.transform_point_in(panel_point, &panel.to_f64()) + geo.loc.to_f64();
            assert!(
                (landed.x - point.x).abs() < 1e-9 && (landed.y - point.y).abs() < 1e-9,
                "{transform:?}: {landed:?} != {point:?}"
            );
        }
    }

    #[test]
    fn fractions_clamp_to_the_area() {
        assert_eq!(fraction(960., 540., 0, 0, 1920, 1080), (0.5, 0.5));
        assert_eq!(fraction(-5., 2000., 0, 0, 1920, 1080), (0., 1.));
        assert_eq!(fraction(2020., 0., 1920, 0, 200, 100), (0.5, 0.));
        assert_eq!(fraction(1., 1., 0, 0, 0, 0), (0., 0.));
    }
}
