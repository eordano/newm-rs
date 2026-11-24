//! The canvas glue: newm-canvas decides where every tiled window goes, and this puts the real
//! macOS windows there through Accessibility. Windows in view take their cell's frame; the
//! others are parked as thin slivers on the nearest left or right screen edge, since nothing can
//! sit above the menu bar and a window below the screen keeps its title bar showing.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use newm_canvas::{Canvas, Config, Dir, Edge, Id, Rect};

use crate::ax;
use crate::children::Children;
use crate::drag::Grab;
use crate::ffi::*;
use crate::keys;
use crate::usage;

pub type Frame = (f64, f64, f64, f64);

/// How much of a parked window shows at the screen edge.
const SLIVER: f64 = 10.0;
/// The least a placed window shows across: macOS pulls a window with less back on screen.
const MIN_SHOW: f64 = 40.0;
/// What a window below the screen keeps showing: its title bar.
const TITLE_KEEP: f64 = 60.0;
/// How far past its cell an app's size counts as its minimum (Terminal snaps to its cells).
const GROW_SLACK: f64 = 16.0;
/// A pan draws at most this many frames; the rest jumps to the end.
pub const PAN_FRAMES: u32 = 20;

pub struct Opts {
    pub keys: PathBuf,
    pub tile: (f64, f64),
    pub top: f64,
    pub bottom: f64,
    pub state: Option<PathBuf>,
    pub terminal: String,
}

pub struct Win {
    pub pid: i32,
    pub el: AXUIElementRef,
    pub app: String,
    /// The frame it had when newm-mac took it, given back on quit.
    pub orig: Frame,
    /// The frame newm-mac last gave it.
    pub last: Option<Frame>,
    pub floating: bool,
    pub full: bool,
}

pub struct App {
    pub el: AXUIElementRef,
    pub obs: AXObserverRef,
    pub name: String,
    pub enhanced: bool,
}

pub struct Drag {
    pub grab: Grab,
    pub frame: Frame,
    pub target: Option<(i32, i32)>,
    pub previews: u32,
    pub preview_ms: f64,
}

pub struct Pan {
    pub view0: (f64, f64),
    pub start: (f64, f64),
    pub last: (f64, f64),
    pub frames: u32,
    pub frame_ms: f64,
    pub moved: usize,
}

pub struct Wm {
    pub canvas: Canvas,
    pub wins: HashMap<Id, Win>,
    pub apps: HashMap<i32, App>,
    pub opts: Opts,
    pub origin: (f64, f64),
    pub screen: (f64, f64),
    pub preview: HashMap<Id, Rect>,
    pub skip: Option<Id>,
    pub drag: Option<Drag>,
    pub pan: Option<Pan>,
    pub observer_cb: AXObserverCallback,
    pub children: Children,
    usage: usage::Log,
    me: i32,
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

fn same(a: Frame, b: Frame) -> bool {
    near(a.0, b.0) && near(a.1, b.1) && near(a.2, b.2) && near(a.3, b.3)
}

fn dir(s: Option<&str>) -> Option<Dir> {
    Some(match s? {
        "left" => Dir::Left,
        "right" => Dir::Right,
        "up" => Dir::Up,
        "down" => Dir::Down,
        _ => return None,
    })
}

fn edge(s: Option<&str>) -> Option<Edge> {
    Some(match s? {
        "left" => Edge::Left,
        "right" => Edge::Right,
        "top" => Edge::Top,
        "bottom" => Edge::Bottom,
        _ => return None,
    })
}

pub fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

impl Wm {
    pub fn new(opts: Opts, observer_cb: AXObserverCallback) -> Wm {
        let b = unsafe { CGDisplayBounds(CGMainDisplayID()) };
        let origin = (b.origin.x, b.origin.y + opts.top);
        let screen = (b.size.width, b.size.height - opts.top - opts.bottom);
        let config = Config {
            tile_w: opts.tile.0,
            tile_h: opts.tile.1,
            ..Config::default()
        };
        let usage = usage::Log::open(std::env::var_os("HOME").map(PathBuf::from).as_deref());
        Wm {
            canvas: Canvas::new(config, screen.0, screen.1),
            wins: HashMap::new(),
            apps: HashMap::new(),
            opts,
            origin,
            screen,
            preview: HashMap::new(),
            skip: None,
            drag: None,
            pan: None,
            observer_cb,
            children: Children::default(),
            usage,
            me: std::process::id() as i32,
        }
    }

