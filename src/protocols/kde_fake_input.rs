//! `org_kde_kwin_fake_input`: KDE's remote-input protocol.
//!
//! xdg-desktop-portal-kde drives its RemoteDesktop sessions through it (KDE Connect's remote
//! mouse and keyboard, krfb). Its requests arrive here as [`FakeInputRequest`]s; the compositor
//! turns them into input events of [`FakeInputBackend`], which go through the same path as a
//! real device's (bindings, focus, idle, cursor), as under KWin.
//!
//! Requests count only after `authenticate`, as in KWin. Who may bind the global at all is the
//! filter's call (see `niri::fake_input_allowed`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use smithay::backend::input::{
    self, Axis, AxisRelativeDirection, AxisSource, ButtonState, InputBackend, InputTime, KeyState,
    Keycode, TouchSlot, UnusedEvent,
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::{Dispatch2, GlobalDispatch2};

use super::raw::kde_fake_input::v6::server::org_kde_kwin_fake_input::{self, OrgKdeKwinFakeInput};

const VERSION: u32 = 6;

/// The global, and what each fake-input device holds pressed.
pub struct KdeFakeInputState {
    pub held: HashMap<FakeInputDevice, Held>,
}

/// The keys, buttons and touch points one fake-input device holds down, so that its end (the
/// resource's destruction or its client's disconnection) releases them all instead of leaving
/// them stuck.
#[derive(Debug, Default)]
pub struct Held {
    /// Evdev key codes, in press order.
    keys: Vec<u32>,
    /// The evdev code and Shift that each held keysym pressed, so that its release lifts the same
    /// key even if the layout changes in between.
    keysyms: HashMap<u32, (u32, bool)>,
    buttons: Vec<u32>,
    touches: Vec<u32>,
}

/// What a device's end releases: keys in reverse press order, then buttons, then touch points.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Release {
    pub keys: Vec<u32>,
    pub buttons: Vec<u32>,
    pub touches: Vec<u32>,
}

fn hold(list: &mut Vec<u32>, value: u32, down: bool) {
    list.retain(|v| *v != value);
    if down {
        list.push(value);
    }
}

impl Held {
    pub fn key(&mut self, code: u32, pressed: bool) {
        hold(&mut self.keys, code, pressed);
    }

    pub fn keysym_pressed(&mut self, keysym: u32, code: u32, shift: bool) {
        self.keysyms.insert(keysym, (code, shift));
    }

    /// The key and Shift that `keysym`'s press used, if it is held.
    pub fn keysym_released(&mut self, keysym: u32) -> Option<(u32, bool)> {
        self.keysyms.remove(&keysym)
    }

    pub fn button(&mut self, button: u32, pressed: bool) {
        hold(&mut self.buttons, button, pressed);
    }

    pub fn touch(&mut self, id: u32, down: bool) {
        hold(&mut self.touches, id, down);
    }

    pub fn touch_cancel(&mut self) {
        self.touches.clear();
    }

    pub fn release(self) -> Release {
        let mut keys = self.keys;
        keys.reverse();
        Release {
            keys,
            buttons: self.buttons,
            touches: self.touches,
        }
    }
}

pub struct KdeFakeInputGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

/// Per-resource state: whether the client sent `authenticate`.
#[derive(Debug, Default)]
pub struct KdeFakeInputData {
    authenticated: AtomicBool,
}

/// A request of an authenticated client. Positions are in global logical coordinates; key codes
/// are Linux evdev codes; `Keysym` names an XKB keysym the compositor finds a key for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FakeInputRequest {
    Motion { dx: f64, dy: f64 },
    MotionAbsolute { x: f64, y: f64 },
    Button { button: u32, pressed: bool },
    Axis { axis: Axis, value: f64 },
    Key { code: u32, pressed: bool },
    Keysym { keysym: u32, pressed: bool },
    TouchDown { id: u32, x: f64, y: f64 },
    TouchMotion { id: u32, x: f64, y: f64 },
    TouchUp { id: u32 },
    TouchCancel,
    TouchFrame,
}

