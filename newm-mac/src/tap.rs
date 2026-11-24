#![allow(non_upper_case_globals)]

//! The run loop: a session event tap takes newm's left-Option chords (and left-Option mouse
//! gestures), AX observers report windows opening, closing and taking focus, and timers kick
//! the window manager. The tap only decides and queues; the canvas and every Accessibility call
//! run from a timer right after, so the tap stays fast and is never disabled for a timeout.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::ax;
use crate::ffi::*;
use crate::keys::{self, Action, Chord, Trigger};
use crate::pointer::{OptPointer, PanEv};
use crate::wm::{self, Wm};

/// A pan frame at most every this many seconds.
const PAN_FRAME: f64 = 0.05;

enum Ev {
    Key { action: Action, repeat: bool, t: Instant },
    DragStart { right: bool, p: (f64, f64) },
    DragMove((f64, f64)),
    DragEnd((f64, f64)),
    Cancel,
    Pan(PanEv),
    Ax { pid: i32, what: String },
}

struct Input {
    binds: HashMap<Chord, Action>,
    ptr: OptPointer,
    /// A left-Option mouse gesture in progress: its events are newm-mac's until the button is up.
    button: Option<bool>,
    swallowed: HashSet<i64>,
    tap: CFMachPortRef,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<Ev>> = const { RefCell::new(VecDeque::new()) };
    static INPUT: RefCell<Option<Input>> = const { RefCell::new(None) };
    static WM: RefCell<Option<Wm>> = const { RefCell::new(None) };
    static KICK: Cell<CFRunLoopTimerRef> = const { Cell::new(ptr::null_mut()) };
    static FRAME: Cell<CFRunLoopTimerRef> = const { Cell::new(ptr::null_mut()) };
    static FRAME_ARMED: Cell<bool> = const { Cell::new(false) };
    static TICKS: Cell<u64> = const { Cell::new(0) };
    static LAST_FRAME: Cell<f64> = const { Cell::new(0.0) };
}

static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: i32) {
    QUIT.store(true, Ordering::SeqCst);
}

fn push(ev: Ev) {
    QUEUE.with(|q| q.borrow_mut().push_back(ev));
    KICK.with(|k| unsafe { CFRunLoopTimerSetNextFireDate(k.get(), CFAbsoluteTimeGetCurrent()) });
}

/// Draws a pan frame now, or arms the frame timer when the last one was under `PAN_FRAME` ago;
/// an armed frame draws the latest pointer position.
fn pan_motion(wm: &mut Wm) {
    if FRAME_ARMED.with(Cell::get) {
        return;
    }
    let now = unsafe { CFAbsoluteTimeGetCurrent() };
    let next = LAST_FRAME.with(Cell::get) + PAN_FRAME;
    if now >= next {
        LAST_FRAME.with(|l| l.set(now));
        wm.pan_frame();
    } else {
        FRAME_ARMED.with(|a| a.set(true));
        FRAME.with(|f| unsafe { CFRunLoopTimerSetNextFireDate(f.get(), next) });
    }
}

extern "C" fn on_ax(_obs: AXObserverRef, _el: AXUIElementRef, notif: CFStringRef, refcon: *mut c_void) {
    push(Ev::Ax {
        pid: refcon as usize as i32,
        what: ax::to_string(notif),
    });
}

fn loc(e: CGEventRef) -> (f64, f64) {
    let p = unsafe { CGEventGetLocation(e) };
    (p.x, p.y)
}