    /// One line of the usage log (with the `telemetry` feature): what happened, to which app.
    pub fn log(&mut self, op: &str, id: Option<Id>, extra: serde_json::Value) {
        if !usage::TELEMETRY {
            return;
        }
        let app = id.and_then(|i| self.wins.get(&i)).map(|w| w.app.as_str());
        let line = usage::line(op, app, extra);
        self.usage.write(&line);
    }

    fn add_app(&mut self, pid: i32, name: String) {
        let el = ax::app_element(pid);
        let mut obs: AXObserverRef = std::ptr::null_mut();
        unsafe {
            if AXObserverCreate(pid, self.observer_cb, &mut obs) == kAXErrorSuccess {
                for n in [
                    "AXWindowCreated",
                    "AXFocusedWindowChanged",
                    "AXApplicationActivated",
                    "AXWindowMiniaturized",
                    "AXWindowDeminiaturized",
                ] {
                    AXObserverAddNotification(obs, el, ax::cfstr(n), pid as usize as *mut _);
                }
                CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(obs), kCFRunLoopDefaultMode);
            }
        }
        // Enhanced UI (set by assistive apps) makes Chromium and Electron animate every frame
        // change; it comes back on quit.
        let enhanced = ax::boolean(el, "AXEnhancedUserInterface") == Some(true);
        if enhanced {
            ax::set_bool(el, "AXEnhancedUserInterface", false);
        }
        self.apps.insert(pid, App { el, obs, name, enhanced });
    }

    /// Finds apps and windows: new ones join the canvas, gone ones leave it.
    pub fn scan_all(&mut self, startup: bool) -> bool {
        let cg = ax::cg_windows();
        for w in &cg {
            if w.layer == 0 && w.pid != self.me && !self.apps.contains_key(&w.pid) {
                self.add_app(w.pid, w.owner.clone());
            }
        }
        let order: Vec<u32> = cg.iter().map(|w| w.wid).collect();
        let pids: Vec<i32> = self.apps.keys().copied().collect();
        let mut changed = false;
        for pid in pids {
            changed |= self.scan_app(pid, &order, startup);
        }
        changed
    }

    pub fn scan_app(&mut self, pid: i32, order: &[u32], startup: bool) -> bool {
        let Some(app) = self.apps.get(&pid) else {
            return false;
        };
        let Some(list) = ax::windows(app.el) else {
            if unsafe { libc_kill0(pid) } {
                return false;
            }
            // The app is gone.
            let gone: Vec<Id> = self.wins.iter().filter(|(_, w)| w.pid == pid).map(|(&i, _)| i).collect();
            for id in &gone {
                self.forget(*id);
            }
            self.apps.remove(&pid);
            return !gone.is_empty();
        };
        let mut seen = HashSet::new();
        let mut fresh = Vec::new();
        for el in list {
            let Some(wid) = ax::window_id(el) else {
                ax::release(el);
                continue;
            };
            let id = wid as Id;
            if self.wins.contains_key(&id) {
                if ax::boolean(el, "AXMinimized") != Some(true) {
                    seen.insert(id);
                }
                ax::release(el);
            } else if ax::tileable(el) {
                seen.insert(id);
                fresh.push((wid, el));
            } else {
                ax::release(el);
            }
        }
        fresh.sort_by_key(|(wid, _)| order.iter().position(|o| o == wid).unwrap_or(usize::MAX));
        let mut changed = false;
        for (wid, el) in fresh {
            self.adopt(pid, wid as Id, el, startup);
            changed = true;
        }
        let gone: Vec<Id> = self
            .wins
            .iter()
            .filter(|(id, w)| w.pid == pid && !seen.contains(id))
            .map(|(&i, _)| i)
            .collect();
        for id in gone {
            self.forget(id);
            changed = true;
        }
        changed
    }

