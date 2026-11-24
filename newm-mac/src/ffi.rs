//! The CoreFoundation, CoreGraphics and Accessibility calls newm-mac makes.

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::{c_char, c_void};

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFIndex = isize;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFRunLoopTimerRef = *mut c_void;
pub type CFMachPortRef = *mut c_void;
pub type AXUIElementRef = *const c_void;
pub type AXObserverRef = *mut c_void;
pub type AXValueRef = *const c_void;
pub type AXError = i32;
pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub type CGEventTapCallBack =
    extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;
pub type AXObserverCallback = extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void);
pub type CFRunLoopTimerCallBack = extern "C" fn(CFRunLoopTimerRef, *mut c_void);

pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kCFNumberSInt64Type: CFIndex = 4;
pub const kAXErrorSuccess: AXError = 0;
pub const kAXValueCGPointType: u32 = 1;
pub const kAXValueCGSizeType: u32 = 2;

pub const kCGHIDEventTap: u32 = 0;
pub const kCGSessionEventTap: u32 = 1;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionDefault: u32 = 0;

pub const kCGEventLeftMouseDown: u32 = 1;
pub const kCGEventLeftMouseUp: u32 = 2;
pub const kCGEventRightMouseDown: u32 = 3;
pub const kCGEventRightMouseUp: u32 = 4;
pub const kCGEventMouseMoved: u32 = 5;
pub const kCGEventLeftMouseDragged: u32 = 6;
pub const kCGEventRightMouseDragged: u32 = 7;
pub const kCGEventKeyDown: u32 = 10;
pub const kCGEventKeyUp: u32 = 11;
pub const kCGEventFlagsChanged: u32 = 12;
pub const kCGEventScrollWheel: u32 = 22;
pub const kCGEventTapDisabledByTimeout: u32 = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: u32 = 0xFFFF_FFFF;

pub const kCGKeyboardEventAutorepeat: u32 = 8;
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGScrollWheelEventDeltaAxis1: u32 = 11;
pub const kCGEventSourceUserData: u32 = 42;

pub const kCGEventSourceStatePrivate: i32 = -1;
pub const kCGEventSourceStateHIDSystemState: i32 = 1;
pub const kCGWindowListOptionOnScreenOnly: u32 = 1;
pub const kCGWindowListExcludeDesktopElements: u32 = 16;
pub const kCGScrollEventUnitLine: u32 = 1;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFBooleanFalse: CFTypeRef;

    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFGetTypeID(cf: CFTypeRef) -> usize;
    pub fn CFStringGetTypeID() -> usize;
    pub fn CFBooleanGetTypeID() -> usize;
    pub fn CFNumberGetTypeID() -> usize;
    pub fn CFStringCreateWithBytes(
        alloc: CFAllocatorRef,
        bytes: *const u8,
        num: CFIndex,
        encoding: u32,
        external: u8,
    ) -> CFStringRef;
    pub fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    pub fn CFStringGetMaximumSizeForEncoding(len: CFIndex, encoding: u32) -> CFIndex;
    pub fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, encoding: u32) -> u8;
    pub fn CFArrayGetCount(a: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(a: CFArrayRef, i: CFIndex) -> CFTypeRef;
    pub fn CFDictionaryGetValue(d: CFDictionaryRef, key: *const c_void) -> *const c_void;
    pub fn CFNumberGetValue(n: CFTypeRef, t: CFIndex, out: *mut c_void) -> u8;
    pub fn CFBooleanGetValue(b: CFTypeRef) -> u8;

    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopRun();
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
    pub fn CFRunLoopTimerCreate(
        alloc: CFAllocatorRef,
        fire: f64,
        interval: f64,
        flags: usize,
        order: CFIndex,
        callout: CFRunLoopTimerCallBack,
        context: *mut c_void,
    ) -> CFRunLoopTimerRef;
    pub fn CFRunLoopTimerSetNextFireDate(timer: CFRunLoopTimerRef, fire: f64);
    pub fn CFAbsoluteTimeGetCurrent() -> f64;
    pub fn CFMachPortCreateRunLoopSource(
        alloc: CFAllocatorRef,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub static kCGWindowNumber: CFStringRef;
    pub static kCGWindowOwnerPID: CFStringRef;
    pub static kCGWindowOwnerName: CFStringRef;
    pub static kCGWindowLayer: CFStringRef;

    pub fn AXIsProcessTrusted() -> u8;
    pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        el: AXUIElementRef,
        attr: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> AXError;
    pub fn AXUIElementIsAttributeSettable(el: AXUIElementRef, attr: CFStringRef, out: *mut u8) -> AXError;
    pub fn AXUIElementPerformAction(el: AXUIElementRef, action: CFStringRef) -> AXError;
    pub fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, timeout: f32) -> AXError;
    pub fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut i32) -> AXError;
    pub fn AXValueCreate(t: u32, ptr: *const c_void) -> AXValueRef;
    pub fn AXValueGetValue(v: AXValueRef, t: u32, ptr: *mut c_void) -> u8;
    pub fn AXObserverCreate(pid: i32, cb: AXObserverCallback, out: *mut AXObserverRef) -> AXError;
    pub fn AXObserverAddNotification(
        obs: AXObserverRef,
        el: AXUIElementRef,
        notif: CFStringRef,
        refcon: *mut c_void,
    ) -> AXError;
    pub fn AXObserverGetRunLoopSource(obs: AXObserverRef) -> CFRunLoopSourceRef;
    pub fn _AXUIElementGetWindow(el: AXUIElementRef, id: *mut u32) -> AXError;

    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        cb: CGEventTapCallBack,
        info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetFlags(e: CGEventRef) -> u64;
    pub fn CGEventSetFlags(e: CGEventRef, flags: u64);
    pub fn CGEventGetIntegerValueField(e: CGEventRef, field: u32) -> i64;
    pub fn CGEventSetIntegerValueField(e: CGEventRef, field: u32, value: i64);
    pub fn CGEventGetLocation(e: CGEventRef) -> CGPoint;
    pub fn CGEventCreateKeyboardEvent(src: CGEventSourceRef, key: u16, down: bool) -> CGEventRef;
    pub fn CGEventCreateMouseEvent(src: CGEventSourceRef, t: u32, pos: CGPoint, button: u32) -> CGEventRef;
    pub fn CGEventCreateScrollWheelEvent2(
        src: CGEventSourceRef,
        units: u32,
        count: u32,
        w1: i32,
        w2: i32,
        w3: i32,
    ) -> CGEventRef;
    pub fn CGEventPost(tap: u32, e: CGEventRef);
    pub fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    pub fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> CFArrayRef;
    pub fn CGMainDisplayID() -> u32;
    pub fn CGDisplayBounds(d: u32) -> CGRect;
    pub fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
}

unsafe extern "C" {
    pub fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}