pub trait KdeFakeInputHandler {
    fn fake_input(&mut self, device: FakeInputDevice, request: FakeInputRequest);
    /// The device's resource is gone (destroyed, or its client disconnected).
    fn fake_input_destroyed(&mut self, device: FakeInputDevice);
}

impl KdeFakeInputState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<OrgKdeKwinFakeInput, KdeFakeInputGlobalData>,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = KdeFakeInputGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, OrgKdeKwinFakeInput, _>(VERSION, global_data);
        Self {
            held: HashMap::new(),
        }
    }
}

impl<D> GlobalDispatch2<OrgKdeKwinFakeInput, D> for KdeFakeInputGlobalData
where
    D: Dispatch<OrgKdeKwinFakeInput, KdeFakeInputData>,
    D: 'static,
{
    fn bind(
        &self,
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<OrgKdeKwinFakeInput>,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(resource, KdeFakeInputData::default());
    }

    fn can_view(&self, client: &Client) -> bool {
        (self.filter)(client)
    }
}

impl<D> Dispatch2<OrgKdeKwinFakeInput, D> for KdeFakeInputData
where
    D: KdeFakeInputHandler,
    D: 'static,
{
    fn request(
        &self,
        state: &mut D,
        _client: &Client,
        resource: &OrgKdeKwinFakeInput,
        request: org_kde_kwin_fake_input::Request,
        _display: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        use org_kde_kwin_fake_input::Request;

        let request = match request {
            Request::Authenticate {
                application,
                reason,
            } => {
                info!("fake input: {application:?} authenticated ({reason:?})");
                self.authenticated.store(true, Ordering::Relaxed);
                return;
            }
            Request::Destroy => return,
            _ if !self.authenticated.load(Ordering::Relaxed) => {
                trace!("fake input: request before authenticate, ignored");
                return;
            }
            Request::PointerMotion { delta_x, delta_y } => FakeInputRequest::Motion {
                dx: delta_x,
                dy: delta_y,
            },
            Request::PointerMotionAbsolute { x, y } => FakeInputRequest::MotionAbsolute { x, y },
            Request::Button { button, state } => FakeInputRequest::Button {
                button,
                pressed: match pressed(state) {
                    Some(pressed) => pressed,
                    None => return,
                },
            },
            Request::Axis { axis, value } => FakeInputRequest::Axis {
                axis: match axis {
                    0 => Axis::Vertical,
                    1 => Axis::Horizontal,
                    _ => {
                        trace!("fake input: axis {axis} out of range, ignored");
                        return;
                    }
                },
                value,
            },
            Request::KeyboardKey { button, state } => FakeInputRequest::Key {
                code: button,
                pressed: match pressed(state) {
                    Some(pressed) => pressed,
                    None => return,
                },
            },
            Request::KeyboardKeysym { keysym, state } => FakeInputRequest::Keysym {
                keysym,
                pressed: match pressed(state) {
                    Some(pressed) => pressed,
                    None => return,
                },
            },
            Request::TouchDown { id, x, y } => FakeInputRequest::TouchDown { id, x, y },
            Request::TouchMotion { id, x, y } => FakeInputRequest::TouchMotion { id, x, y },
            Request::TouchUp { id } => FakeInputRequest::TouchUp { id },
            Request::TouchCancel => FakeInputRequest::TouchCancel,
            Request::TouchFrame => FakeInputRequest::TouchFrame,
        };

        let device = FakeInputDevice {
            resource: resource.clone(),
        };
        state.fake_input(device, request);
    }

    fn destroyed(&self, state: &mut D, _client: ClientId, resource: &OrgKdeKwinFakeInput) {
        state.fake_input_destroyed(FakeInputDevice {
            resource: resource.clone(),
        });
    }
}

/// A button or key state (0 released, 1 pressed); any other value is out of range.
fn pressed(state: u32) -> Option<bool> {
    match state {
        0 => Some(false),
        1 => Some(true),
        _ => {
            trace!("fake input: state {state} out of range, ignored");
            None
        }
    }
}

/// Backend type of fake-input events; never constructed, only its event types are used.
#[derive(Debug)]
pub struct FakeInputBackend;

impl InputBackend for FakeInputBackend {
    type Device = FakeInputDevice;