    fn adopt(&mut self, pid: i32, id: Id, el: AXUIElementRef, startup: bool) {
        let orig = ax::frame(el).unwrap_or((0.0, 0.0, 400.0, 300.0));
        let app = self.apps.get(&pid).map(|a| a.name.clone()).unwrap_or_default();
        if let Some(a) = self.apps.get(&pid) {
            for n in ["AXUIElementDestroyed", "AXWindowResized"] {
                unsafe { AXObserverAddNotification(a.obs, el, ax::cfstr(n), pid as usize as *mut _) };
            }
        }
        // A window opening while newm-mac runs asks for its own size, as on newm-rs; the ones
        // taken at startup start at one tile and grow to what their app allows.
        let want = (!startup).then(|| self.canvas.need(orig.2, orig.3));
        self.canvas.add(id, 1.0, 1.0, want);
        self.wins.insert(
            id,
            Win {
                pid,
                el,
                app,
                orig,
                last: None,
                floating: false,
                full: false,
            },
        );
        self.log("open", Some(id), serde_json::json!({"startup": startup}));
    }

    fn forget(&mut self, id: Id) {
        if self.drag.as_ref().is_some_and(|d| d.grab.id == id) {
            self.drop_drag();
        }
        self.canvas.remove(id);
        self.preview.remove(&id);
        self.log("close", Some(id), serde_json::Value::Null);
        if let Some(w) = self.wins.remove(&id) {
            ax::release(w.el);
        }
    }

    /// Where a window goes now, and whether it is parked.
    fn target(&self, id: Id) -> Option<(Frame, bool)> {
        let w = self.wins.get(&id)?;
        let (ox, oy) = self.origin;
        let (sw, sh) = self.screen;
        if w.full {
            return Some(((ox, oy, sw, sh), false));
        }
        let r = self.preview.get(&id).copied().or_else(|| self.canvas.rect(id))?;
        let (x, y, ww, hh) = self.canvas.screen_px(r);
        let shown = x + ww >= MIN_SHOW && x <= sw - MIN_SHOW && y >= -0.5 && y <= sh - TITLE_KEEP;
        if shown {
            return Some(((ox + x, oy + y, ww, hh), false));
        }
        let left = x + ww / 2.0 < sw / 2.0;
        let px = if left { ox + SLIVER - ww } else { ox + sw - SLIVER };
        let py = oy + y.clamp(0.0, (sh - TITLE_KEEP).max(0.0));
        Some(((px, py, ww, hh), true))
    }

    /// Puts every tiled window where the canvas (or the drag preview) says, moving only those
    /// whose frame changes. An app that refuses a size smaller than its window's cell gets its
    /// size as the window's minimum and the canvas places it again. Returns the windows moved.
    pub fn apply(&mut self) -> usize {
        let mut moved = 0;
        for _ in 0..4 {
            let mut grow = Vec::new();
            let ids: Vec<Id> = self.canvas.windows().iter().map(|w| w.id).collect();
            for id in ids {
                if Some(id) == self.skip {
                    continue;
                }
                let Some((f, _parked)) = self.target(id) else {
                    continue;
                };
                let w = self.wins.get_mut(&id).unwrap();
                if w.last.is_some_and(|l| same(l, f)) {
                    continue;
                }
                let resize = w.last.is_none_or(|l| !near(l.2, f.2) || !near(l.3, f.3));
                ax::set_point(w.el, f.0, f.1);
                if resize {
                    ax::set_size(w.el, f.2, f.3);
                    ax::set_point(w.el, f.0, f.1);
                }
                w.last = Some(f);
                moved += 1;
                if resize
                    && !w.full
                    && !self.preview.contains_key(&id)
                    && let Some((aw, ah)) = ax::size(w.el)
                    && (aw > f.2 + GROW_SLACK || ah > f.3 + GROW_SLACK)
                {
                    grow.push((id, aw, ah));
                }
            }
            if grow.is_empty() {
                break;
            }
            for (id, aw, ah) in grow {
                let old = self.canvas.window(id).map(|w| (w.min_w, w.min_h)).unwrap_or((1.0, 1.0));
                self.canvas.set_min(id, aw.max(old.0), ah.max(old.1));
                self.log("min", Some(id), serde_json::json!({"w": aw, "h": ah}));
            }
            self.canvas.ensure_focus_visible();
        }
        self.write_state();
        moved
    }