extern "C" fn on_event(_proxy: CGEventTapProxy, ty: u32, e: CGEventRef, _info: *mut c_void) -> CGEventRef {
    INPUT.with(|i| {
        let mut guard = i.borrow_mut();
        let Some(inp) = guard.as_mut() else {
            return e;
        };
        if ty == kCGEventTapDisabledByTimeout || ty == kCGEventTapDisabledByUserInput {
            unsafe { CGEventTapEnable(inp.tap, true) };
            return e;
        }
        if unsafe { CGEventGetIntegerValueField(e, kCGEventSourceUserData) } == wm::TAG {
            return e;
        }
        let flags = unsafe { CGEventGetFlags(e) };
        let cmd = flags & keys::CMD != 0;
        match ty {
            kCGEventFlagsChanged => {
                let code = unsafe { CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode) };
                if code == 58
                    && let Some(ev) = inp.ptr.opt(flags & keys::LEFT_OPT != 0, loc(e))
                {
                    push(Ev::Pan(ev));
                }
                e
            }
            kCGEventKeyDown => {
                let code = unsafe { CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode) };
                if inp.button.is_some() && code == 53 {
                    inp.swallowed.insert(code);
                    push(Ev::Cancel);
                    return ptr::null_mut();
                }
                if flags & keys::LEFT_OPT == 0 || cmd {
                    return e;
                }
                let chord = Chord {
                    trigger: Trigger::Key(code as u16),
                    shift: flags & keys::SHIFT != 0,
                    ctrl: flags & keys::CTRL != 0,
                };
                let Some(action) = inp.binds.get(&chord).cloned() else {
                    return e;
                };
                inp.swallowed.insert(code);
                let repeat = unsafe { CGEventGetIntegerValueField(e, kCGKeyboardEventAutorepeat) } != 0;
                push(Ev::Key {
                    action,
                    repeat,
                    t: Instant::now(),
                });
                ptr::null_mut()
            }
            kCGEventKeyUp => {
                let code = unsafe { CGEventGetIntegerValueField(e, kCGKeyboardEventKeycode) };
                if inp.swallowed.remove(&code) { ptr::null_mut() } else { e }
            }
            kCGEventLeftMouseDown | kCGEventRightMouseDown => {
                let opt = inp.ptr.held || flags & keys::LEFT_OPT != 0;
                if !opt || cmd || inp.button.is_some() {
                    return e;
                }
                let right = ty == kCGEventRightMouseDown;
                inp.button = Some(right);
                if let Some(ev) = inp.ptr.stop() {
                    push(Ev::Pan(ev));
                }
                push(Ev::DragStart { right, p: loc(e) });
                ptr::null_mut()
            }
            kCGEventLeftMouseDragged | kCGEventRightMouseDragged => {
                if inp.button.is_none() {
                    return e;
                }
                push(Ev::DragMove(loc(e)));
                ptr::null_mut()
            }
            kCGEventLeftMouseUp | kCGEventRightMouseUp => {
                let p = loc(e);
                inp.ptr.release(p);
                if inp.button.take().is_none() {
                    return e;
                }
                push(Ev::DragEnd(p));
                ptr::null_mut()
            }
            kCGEventMouseMoved => {
                if inp.button.is_none() {
                    for ev in inp.ptr.moved(loc(e)) {
                        push(Ev::Pan(ev));
                    }
                }
                e
            }
            kCGEventScrollWheel => {
                if !(inp.ptr.held || flags & keys::LEFT_OPT != 0) || cmd {
                    return e;
                }
                let d = unsafe { CGEventGetIntegerValueField(e, kCGScrollWheelEventDeltaAxis1) };
                let trigger = if d > 0 { Trigger::WheelUp } else if d < 0 { Trigger::WheelDown } else { return e };
                let chord = Chord {
                    trigger,
                    shift: flags & keys::SHIFT != 0,
                    ctrl: flags & keys::CTRL != 0,
                };
                match inp.binds.get(&chord).cloned() {
                    Some(action) => {
                        push(Ev::Key {
                            action,
                            repeat: false,
                            t: Instant::now(),
                        });
                        ptr::null_mut()
                    }
                    None => e,
                }
            }
            _ => e,
        }
    })
}

fn handle(wm: &mut Wm, ev: Ev) {
    match ev {
        Ev::Key { action, repeat, t } => {
            let moved = wm.run(&action, repeat);
            eprintln!(
                "newm-mac: key {} repeat={repeat} moved={moved} key_to_move_ms={:.2}",
                action.describe(),
                wm::ms(t)
            );
        }
        Ev::DragStart { right, p } => {
            if !wm.drag_start(right, p) {
                eprintln!("newm-mac: no tiled window under the pointer");
            }
        }
        Ev::DragMove(p) => {
            wm.drag_move(p);
        }
        Ev::DragEnd(p) => {
            wm.drag_end(Some(p));
        }
        Ev::Cancel => {
            INPUT.with(|i| {
                if let Some(inp) = i.borrow_mut().as_mut() {
                    inp.button = None;
                }
            });
            wm.drag_end(None);
        }
        Ev::Pan(PanEv::Start(p)) => wm.pan_start(p),
        Ev::Pan(PanEv::Move(p)) => {
            if let Some(pan) = wm.pan.as_mut() {
                pan.last = p;
                pan_motion(wm);
            }
        }
        Ev::Pan(PanEv::End) => {
            wm.pan_end();
        }
        Ev::Ax { pid, what } => match what.as_str() {
            "AXFocusedWindowChanged" | "AXApplicationActivated" => {
                wm.sync_focus(pid);
            }
            "AXWindowResized" => {
                wm.check_sizes(pid);
            }
            _ => {
                let order: Vec<u32> = ax::cg_windows().iter().map(|w| w.wid).collect();
                if wm.scan_app(pid, &order, false) {
                    wm.apply();
                }
            }
        },
    }
}

extern "C" fn on_kick(_t: CFRunLoopTimerRef, _info: *mut c_void) {
    while let Some(mut ev) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
        // Only the latest pointer position of a run of drags matters.
        if matches!(ev, Ev::DragMove(_)) {
            while let Some(next) = QUEUE.with(|q| {
                let mut q = q.borrow_mut();
                matches!(q.front(), Some(Ev::DragMove(_))).then(|| q.pop_front()).flatten()
            }) {
                ev = next;
            }
        }
        WM.with(|w| {
            if let Ok(mut g) = w.try_borrow_mut()
                && let Some(wm) = g.as_mut()
            {
                handle(wm, ev);
            }
        });
    }
}