    type KeyboardKeyEvent = FakeKeyEvent;
    type PointerAxisEvent = FakeAxisEvent;
    type PointerButtonEvent = FakeButtonEvent;
    type PointerMotionEvent = FakeMotionEvent;
    type PointerMotionAbsoluteEvent = FakeAbsoluteEvent;

    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = UnusedEvent;
    type GesturePinchUpdateEvent = UnusedEvent;
    type GesturePinchEndEvent = UnusedEvent;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;

    type TouchDownEvent = FakeTouchEvent;
    type TouchUpEvent = FakeTouchEvent;
    type TouchMotionEvent = FakeTouchEvent;
    type TouchCancelEvent = FakeTouchEvent;
    type TouchFrameEvent = FakeTouchEvent;

    type TabletToolAxisEvent = UnusedEvent;
    type TabletToolProximityEvent = UnusedEvent;
    type TabletToolTipEvent = UnusedEvent;
    type TabletToolButtonEvent = UnusedEvent;

    type SwitchToggleEvent = UnusedEvent;

    type SpecialEvent = UnusedEvent;
}

/// The device behind one `org_kde_kwin_fake_input` resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FakeInputDevice {
    resource: OrgKdeKwinFakeInput,
}

impl input::Device for FakeInputDevice {
    fn id(&self) -> String {
        format!("kde-fake-input-{}", self.resource.id())
    }

    fn name(&self) -> String {
        String::from("KDE fake input")
    }

    fn has_capability(&self, capability: input::DeviceCapability) -> bool {
        matches!(
            capability,
            input::DeviceCapability::Pointer
                | input::DeviceCapability::Keyboard
                | input::DeviceCapability::Touch
        )
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<PathBuf> {
        None
    }
}

macro_rules! fake_event {
    ($ty:ty) => {
        impl input::Event<FakeInputBackend> for $ty {
            fn time(&self) -> InputTime {
                InputTime::from_millis(self.time)
            }

            fn device(&self) -> FakeInputDevice {
                self.device.clone()
            }
        }
    };
}

#[derive(Debug)]
pub struct FakeMotionEvent {
    pub device: FakeInputDevice,
    pub time: u32,
    pub dx: f64,
    pub dy: f64,
}
fake_event!(FakeMotionEvent);

impl input::PointerMotionEvent<FakeInputBackend> for FakeMotionEvent {
    fn delta_x(&self) -> f64 {
        self.dx
    }

    fn delta_y(&self) -> f64 {
        self.dy
    }

    fn delta_x_unaccel(&self) -> f64 {
        self.dx
    }

    fn delta_y_unaccel(&self) -> f64 {
        self.dy
    }
}

/// An absolute position, as fractions (0..=1) of the area the compositor mapped it from: the
/// whole output layout for the pointer, the touch output for touch.
#[derive(Debug)]
pub struct FakeAbsoluteEvent {
    pub device: FakeInputDevice,
    pub time: u32,
    pub x: f64,
    pub y: f64,
}
fake_event!(FakeAbsoluteEvent);

impl input::AbsolutePositionEvent<FakeInputBackend> for FakeAbsoluteEvent {
    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn x_transformed(&self, width: i32) -> f64 {
        self.x.clamp(0., 1.) * f64::from(width)
    }

    fn y_transformed(&self, height: i32) -> f64 {
        self.y.clamp(0., 1.) * f64::from(height)
    }
}

impl input::PointerMotionAbsoluteEvent<FakeInputBackend> for FakeAbsoluteEvent {}

#[derive(Debug)]
pub struct FakeButtonEvent {
    pub device: FakeInputDevice,
    pub time: u32,
    pub button: u32,
    pub state: ButtonState,
}
fake_event!(FakeButtonEvent);

impl input::PointerButtonEvent<FakeInputBackend> for FakeButtonEvent {
    fn button_code(&self) -> u32 {
        self.button
    }

    fn state(&self) -> ButtonState {
        self.state
    }
}

#[derive(Debug)]
pub struct FakeAxisEvent {
    pub device: FakeInputDevice,
    pub time: u32,
    pub axis: Axis,
    pub value: f64,
}
fake_event!(FakeAxisEvent);

impl input::PointerAxisEvent<FakeInputBackend> for FakeAxisEvent {
    fn amount(&self, axis: Axis) -> Option<f64> {
        (axis == self.axis).then_some(self.value)
    }