    pub fn raise(&self, id: Id) {
        let Some(w) = self.wins.get(&id) else {
            return;
        };
        ax::perform(w.el, "AXRaise");
        ax::set_bool(w.el, "AXMain", true);
        if let Some(a) = self.apps.get(&w.pid) {
            ax::set_bool(a.el, "AXFrontmost", true);
        }
    }

    /// Follows a focus change made outside newm-mac (a click, ⌘Tab): the canvas focuses the
    /// window and shows it.
    pub fn sync_focus(&mut self, pid: i32) -> usize {
        let Some(a) = self.apps.get(&pid) else {
            return 0;
        };
        let Some(wid) = ax::focused_window(a.el) else {
            return 0;
        };
        let id = wid as Id;
        if self.drag.is_some() || self.canvas.focus() == Some(id) || self.canvas.rect(id).is_none() {
            return 0;
        }
        self.canvas.focus_id(id);
        self.log("focus-follow", Some(id), serde_json::Value::Null);
        self.apply()
    }

    /// An app grew a window past its cell (or would not shrink): its size becomes the minimum.
    pub fn check_sizes(&mut self, pid: i32) -> usize {
        let mut grow = Vec::new();
        for (&id, w) in &self.wins {
            if w.pid != pid || w.full || w.floating || self.preview.contains_key(&id) {
                continue;
            }
            if let (Some(l), Some((aw, ah))) = (w.last, ax::size(w.el))
                && (aw > l.2 + GROW_SLACK || ah > l.3 + GROW_SLACK)
            {
                grow.push((id, aw, ah));
            }
        }
        if grow.is_empty() {
            return 0;
        }
        for (id, aw, ah) in grow {
            self.canvas.set_min(id, aw, ah);
            if let Some(w) = self.wins.get_mut(&id) {
                w.last = None;
            }
            self.log("grow", Some(id), serde_json::json!({"w": aw, "h": ah}));
        }
        self.apply()
    }

    fn focused(&self) -> Option<Id> {
        self.canvas.focus()
    }

    /// Runs a keymap action; returns the windows moved.
    pub fn run(&mut self, action: &keys::Action, repeat: bool) -> usize {
        match action {
            keys::Action::Wm(a) => self.run_wm(a, repeat),
            keys::Action::Spawn(cmd) if !repeat => {
                if let Err(e) = self.children.spawn(cmd) {
                    eprintln!("newm-mac: cannot spawn {cmd}: {e}");
                }
                self.log("spawn", None, serde_json::Value::Null);
                0
            }
            keys::Action::Send { keys: k, app } if !repeat => {
                let app = app.as_ref().map(|a| if a == "terminal" { self.opts.terminal.clone() } else { a.clone() });
                send_later(k.clone(), app);
                self.log("send", None, serde_json::json!({"keys": k}));
                0
            }
            _ => 0,
        }
    }