extern "C" fn on_frame(_t: CFRunLoopTimerRef, _info: *mut c_void) {
    FRAME_ARMED.with(|a| a.set(false));
    WM.with(|w| {
        if let Ok(mut g) = w.try_borrow_mut()
            && let Some(wm) = g.as_mut()
        {
            LAST_FRAME.with(|l| l.set(unsafe { CFAbsoluteTimeGetCurrent() }));
            wm.pan_frame();
        }
    });
}

extern "C" fn on_poll(_t: CFRunLoopTimerRef, _info: *mut c_void) {
    if QUIT.load(Ordering::SeqCst) {
        WM.with(|w| {
            if let Some(wm) = w.borrow_mut().as_mut() {
                wm.restore();
            }
        });
        std::process::exit(0);
    }
    let n = TICKS.with(|t| {
        t.set(t.get() + 1);
        t.get()
    });
    if !n.is_multiple_of(4) {
        return;
    }
    WM.with(|w| {
        if let Ok(mut g) = w.try_borrow_mut()
            && let Some(wm) = g.as_mut()
        {
            wm.children.reap();
            if wm.drag.is_none() && wm.pan.is_none() && wm.scan_all(false) {
                wm.apply();
            }
        }
    });
}

fn timer(interval: f64, cb: CFRunLoopTimerCallBack, first: f64) -> CFRunLoopTimerRef {
    unsafe {
        let t = CFRunLoopTimerCreate(ptr::null(), CFAbsoluteTimeGetCurrent() + first, interval, 0, 0, cb, ptr::null_mut());
        CFRunLoopAddTimer(CFRunLoopGetCurrent(), t, kCFRunLoopCommonModes);
        t
    }
}

pub fn run(opts: wm::Opts) -> i32 {
    if unsafe { AXIsProcessTrusted() } == 0 {
        eprintln!("newm-mac: no Accessibility permission for this process");
        return 1;
    }
    let text = match std::fs::read_to_string(&opts.keys) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("newm-mac: {}: {e}", opts.keys.display());
            return 1;
        }
    };
    let binds = match keys::parse(&text).and_then(|e| keys::bindings(&e)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("newm-mac: {}: {e}", opts.keys.display());
            return 1;
        }
    };
    eprintln!("newm-mac: {} chords from {}", binds.len(), opts.keys.display());
    let mask = [
        kCGEventKeyDown,
        kCGEventKeyUp,
        kCGEventFlagsChanged,
        kCGEventLeftMouseDown,
        kCGEventLeftMouseUp,
        kCGEventLeftMouseDragged,
        kCGEventRightMouseDown,
        kCGEventRightMouseUp,
        kCGEventRightMouseDragged,
        kCGEventMouseMoved,
        kCGEventScrollWheel,
    ]
    .iter()
    .fold(0u64, |m, t| m | (1 << t));
    let tap = unsafe {
        CGEventTapCreate(kCGSessionEventTap, kCGHeadInsertEventTap, kCGEventTapOptionDefault, mask, on_event, ptr::null_mut())
    };
    if tap.is_null() {
        eprintln!("newm-mac: cannot create the event tap (Accessibility or Input Monitoring)");
        return 1;
    }
    unsafe {
        let src = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        for s in [2, 15, 1] {
            signal(s, on_signal);
        }
    }
    INPUT.with(|i| {
        *i.borrow_mut() = Some(Input {
            binds,
            ptr: OptPointer::default(),
            button: None,
            swallowed: HashSet::new(),
            tap,
        })
    });
    let t0 = Instant::now();
    let mut wm = Wm::new(opts, on_ax);
    wm.scan_all(true);
    if let Some(pid) = ax::frontmost_pid()
        && let Some(wid) = wm.apps.get(&pid).and_then(|a| ax::focused_window(a.el))
    {
        wm.canvas.focus_id(wid as u64);
    }
    let moved = wm.apply();
    if let Some(f) = wm.canvas.focus() {
        wm.raise(f);
    }
    eprintln!(
        "newm-mac: took {} windows of {} apps in {:.1} ms ({} moved)",
        wm.wins.len(),
        wm.apps.len(),
        wm::ms(t0),
        moved
    );
    WM.with(|w| *w.borrow_mut() = Some(wm));
    KICK.with(|k| k.set(timer(1e9, on_kick, 1e9)));
    FRAME.with(|f| f.set(timer(1e9, on_frame, 1e9)));
    timer(0.25, on_poll, 0.25);
    unsafe { CFRunLoopRun() };
    0
}