    fn amount_v120(&self, _axis: Axis) -> Option<f64> {
        None
    }

    fn source(&self) -> AxisSource {
        AxisSource::Wheel
    }

    fn relative_direction(&self, _axis: Axis) -> AxisRelativeDirection {
        AxisRelativeDirection::Identical
    }
}

#[derive(Debug)]
pub struct FakeKeyEvent {
    pub device: FakeInputDevice,
    pub time: u32,
    /// The Linux evdev code.
    pub code: u32,
    pub state: KeyState,
}
fake_event!(FakeKeyEvent);

impl input::KeyboardKeyEvent<FakeInputBackend> for FakeKeyEvent {
    fn key_code(&self) -> Keycode {
        self.code.saturating_add(8).into()
    }

    fn state(&self) -> KeyState {
        self.state
    }

    fn count(&self) -> u32 {
        u32::from(self.state == KeyState::Pressed)
    }
}

/// One touch event; `x`/`y` are fractions of the touch output, as in [`FakeAbsoluteEvent`].
#[derive(Debug)]
pub struct FakeTouchEvent {
    pub device: FakeInputDevice,
    pub time: u32,
    pub id: Option<u32>,
    pub x: f64,
    pub y: f64,
}
fake_event!(FakeTouchEvent);

impl input::TouchEvent<FakeInputBackend> for FakeTouchEvent {
    fn slot(&self) -> TouchSlot {
        TouchSlot::from(self.id)
    }
}

impl input::AbsolutePositionEvent<FakeInputBackend> for FakeTouchEvent {
    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn x_transformed(&self, width: i32) -> f64 {
        self.x.clamp(0., 1.) * f64::from(width)
    }