    fn run_wm(&mut self, a: &str, repeat: bool) -> usize {
        let mut words = a.split_whitespace();
        let op = words.next().unwrap_or("");
        let arg = words.next();
        let n: i32 = words.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let repeats = matches!(
            op,
            "focus-column-left"
                | "focus-column-right"
                | "focus-window-up"
                | "focus-window-down"
                | "move-column-left"
                | "move-column-right"
                | "move-window-up"
                | "move-window-down"
                | "swap-neighbour"
                | "resize-edge"
        );
        if repeat && !repeats {
            return 0;
        }
        let before = self.focused();
        let id = self.focused();
        let done = match op {
            "focus-column-left" => self.canvas.focus_dir(Dir::Left),
            "focus-column-right" => self.canvas.focus_dir(Dir::Right),
            "focus-window-up" => self.canvas.focus_dir(Dir::Up),
            "focus-window-down" => self.canvas.focus_dir(Dir::Down),
            "move-column-left" => id.is_some_and(|i| self.canvas.move_by(i, Dir::Left)),
            "move-column-right" => id.is_some_and(|i| self.canvas.move_by(i, Dir::Right)),
            "move-window-up" => id.is_some_and(|i| self.canvas.move_by(i, Dir::Up)),
            "move-window-down" => id.is_some_and(|i| self.canvas.move_by(i, Dir::Down)),
            "swap-neighbour" => dir(arg).is_some_and(|d| self.canvas.swap(d)),
            "send-to-edge" => dir(arg).is_some_and(|d| self.canvas.send_to_edge(d)),
            "resize-edge" => edge(arg).is_some_and(|e| self.canvas.resize_focused(e, n)),
            "undo" => self.canvas.undo(),
            "redo" => self.canvas.redo(),
            "gather-islands" => self.canvas.gather_islands(),
            "zoom-in" | "zoom-out" | "zoom-reset" => {
                // No overview or zoom on the Mac yet: apps cannot be drawn scaled.
                self.log(op, id, serde_json::json!({"noop": true}));
                return 0;
            }
            "fullscreen-window" => id.is_some_and(|i| {
                let w = self.wins.get_mut(&i).unwrap();
                w.full = !w.full;
                true
            }),
            "toggle-window-floating" => self.toggle_floating(),
            "quit-app" => {
                send_later("Cmd+q".into(), None);
                true
            }
            "close-window" => id.is_some_and(|i| {
                let el = self.wins[&i].el;
                if let Some(b) = ax::copy(el, "AXCloseButton") {
                    ax::perform(b, "AXPress");
                    ax::release(b);
                }
                true
            }),
            "quit" => {
                self.restore();
                std::process::exit(0);
            }
            _ => {
                eprintln!("newm-mac: unknown action {a}");
                false
            }
        };
        if !done {
            return 0;
        }
        self.canvas.ensure_focus_visible();
        let moved = self.apply();
        let after = self.focused();
        if op == "toggle-window-floating" {
            // The window floated or tiled keeps the focus macOS gave it.
        } else if (after != before || op == "fullscreen-window")
            && let Some(f) = after
        {
            self.raise(f);
        }
        self.log(op, after, serde_json::Value::Null);
        moved
    }

    /// Floats the focused window off the canvas, keeping its frame and the focus; on a floating
    /// window (the one macOS has focused), tiles it again beside the canvas's focus.
    fn toggle_floating(&mut self) -> bool {
        let actual = ax::frontmost_pid()
            .and_then(|pid| self.apps.get(&pid))
            .and_then(|a| ax::focused_window(a.el))
            .map(|w| w as Id);
        if let Some(id) = actual.filter(|id| self.wins.get(id).is_some_and(|w| w.floating)) {
            let (w, h) = ax::size(self.wins[&id].el).unwrap_or((1.0, 1.0));
            self.canvas.add(id, 1.0, 1.0, Some(self.canvas.need(w, h)));
            let win = self.wins.get_mut(&id).unwrap();
            win.floating = false;
            win.last = None;
            return true;
        }
        let Some(id) = self.focused() else {
            return false;
        };
        self.canvas.remove(id);
        let w = self.wins.get_mut(&id).unwrap();
        w.floating = true;
        w.last = None;
        true
    }

    /// Gives every window the frame it had before newm-mac, and enhanced UI back to its apps.
    pub fn restore(&mut self) {
        for w in self.wins.values() {
            // Position first: a window low on the screen cannot take a size that runs past its
            // bottom, and macOS clamps the height instead.
            let (x, y, ww, hh) = w.orig;
            ax::set_point(w.el, x, y);
            ax::set_size(w.el, ww, hh);
            ax::set_point(w.el, x, y);
        }
        for a in self.apps.values() {
            if a.enhanced {
                ax::set_bool(a.el, "AXEnhancedUserInterface", true);
            }
        }
        self.log("quit", None, serde_json::json!({"windows": self.wins.len()}));
        eprintln!("newm-mac: gave {} windows their frames back", self.wins.len());
    }

