use smithay::backend::input::{
    Device, DeviceCapability, Event, GestureBeginEvent, GestureEndEvent, GesturePinchBeginEvent,
    GesturePinchEndEvent, GesturePinchUpdateEvent, InputBackend, InputTime, KeyState,
    KeyboardKeyEvent, Keycode, UnusedEvent,
};
use smithay::output::Output;

use crate::input::backend_ext::NiriInputDevice;

pub struct TestInputBackend;

#[derive(PartialEq, Eq, Hash)]
pub struct TestInputDevice;

pub struct TestKeyboardKeyEvent {
    pub time: InputTime,
    pub code: Keycode,
    pub state: KeyState,
    pub count: u32,
}

/// A touchpad pinch begins with this many fingers.
pub struct TestPinchBegin {
    pub fingers: u32,
}

/// The fingers of a touchpad pinch are `scale` times as far apart as when it began.
pub struct TestPinchUpdate {
    pub scale: f64,
}

/// A touchpad pinch ends.
pub struct TestPinchEnd;

impl InputBackend for TestInputBackend {
    type Device = TestInputDevice;

    type KeyboardKeyEvent = TestKeyboardKeyEvent;
    type PointerAxisEvent = UnusedEvent;
    type PointerButtonEvent = UnusedEvent;
    type PointerMotionEvent = UnusedEvent;
    type PointerMotionAbsoluteEvent = UnusedEvent;

    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = TestPinchBegin;
    type GesturePinchUpdateEvent = TestPinchUpdate;
    type GesturePinchEndEvent = TestPinchEnd;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;

    type TouchDownEvent = UnusedEvent;
    type TouchUpEvent = UnusedEvent;
    type TouchMotionEvent = UnusedEvent;
    type TouchCancelEvent = UnusedEvent;
    type TouchFrameEvent = UnusedEvent;
    type TabletToolAxisEvent = UnusedEvent;
    type TabletToolProximityEvent = UnusedEvent;
    type TabletToolTipEvent = UnusedEvent;
    type TabletToolButtonEvent = UnusedEvent;

    type SwitchToggleEvent = UnusedEvent;

    type SpecialEvent = UnusedEvent;
}

impl Device for TestInputDevice {
    fn id(&self) -> String {
        String::from("test")
    }

    fn name(&self) -> String {
        String::from("test input device")
    }

    fn has_capability(&self, capability: DeviceCapability) -> bool {
        matches!(capability, DeviceCapability::Keyboard)
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

impl NiriInputDevice for TestInputDevice {
    fn output(&self, _state: &crate::niri::State) -> Option<Output> {
        None
    }
}

impl Event<TestInputBackend> for TestKeyboardKeyEvent {
    fn time(&self) -> InputTime {
        self.time
    }

    fn device(&self) -> <TestInputBackend as InputBackend>::Device {
        TestInputDevice
    }
}

impl KeyboardKeyEvent<TestInputBackend> for TestKeyboardKeyEvent {
    fn key_code(&self) -> Keycode {
        self.code
    }

    fn state(&self) -> KeyState {
        self.state
    }

    fn count(&self) -> u32 {
        self.count
    }
}

macro_rules! test_event {
    ($ty:ty) => {
        impl Event<TestInputBackend> for $ty {
            fn time(&self) -> InputTime {
                InputTime::from_micros(0)
            }

            fn device(&self) -> TestInputDevice {
                TestInputDevice
            }
        }
    };
}

test_event!(TestPinchBegin);
test_event!(TestPinchUpdate);
test_event!(TestPinchEnd);

impl GestureBeginEvent<TestInputBackend> for TestPinchBegin {
    fn fingers(&self) -> u32 {
        self.fingers
    }
}

impl GesturePinchBeginEvent<TestInputBackend> for TestPinchBegin {}

impl GesturePinchUpdateEvent<TestInputBackend> for TestPinchUpdate {
    fn delta_x(&self) -> f64 {
        0.
    }

    fn delta_y(&self) -> f64 {
        0.
    }

    fn scale(&self) -> f64 {
        self.scale
    }

    fn rotation(&self) -> f64 {
        0.
    }
}

impl GestureEndEvent<TestInputBackend> for TestPinchEnd {
    fn cancelled(&self) -> bool {
        false
    }
}

impl GesturePinchEndEvent<TestInputBackend> for TestPinchEnd {}
