//! Accessibility and window-list helpers: windows, their frames, and the apps that own them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;

use crate::ffi::*;

thread_local! {
    static NAMES: RefCell<HashMap<&'static str, usize>> = RefCell::new(HashMap::new());
}

/// A CFString for a constant name, created once and kept.
pub fn cfstr(s: &'static str) -> CFStringRef {
    NAMES.with(|n| {
        *n.borrow_mut().entry(s).or_insert_with(|| unsafe {
            CFStringCreateWithBytes(ptr::null(), s.as_ptr(), s.len() as CFIndex, kCFStringEncodingUTF8, 0)
                as usize
        }) as CFStringRef
    })
}

pub fn to_string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    unsafe {
        if CFGetTypeID(s) != CFStringGetTypeID() {
            return String::new();
        }
        let len = CFStringGetMaximumSizeForEncoding(CFStringGetLength(s), kCFStringEncodingUTF8) + 1;
        let mut buf = vec![0u8; len as usize];
        if CFStringGetCString(s, buf.as_mut_ptr() as *mut _, len, kCFStringEncodingUTF8) == 0 {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
}

/// An attribute's value, owned by the caller.
pub fn copy(el: AXUIElementRef, attr: &'static str) -> Option<CFTypeRef> {
    let mut v: CFTypeRef = ptr::null();
    let e = unsafe { AXUIElementCopyAttributeValue(el, cfstr(attr), &mut v) };
    (e == kAXErrorSuccess && !v.is_null()).then_some(v)
}

pub fn release(v: CFTypeRef) {
    if !v.is_null() {
        unsafe { CFRelease(v) }
    }
}

pub fn string(el: AXUIElementRef, attr: &'static str) -> Option<String> {
    let v = copy(el, attr)?;
    let s = to_string(v);
    release(v);
    Some(s)
}

pub fn boolean(el: AXUIElementRef, attr: &'static str) -> Option<bool> {
    let v = copy(el, attr)?;
    let b = unsafe { (CFGetTypeID(v) == CFBooleanGetTypeID()).then(|| CFBooleanGetValue(v) != 0) };
    release(v);
    b
}

pub fn set_bool(el: AXUIElementRef, attr: &'static str, b: bool) -> AXError {
    unsafe {
        AXUIElementSetAttributeValue(el, cfstr(attr), if b { kCFBooleanTrue } else { kCFBooleanFalse })
    }
}

pub fn point(el: AXUIElementRef) -> Option<(f64, f64)> {
    let v = copy(el, "AXPosition")?;
    let mut p = CGPoint::default();
    let ok = unsafe { AXValueGetValue(v, kAXValueCGPointType, &mut p as *mut _ as *mut c_void) };
    release(v);
    (ok != 0).then_some((p.x, p.y))
}

pub fn size(el: AXUIElementRef) -> Option<(f64, f64)> {
    let v = copy(el, "AXSize")?;
    let mut s = CGSize::default();
    let ok = unsafe { AXValueGetValue(v, kAXValueCGSizeType, &mut s as *mut _ as *mut c_void) };
    release(v);
    (ok != 0).then_some((s.width, s.height))
}

pub fn frame(el: AXUIElementRef) -> Option<(f64, f64, f64, f64)> {
    let (x, y) = point(el)?;
    let (w, h) = size(el)?;
    Some((x, y, w, h))
}

pub fn set_point(el: AXUIElementRef, x: f64, y: f64) -> AXError {
    let p = CGPoint { x, y };
    unsafe {
        let v = AXValueCreate(kAXValueCGPointType, &p as *const _ as *const c_void);
        let e = AXUIElementSetAttributeValue(el, cfstr("AXPosition"), v);
        release(v);
        e
    }
}

pub fn set_size(el: AXUIElementRef, w: f64, h: f64) -> AXError {
    let s = CGSize { width: w, height: h };
    unsafe {
        let v = AXValueCreate(kAXValueCGSizeType, &s as *const _ as *const c_void);
        let e = AXUIElementSetAttributeValue(el, cfstr("AXSize"), v);
        release(v);
        e
    }
}

pub fn settable(el: AXUIElementRef, attr: &'static str) -> bool {
    let mut b = 0u8;
    unsafe { AXUIElementIsAttributeSettable(el, cfstr(attr), &mut b) == kAXErrorSuccess && b != 0 }
}

pub fn perform(el: AXUIElementRef, action: &'static str) -> AXError {
    unsafe { AXUIElementPerformAction(el, cfstr(action)) }
}

pub fn window_id(el: AXUIElementRef) -> Option<u32> {
    let mut id = 0u32;
    let e = unsafe { _AXUIElementGetWindow(el, &mut id) };
    (e == kAXErrorSuccess && id != 0).then_some(id)
}

pub fn app_element(pid: i32) -> AXUIElementRef {
    unsafe {
        let a = AXUIElementCreateApplication(pid);
        AXUIElementSetMessagingTimeout(a, 1.0);
        a
    }
}

/// The app's windows, each retained; None when the app does not answer.
pub fn windows(app: AXUIElementRef) -> Option<Vec<AXUIElementRef>> {
    let arr = copy(app, "AXWindows")?;
    let n = unsafe { CFArrayGetCount(arr) };
    let out = (0..n)
        .map(|i| unsafe { CFRetain(CFArrayGetValueAtIndex(arr, i)) as AXUIElementRef })
        .collect();
    release(arr);
    Some(out)
}

/// A window newm-mac tiles: a standard window, not minimized or native-fullscreen, that can be
/// resized. Dialogs, sheets, panels and fixed-size windows float untouched.
pub fn tileable(el: AXUIElementRef) -> bool {
    string(el, "AXSubrole").as_deref() == Some("AXStandardWindow")
        && boolean(el, "AXMinimized") != Some(true)
        && boolean(el, "AXFullScreen") != Some(true)
        && settable(el, "AXSize")
}

pub fn focused_window(app: AXUIElementRef) -> Option<u32> {
    let w = copy(app, "AXFocusedWindow")?;
    let id = window_id(w);
    release(w);
    id
}

/// The pid of the frontmost app: the system-wide focused application, else (it does not answer
/// a process outside the GUI session) the owner of the frontmost normal window.
pub fn frontmost_pid() -> Option<i32> {
    let ax = unsafe {
        let sys = AXUIElementCreateSystemWide();
        let app = copy(sys, "AXFocusedApplication");
        release(sys);
        app.and_then(|app| {
            let mut pid = 0;
            let e = AXUIElementGetPid(app, &mut pid);
            release(app);
            (e == kAXErrorSuccess).then_some(pid)
        })
    };
    let me = std::process::id() as i32;
    ax.or_else(|| cg_windows().into_iter().find(|w| w.layer == 0 && w.pid != me).map(|w| w.pid))
}

pub struct CgWin {
    pub wid: u32,
    pub pid: i32,
    pub layer: i64,
    pub owner: String,
}

fn dict_i64(d: CFDictionaryRef, key: CFStringRef) -> i64 {
    unsafe {
        let v = CFDictionaryGetValue(d, key);
        let mut n = 0i64;
        if !v.is_null() && CFGetTypeID(v) == CFNumberGetTypeID() {
            CFNumberGetValue(v, kCFNumberSInt64Type, &mut n as *mut _ as *mut c_void);
        }
        n
    }
}

/// On-screen windows, front to back.
pub fn cg_windows() -> Vec<CgWin> {
    unsafe {
        let arr = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            0,
        );
        if arr.is_null() {
            return Vec::new();
        }
        let out = (0..CFArrayGetCount(arr))
            .map(|i| {
                let d = CFArrayGetValueAtIndex(arr, i);
                CgWin {
                    wid: dict_i64(d, kCGWindowNumber) as u32,
                    pid: dict_i64(d, kCGWindowOwnerPID) as i32,
                    layer: dict_i64(d, kCGWindowLayer),
                    owner: to_string(CFDictionaryGetValue(d, kCGWindowOwnerName)),
                }
            })
            .collect();
        CFRelease(arr);
        out
    }
}

/// `newm-mac frames`: every tileable window on this Space with its frame, one JSON per line.
pub fn print_frames() -> i32 {
    let me = std::process::id() as i32;
    let cg = cg_windows();
    let mut pids: Vec<(i32, String)> = cg
        .iter()
        .filter(|w| w.layer == 0 && w.pid != me)
        .map(|w| (w.pid, w.owner.clone()))
        .collect();
    pids.sort();
    pids.dedup();
    for (pid, owner) in pids {
        let app = app_element(pid);
        for w in windows(app).unwrap_or_default() {
            if let (Some(id), Some((x, y, ww, hh)), true) = (window_id(w), frame(w), tileable(w)) {
                println!(
                    "{}",
                    serde_json::json!({"app": owner, "pid": pid, "id": id, "x": x, "y": y, "w": ww, "h": hh})
                );
            }
            release(w);
        }
        release(app);
    }
    0
}
