//! `newm-mac inject`: synthetic input for end-to-end tests, posted at the HID level as a keyboard
//! and mouse would, so newm-mac's tap sees it like the user's own.

use std::thread::sleep;
use std::time::Duration;

use crate::ax;
use crate::ffi::*;
use crate::keys::{self, ALT, CTRL, LEFT_CTRL_DEV, LEFT_OPT, LEFT_SHIFT_DEV, RIGHT_OPT, SHIFT};

fn src() -> CGEventSourceRef {
    unsafe { CGEventSourceCreate(kCGEventSourceStateHIDSystemState) }
}

fn post(e: CGEventRef, flags: u64) {
    unsafe {
        CGEventSetFlags(e, flags);
        CGEventPost(kCGHIDEventTap, e);
        CFRelease(e as CFTypeRef);
    }
}

fn key(code: u16, down: bool, flags: u64, repeat: bool) {
    unsafe {
        let e = CGEventCreateKeyboardEvent(src(), code, down);
        if repeat {
            CGEventSetIntegerValueField(e, kCGKeyboardEventAutorepeat, 1);
        }
        post(e, flags);
    }
    sleep(Duration::from_millis(15));
}

fn mouse(ty: u32, p: (f64, f64), button: u32, flags: u64) {
    unsafe { post(CGEventCreateMouseEvent(src(), ty, CGPoint { x: p.0, y: p.1 }, button), flags) };
}

const OPT: u64 = ALT | LEFT_OPT;

fn opt(down: bool) {
    key(58, down, if down { OPT } else { 0 }, false);
}

/// Flags for a newm combo with `Mod` held.
fn combo_flags(combo: &str) -> Result<(u16, u64), String> {
    let (mods, k) = keys::split(combo);
    let code = keys::keycode(k).ok_or_else(|| format!("no key {k}"))?;
    let mut f = 0;
    for m in mods {
        f |= match m {
            "Mod" => OPT,
            "Shift" => SHIFT | LEFT_SHIFT_DEV,
            "Ctrl" => CTRL | LEFT_CTRL_DEV,
            _ => return Err(format!("{combo}: inject takes Mod, Shift and Ctrl")),
        };
    }
    Ok((code, f))
}

fn num(a: &[String], k: usize) -> f64 {
    a.get(k).and_then(|s| s.parse().ok()).unwrap_or(0.0)
}

fn lerp(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

pub fn main(a: &[String]) -> i32 {
    let what = a.first().map(String::as_str).unwrap_or("");
    match what {
        // inject key Mod+Shift+h
        "key" | "hold" => {
            let Ok((code, f)) = combo_flags(a.get(1).map_or("", String::as_str)) else {
                eprintln!("inject: bad combo");
                return 2;
            };
            let repeats = if what == "hold" { num(a, 2) as usize } else { 0 };
            if f & LEFT_OPT != 0 {
                opt(true);
            }
            key(code, true, f, false);
            for _ in 0..repeats {
                sleep(Duration::from_millis(60));
                key(code, true, f, true);
            }
            key(code, false, f, false);
            if f & LEFT_OPT != 0 {
                opt(false);
            }
        }
        // inject rkey e: right Option + a key, the layout's character
        "rkey" => {
            let code = keys::keycode(a.get(1).map_or("", String::as_str)).unwrap_or(14);
            key(61, true, ALT | RIGHT_OPT, false);
            key(code, true, ALT | RIGHT_OPT, false);
            key(code, false, ALT | RIGHT_OPT, false);
            key(61, false, 0, false);
        }
        // inject plain h: a key with no modifier
        "plain" => {
            let code = keys::keycode(a.get(1).map_or("", String::as_str)).unwrap_or(4);
            key(code, true, 0, false);
            key(code, false, 0, false);
        }
        // inject cmd Cmd+a: a native shortcut, untagged
        "cmd" => match keys::shortcut(a.get(1).map_or("", String::as_str)) {
            Ok((code, f)) => {
                key(55, true, keys::CMD | keys::LEFT_CMD_DEV, false);
                key(code, true, f, false);
                key(code, false, f, false);
                key(55, false, 0, false);
            }
            Err(e) => {
                eprintln!("inject: {e}");
                return 2;
            }
        },
        // inject drag left|right x1 y1 x2 y2 [esc]
        "drag" => {
            let right = a.get(1).map(String::as_str) == Some("right");
            let (p1, p2) = ((num(a, 2), num(a, 3)), (num(a, 4), num(a, 5)));
            let esc = a.get(6).map(String::as_str) == Some("esc");
            let (down, drag, up, button) = if right {
                (kCGEventRightMouseDown, kCGEventRightMouseDragged, kCGEventRightMouseUp, 1)
            } else {
                (kCGEventLeftMouseDown, kCGEventLeftMouseDragged, kCGEventLeftMouseUp, 0)
            };
            unsafe { CGWarpMouseCursorPosition(CGPoint { x: p1.0, y: p1.1 }) };
            sleep(Duration::from_millis(50));
            opt(true);
            mouse(down, p1, button, OPT);
            sleep(Duration::from_millis(30));
            for k in 1..=20 {
                mouse(drag, lerp(p1, p2, k as f64 / 20.0), button, OPT);
                sleep(Duration::from_millis(16));
            }
            sleep(Duration::from_millis(200));
            if esc {
                key(53, true, OPT, false);
                key(53, false, OPT, false);
            }
            mouse(up, p2, button, OPT);
            opt(false);
        }
        // inject pan x1 y1 x2 y2: left Option held, the pointer moving with no button
        "pan" => {
            let (p1, p2) = ((num(a, 1), num(a, 2)), (num(a, 3), num(a, 4)));
            unsafe { CGWarpMouseCursorPosition(CGPoint { x: p1.0, y: p1.1 }) };
            sleep(Duration::from_millis(50));
            mouse(kCGEventMouseMoved, p1, 0, 0);
            opt(true);
            for k in 1..=30 {
                mouse(kCGEventMouseMoved, lerp(p1, p2, k as f64 / 30.0), 0, OPT);
                sleep(Duration::from_millis(16));
            }
            sleep(Duration::from_millis(150));
            opt(false);
        }
        // inject wheel up|down
        "wheel" => {
            let d = if a.get(1).map(String::as_str) == Some("down") { -1 } else { 1 };
            opt(true);
            unsafe { post(CGEventCreateScrollWheelEvent2(src(), kCGScrollEventUnitLine, 1, d, 0, 0), OPT) };
            sleep(Duration::from_millis(20));
            opt(false);
        }
        // inject text: the focused element's text and selection (a test's own document only)
        "text" => {
            let Some(pid) = ax::frontmost_pid() else {
                return 1;
            };
            let app = ax::app_element(pid);
            let Some(el) = ax::copy(app, "AXFocusedUIElement") else {
                eprintln!("inject: no focused element");
                return 1;
            };
            println!(
                "{}",
                serde_json::json!({"value": ax::string(el, "AXValue"), "selected": ax::string(el, "AXSelectedText")})
            );
            ax::release(el);
            ax::release(app);
        }
        _ => {
            eprintln!("usage: newm-mac inject key|hold|rkey|plain|cmd|drag|pan|wheel|text ...");
            return 2;
        }
    }
    0
}