    fn y_transformed(&self, height: i32) -> f64 {
        self.y.clamp(0., 1.) * f64::from(height)
    }
}

impl input::TouchDownEvent<FakeInputBackend> for FakeTouchEvent {}
impl input::TouchMotionEvent<FakeInputBackend> for FakeTouchEvent {}
impl input::TouchUpEvent<FakeInputBackend> for FakeTouchEvent {}
impl input::TouchCancelEvent<FakeInputBackend> for FakeTouchEvent {}
impl input::TouchFrameEvent<FakeInputBackend> for FakeTouchEvent {}

/// The owner and mode bits of a file or directory.
#[derive(Debug, Clone, Copy)]
pub struct Owner {
    pub uid: u32,
    pub mode: u32,
}

impl Owner {
    /// Owned by root, and no one else may write to it, except to a sticky directory (such as
    /// `/nix/store`), where only an entry's owner may rename or delete it.
    fn trusted(self, is_dir: bool) -> bool {
        const STICKY: u32 = 0o1000;
        self.uid == 0 && (self.mode & 0o022 == 0 || (is_dir && self.mode & STICKY != 0))
    }
}

/// Whether an executable may bind `org_kde_kwin_fake_input`: xdg-desktop-portal-kde only, which
/// asks the user before any RemoteDesktop session starts (KWin likewise grants the interface
/// only to the executables whose desktop file lists it).
///
/// `exe` is the resolved path of the running executable (`/proc/<pid>/exe`) and `exe_owner` that
/// file's owner. Its file name must name the portal (NixOS's wrappers run the binary as
/// `.xdg-desktop-portal-kde-wrapped`), and neither the file nor any directory above it may be
/// writable by anyone but root, so a user cannot put a binary of that name in place:
/// `dir_owner` reads each directory's owner.
pub fn executable_may_fake_input(
    exe: &Path,
    exe_owner: Option<Owner>,
    dir_owner: impl Fn(&Path) -> Option<Owner>,
) -> bool {
    const ALLOWED: &[&str] = &["xdg-desktop-portal-kde"];
    let Some(path) = exe.to_str() else {
        return false;
    };
    let exe = Path::new(path.strip_suffix(" (deleted)").unwrap_or(path));
    if !exe.is_absolute() {
        return false;
    }
    let Some(name) = exe.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let name = name.strip_prefix('.').unwrap_or(name);
    let name = name.strip_suffix("-wrapped").unwrap_or(name);
    if !ALLOWED.contains(&name) {
        return false;
    }
    exe_owner.is_some_and(|owner| owner.trusted(false))
        && exe
            .ancestors()
            .skip(1)
            .all(|dir| dir_owner(dir).is_some_and(|owner| owner.trusted(true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT_DIR: Owner = Owner {
        uid: 0,
        mode: 0o40755,
    };
    const ROOT_EXE: Owner = Owner {
        uid: 0,
        mode: 0o100555,
    };

    /// Directory owners as on a NixOS system with a user's home and a world-writable /tmp.
    fn dirs(dir: &Path) -> Option<Owner> {
        let owner = |uid, mode| Some(Owner { uid, mode });
        match dir.to_str()? {
            "/nix/store" => owner(0, 0o41775),
            "/tmp" => owner(0, 0o41777),
            "/srv/shared" => owner(0, 0o40775),
            d if d.starts_with("/home/user") || d.starts_with("/tmp/") => owner(1000, 0o40755),
            _ => Some(ROOT_DIR),
        }
    }

    fn may(exe: &str) -> bool {
        executable_may_fake_input(Path::new(exe), Some(ROOT_EXE), dirs)
    }

    #[test]
    fn only_the_kde_portal_may_fake_input() {
        for ok in [
            "/nix/store/x-xdg-desktop-portal-kde-6.8.3/libexec/.xdg-desktop-portal-kde-wrapped",
            "/usr/libexec/xdg-desktop-portal-kde",
            "/usr/libexec/xdg-desktop-portal-kde (deleted)",
        ] {
            assert!(may(ok), "{ok}");
        }
        for no in [
            "/usr/bin/kdeconnectd",
            "/usr/bin/xdg-desktop-portal-kde-helper",
            "/tmp/.wrapped",
            "xdg-desktop-portal-kde",
            "/",
        ] {
            assert!(!may(no), "{no}");
        }
    }

    #[test]
    fn a_portal_name_in_a_user_writable_place_may_not_fake_input() {
        for no in [
            "/home/user/bin/xdg-desktop-portal-kde",
            "/home/user/.local/libexec/.xdg-desktop-portal-kde-wrapped",
            "/tmp/x/xdg-desktop-portal-kde",
            "/srv/shared/xdg-desktop-portal-kde",
        ] {
            // The file itself is root's, but a directory above it is not safe.
            assert!(!may(no), "{no}");
        }
        let exe = Path::new("/usr/libexec/xdg-desktop-portal-kde");
        let users = Owner {
            uid: 1000,
            mode: 0o100755,
        };
        let writable = Owner {
            uid: 0,
            mode: 0o100777,
        };
        assert!(!executable_may_fake_input(exe, Some(users), dirs));
        assert!(!executable_may_fake_input(exe, Some(writable), dirs));
        assert!(!executable_may_fake_input(exe, None, dirs));
        assert!(!executable_may_fake_input(exe, Some(ROOT_EXE), |_| None));
    }

    #[test]
    fn a_device_end_releases_what_it_holds() {
        let mut held = Held::default();
        held.key(42, true);
        held.key(30, true);
        held.keysym_pressed(0x41, 30, true);
        held.key(31, true);
        held.key(31, false);
        held.button(0x110, true);
        held.button(0x111, true);
        held.button(0x111, false);
        held.touch(3, true);
        held.touch(4, true);
        held.touch(3, false);
        assert_eq!(held.keysym_released(0x41), Some((30, true)));
        assert_eq!(held.keysym_released(0x41), None);
        assert_eq!(
            held.release(),
            Release {
                keys: vec![30, 42],
                buttons: vec![0x110],
                touches: vec![4],
            }
        );

        let mut held = Held::default();
        held.touch(1, true);
        held.touch_cancel();
        held.key(30, true);
        held.key(30, true);
        assert_eq!(
            held.release(),
            Release {
                keys: vec![30],
                ..Release::default()
            }
        );
    }
}