    pub fn drag_start(&mut self, right: bool, p: (f64, f64)) -> bool {
        let (sx, sy) = (p.0 - self.origin.0, p.1 - self.origin.1);
        let Some(id) = self.canvas.window_at(sx, sy) else {
            return false;
        };
        let Some(w) = self.wins.get(&id).filter(|w| !w.full) else {
            return false;
        };
        let (Some(frame), Some(r)) = (w.last.or_else(|| ax::frame(w.el)), self.canvas.rect(id)) else {
            return false;
        };
        let r = self.canvas.screen_px(r);
        let edges = (
            if sx < r.0 + r.2 / 2.0 { Edge::Left } else { Edge::Right },
            if sy < r.1 + r.3 / 2.0 { Edge::Top } else { Edge::Bottom },
        );
        self.raise(id);
        if !right {
            self.skip = Some(id);
        }
        self.drag = Some(Drag {
            grab: Grab {
                id,
                right,
                start: p,
                edges,
            },
            frame,
            target: None,
            previews: 0,
            preview_ms: 0.0,
        });
        true
    }

    /// Ends a drag whose window is gone or off the canvas: the other windows leave their
    /// preview places at the next `apply`.
    fn drop_drag(&mut self) {
        if self.drag.take().is_some() {
            self.skip = None;
            self.preview.clear();
            self.log("drag-lost", None, serde_json::Value::Null);
        }
    }

    pub fn drag_move(&mut self, p: (f64, f64)) -> usize {
        let Some(d) = self.drag.as_ref() else {
            return 0;
        };
        let g = d.grab;
        let (Some(w), Some((t, c))) = (self.wins.get(&g.id), g.layout(&self.canvas, p)) else {
            self.drop_drag();
            return self.apply();
        };
        if !g.right {
            ax::set_point(w.el, d.frame.0 + p.0 - g.start.0, d.frame.1 + p.1 - g.start.1);
        }
        if d.target == Some(t) {
            return 0;
        }
        let t0 = Instant::now();
        let (right, id) = (g.right, g.id);
        self.preview = c
            .windows()
            .iter()
            .filter(|w| (right || w.id != id) && Some(w.rect) != self.canvas.rect(w.id))
            .map(|w| (w.id, w.rect))
            .collect();
        let moved = self.apply();
        if let Some(d) = self.drag.as_mut() {
            d.target = Some(t);
            d.previews += 1;
            d.preview_ms += ms(t0);
        }
        moved
    }

    pub fn drag_end(&mut self, p: Option<(f64, f64)>) -> usize {
        let Some(d) = self.drag.take() else {
            return 0;
        };
        self.skip = None;
        self.preview.clear();
        let id = d.grab.id;
        if !self.wins.contains_key(&id) || self.canvas.rect(id).is_none() {
            self.log("drag-lost", None, serde_json::Value::Null);
            return self.apply();
        }
        let committed = p.is_some_and(|p| d.grab.drop_on(&mut self.canvas, p));
        self.canvas.focus_id(id);
        if let Some(w) = self.wins.get_mut(&id) {
            w.last = None;
        }
        let moved = self.apply();
        self.raise(id);
        let op = match (p.is_some(), d.grab.right) {
            (false, _) => "drag-cancel",
            (true, false) => "drop",
            (true, true) => "resize-drag",
        };
        self.log(
            op,
            Some(id),
            serde_json::json!({"changed": committed, "previews": d.previews, "preview_ms": d.preview_ms}),
        );
        eprintln!(
            "newm-mac: {op} changed={committed} previews={} preview_ms_avg={:.2}",
            d.previews,
            if d.previews > 0 { d.preview_ms / d.previews as f64 } else { 0.0 }
        );
        moved
    }

    pub fn pan_start(&mut self, start: (f64, f64)) {
        self.pan = Some(Pan {
            view0: self.canvas.view(),
            start,
            last: start,
            frames: 0,
            frame_ms: 0.0,
            moved: 0,
        });
    }

    /// Draws one pan frame; false once the pan has used its frames.
    pub fn pan_frame(&mut self) -> bool {
        let Some(p) = self.pan.as_ref() else {
            return false;
        };
        if p.frames >= PAN_FRAMES {
            return false;
        }
        let v = (p.view0.0 - (p.last.0 - p.start.0), p.view0.1 - (p.last.1 - p.start.1));
        self.canvas.set_view(v.0, v.1);
        let t0 = Instant::now();
        let moved = self.apply();
        let p = self.pan.as_mut().unwrap();
        p.frames += 1;
        p.frame_ms += ms(t0);
        p.moved += moved;
        true
    }

    pub fn pan_end(&mut self) -> usize {
        let Some(p) = self.pan.take() else {
            return 0;
        };
        let v = (p.view0.0 - (p.last.0 - p.start.0), p.view0.1 - (p.last.1 - p.start.1));
        self.canvas.set_view(v.0, v.1);
        let before = self.focused();
        self.canvas.focus_view_center();
        let moved = self.apply();
        if let Some(f) = self.focused().filter(|&f| Some(f) != before) {
            self.raise(f);
        }
        self.log(
            "pan",
            self.focused(),
            serde_json::json!({"frames": p.frames, "frame_ms": p.frame_ms, "moved": p.moved}),
        );
        eprintln!(
            "newm-mac: pan frames={} frame_ms_avg={:.2} windows_moved={}",
            p.frames,
            if p.frames > 0 { p.frame_ms / p.frames as f64 } else { 0.0 },
            p.moved
        );
        moved
    }

    fn write_state(&self) {
        let Some(path) = self.opts.state.as_ref() else {
            return;
        };
        let wins: Vec<serde_json::Value> = self
            .wins
            .iter()
            .map(|(&id, w)| {
                let r = self.canvas.rect(id);
                serde_json::json!({
                    "id": id, "app": w.app, "floating": w.floating, "full": w.full,
                    "rect": r.map(|r| [r.i, r.j, r.w, r.h]),
                    "parked": self.target(id).map(|t| t.1),
                    "frame": w.last.map(|f| [f.0, f.1, f.2, f.3]),
                    "orig": [w.orig.0, w.orig.1, w.orig.2, w.orig.3],
                })
            })
            .collect();
        let s = serde_json::json!({
            "focus": self.canvas.focus(), "view": [self.canvas.view().0, self.canvas.view().1],
            "tile": [self.opts.tile.0, self.opts.tile.1], "origin": [self.origin.0, self.origin.1],
            "screen": [self.screen.0, self.screen.1], "windows": wins,
        });
        let _ = std::fs::write(path, s.to_string());
    }
}

/// Whether a process still exists.
unsafe fn libc_kill0(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, 0) == 0 }
}

/// A tag on the events newm-mac posts, so its own tap lets them through.
pub const TAG: i64 = 0x6e65_776d;

/// Posts a native shortcut, on its own source so the held left Option does not join it.
pub fn post_shortcut(code: u16, flags: u64, tag: bool) {
    unsafe {
        let src = CGEventSourceCreate(kCGEventSourceStatePrivate);
        for down in [true, false] {
            let e = CGEventCreateKeyboardEvent(src, code, down);
            CGEventSetFlags(e, flags);
            if tag {
                CGEventSetIntegerValueField(e, kCGEventSourceUserData, TAG);
            }
            CGEventPost(kCGHIDEventTap, e);
            CFRelease(e as CFTypeRef);
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        if !src.is_null() {
            CFRelease(src as CFTypeRef);
        }
    }
}

/// Activates `app` (a bundle id) when given, then posts `keys`, off the run loop.
pub fn send_later(keys: String, app: Option<String>) {
    std::thread::spawn(move || {
        let Ok((code, flags)) = keys::shortcut(&keys) else {
            eprintln!("newm-mac: cannot post {keys}");
            return;
        };
        if let Some(app) = app {
            let _ = std::process::Command::new("/usr/bin/open").arg("-b").arg(&app).status();
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        // Let the user's own key-up of the chord pass first.
        std::thread::sleep(std::time::Duration::from_millis(30));
        post_shortcut(code, flags, true);
    });
}
