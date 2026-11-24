//! The usage log: one JSON line per rearrangement of a canvas's windows, appended to
//! `$XDG_STATE_HOME/newm-rs/usage.jsonl` (`~/.local/state/newm-rs/usage.jsonl`), so the placement
//! weights can be tuned on real use later. A line holds the time, the operation, the app it acted
//! on, every window's tiles before and after by app ID, and the cost the placement rules give the
//! change. Never a window title. It also holds the op's number in this session (`seq`), the
//! camera's view before the op and where it goes, at 1:1 and at the zoom, the camera's jump, and
//! how far each window that moved went, in tiles and in 1:1 pixels; the motion the op then draws
//! is a later `{"op":"motion"}` line with the same `seq` (see `motion.rs`).
//!
//! Each op line also holds `op_us`, the microseconds the canvas op took; a line written while an
//! interactive drag is on carries `during_drag`. An `open` line says where the span came from
//! (`size_source`), the size the app had committed (`app_px`), the span, whether the window took
//! the focus, the time since the last open, and how a session restore matched it. `app-grow`
//! lines are an app growing its window (`cause`: `min-size`, `kept-commit` or `config`; `pushed`
//! when the placement rules found no layout). `settle`/`unsettled`, `float` and
//! `restore-expired` lines follow a new window to its cell size, record how a new window was
//! classified, and a placeholder no window took. `contrib/usage-summary.py` sums the file up.
//!
//! The lines go to one writer thread through a bounded channel, so the compositor never waits on
//! the file; when the channel is full a line is dropped, and the next one counts the drops in
//! `dropped`.
//!
//! Only a build with the `telemetry` feature writes the log; without it no line is made, timed or
//! written, and no writer thread starts.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use newm_canvas::{place, Canvas, Id, Rect, Window};
use newm_kde::apps;
use serde_json::{json, Value};
use smithay::utils::{Logical, Point, Size};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData};

use super::motion::{next_seq, now_ms, r, Cause, TELEMETRY};
use super::{CanvasSpace, Watch, SETTLE_WAIT};
use crate::layout::LayoutElement;

/// The camera: the 1:1 view point it draws or goes to, its scale and its zoom.
#[derive(Debug, Clone, Copy)]
struct CameraAt {
    view: Point<f64, Logical>,
    scale: f64,
    zoom: i32,
}

/// The canvas before a logged op.
struct Before {
    windows: Vec<Window>,
    apps: Vec<(Id, Option<String>)>,
    camera: CameraAt,
    seq: u64,
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// The app of window `id`, as [`apps::canonical`] names it.
    pub(super) fn app_of(&self, id: Id) -> Option<String> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.tile.window().app_id())
            .map(|app| apps::canonical(&app))
    }

    /// Runs `op` through [`Self::apply`] and logs it as `name` when it moved a window, opened or
    /// closed one, or is a cancelled drag; `mover` is the window the user acted on and `app` its
    /// app ID.
    pub(super) fn logged<R>(
        &mut self,
        name: &'static str,
        mover: Option<Id>,
        app: Option<String>,
        op: impl FnOnce(&mut Canvas) -> R,
    ) -> R {
        self.logged_with(name, mover, app, op, |_, _| Value::Null)
    }

    /// [`Self::logged`], adding the fields of the object `extra` gives from the canvas space and
    /// the op's result once the op ran.
    pub(super) fn logged_with<R>(
        &mut self,
        name: &'static str,
        mover: Option<Id>,
        app: Option<String>,
        op: impl FnOnce(&mut Canvas) -> R,
        extra: impl FnOnce(&Self, &R) -> Value,
    ) -> R {
        let before = TELEMETRY.then(|| {
            let windows: Vec<Window> = self.canvas.windows().to_vec();
            Before {
                apps: windows.iter().map(|w| (w.id, self.app_of(w.id))).collect(),
                windows,
                camera: CameraAt {
                    view: self.view_point(),
                    scale: self.camera().scale,
                    zoom: self.canvas.zoom(),
                },
                seq: next_seq(),
            }
        });
        if let Some(before) = &before {
            self.cause(Cause::op(name, before.seq));
        }
        let (rv, took) = self.apply(|c| {
            let start = Instant::now();
            let rv = op(c);
            (rv, start.elapsed())
        });
        if took >= crate::utils::SLOW_WORK {
            debug!(
                "slow main-loop work: canvas {name} took {} ms",
                took.as_millis()
            );
        }
        let Some(before) = before else {
            return rv;
        };
        let after = self.canvas.windows();
        let changed = before.windows.len() != after.len()
            || before.windows.iter().any(|w| {
                after
                    .iter()
                    .find(|a| a.id == w.id)
                    .is_none_or(|a| a.rect != w.rect)
            });
        if changed || name == "cancel" {
            let mut line = self.usage_line(name, mover, app, &before);
            line["op_us"] = json!(took.as_micros() as u64);
            if let (Value::Object(line), Value::Object(extra)) = (&mut line, extra(self, &rv)) {
                line.extend(extra);
            }
            self.emit(line);
        }
        rv
    }

    /// Writes `line` to the usage log, marked as written during a drag while one is on; tests keep
    /// it instead.
    pub(super) fn emit(&mut self, mut line: Value) {
        if !TELEMETRY {
            return;
        }
        if self.drag.is_some() {
            line["during_drag"] = json!(true);
        }
        #[cfg(test)]
        self.motion.lines.push(line.clone());
        write(line);
    }

    /// Logs a `settle` line once new window `id` shows the size asked of it last, its cell, after
    /// acking that request: how many sizes were asked of it and the time since it mapped.
    pub(super) fn check_settled(&mut self, id: Id) {
        if !TELEMETRY {
            return;
        }
        let Some(idx) = self.entries.iter().position(|e| e.id == id) else {
            return;
        };
        let e = &self.entries[idx];
        let Some(watch) = e.watch else { return };
        let win = e.tile.window();
        let Some(asked) = win.requested_size() else {
            return;
        };
        if asked.w <= 0 || asked.h <= 0 || win.size() != asked || !win.is_size_settled() {
            return;
        }
        let configures = watch.configures + u32::from(watch.asked != Some(asked));
        let ms = self.clock.now_unadjusted().saturating_sub(watch.since);
        self.entries[idx].watch = None;
        let line = self.settle_line("settle", id, configures, ms);
        self.emit(line);
    }

    /// Logs an `unsettled` line for each new window that has not shown its cell's size
    /// [`SETTLE_WAIT`] after it mapped, and stops following it.
    pub(super) fn watch_unsettled(&mut self, now: Duration) {
        if !TELEMETRY {
            return;
        }
        let _span = tracy_client::span!("CanvasSpace::watch_unsettled");
        let late: Vec<(Id, Watch)> = self
            .entries
            .iter()
            .filter_map(|e| Some((e.id, e.watch?)))
            .filter(|(_, w)| now.saturating_sub(w.since) >= SETTLE_WAIT)
            .collect();
        for (id, watch) in late {
            if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                e.watch = None;
            }
            let line = self.settle_line("unsettled", id, watch.configures, now - watch.since);
            self.emit(line);
        }
    }

    fn settle_line(&self, op: &str, id: Id, configures: u32, took: Duration) -> Value {
        json!({
            "t": now_ms(),
            "op": op,
            "app": self.app_of(id),
            "window": id,
            "configures": configures,
            "ms": took.as_millis() as u64,
        })
    }

    /// The camera's view before an op and where it goes, at 1:1 and at the zoom, and its jump.
    fn camera_json(&self, before: CameraAt) -> Value {
        let after = CameraAt {
            view: self.target_view_point(),
            scale: self.canvas.scale(),
            zoom: self.canvas.zoom(),
        };
        let at = |p: Point<f64, Logical>, s: f64| json!([r(p.x * s), r(p.y * s)]);
        let jump = after.view - before.view;
        json!({
            "before": at(before.view, 1.),
            "after": at(after.view, 1.),
            "before_zoomed": at(before.view, before.scale),
            "after_zoomed": at(after.view, after.scale),
            "jump": [r(jump.x), r(jump.y), r(jump.x.hypot(jump.y))],
            "zoom": [before.zoom, after.zoom],
            "scale": [before.scale, after.scale],
        })
    }

    fn usage_line(
        &self,
        name: &str,
        mover: Option<Id>,
        app: Option<String>,
        before: &Before,
    ) -> Value {
        let Before {
            windows,
            apps,
            camera,
            seq,
        } = before;
        let before: &[Window] = windows;
        let after = self.canvas.windows();
        let app_of = |id: Id| {
            apps.iter()
                .find(|a| a.0 == id)
                .and_then(|a| a.1.clone())
                .or_else(|| self.app_of(id))
        };
        let tiles = |t: Rect| json!([t.i, t.j, t.w, t.h]);
        let list = |ws: &[Window]| -> Vec<Value> {
            ws.iter()
                .map(|w| json!({ "app": app_of(w.id), "id": w.id, "tiles": tiles(w.rect) }))
                .collect()
        };
        let kept: Vec<Window> = before
            .iter()
            .filter(|w| after.iter().any(|a| a.id == w.id))
            .copied()
            .collect();
        let rects: Vec<Rect> = kept
            .iter()
            .map(|w| after.iter().find(|a| a.id == w.id).unwrap().rect)
            .collect();
        let config = self.canvas.config();
        let movers: Vec<Id> = mover.into_iter().collect();
        let cost = place::cost(
            &kept,
            &rects,
            &movers,
            config.distance_weight,
            config.diagonal_weight,
        );
        let islands = |ws: &[Window]| {
            let label = place::islands(ws, &[]);
            label
                .values()
                .collect::<std::collections::HashSet<_>>()
                .len()
        };
        let moved: Vec<Value> = kept
            .iter()
            .zip(&rects)
            .filter_map(|(w, &to)| {
                let from = w.rect;
                let d = self.cell(to).loc - self.cell(from).loc;
                if d == Point::from((0., 0.)) {
                    return None;
                }
                let (di, dj) = (to.i - from.i, to.j - from.j);
                Some(json!({
                    "app": app_of(w.id),
                    "id": w.id,
                    "d_tiles": [di, dj, r(f64::from(di).hypot(f64::from(dj)))],
                    "d_px": [r(d.x), r(d.y), r(d.x.hypot(d.y))],
                }))
            })
            .collect();
        json!({
            "t": now_ms(),
            "seq": seq,
            "op": name,
            "app": app.as_deref().map(apps::canonical),
            "window": mover,
            "zoom": self.canvas.zoom(),
            "before": list(before),
            "after": list(after),
            "tiles": cost.tiles,
            "distance": cost.distance,
            "diagonal": cost.diagonal,
            "islands": [islands(before), islands(after)],
            "weights": [config.distance_weight, config.diagonal_weight],
            "camera": self.camera_json(*camera),
            "moved": moved,
        })
    }
}

/// When the last window opened, in ms since the epoch; 0 before the first.
static LAST_OPEN: AtomicU64 = AtomicU64::new(0);

/// The ms since the last window opened on any canvas (`None` for the first), and now as the last.
pub(super) fn since_last_open() -> Option<u64> {
    let now = now_ms() as u64;
    let last = LAST_OPEN.swap(now, Ordering::Relaxed);
    (last != 0).then(|| now.saturating_sub(last))
}

/// The `float` line for a new window: whether it floats or tiles and why (`tiled`, `dialog` for
/// a child, `popup-like` for a fixed-height or rule-floated window, `overlay`), with its app,
/// minimum and maximum size, parent, the output it opens on and whether a rule decided. `app` is
/// the app as [`apps::canonical`] names it and `app_id` the window's own. `max_huge` flags a
/// maximum of 32767 or more on an axis, which X11 toolkits write on the axis they do not cap;
/// `max_fits` says whether the maximum fits within the output, which an overlay's must.
pub(crate) fn float_line(
    toplevel: &ToplevelSurface,
    (min, max): (Size<i32, Logical>, Size<i32, Logical>),
    (is_floating, is_overlay): (bool, bool),
    by_rule: bool,
    output: Option<Size<i32, Logical>>,
) -> Value {
    let has_parent = toplevel.parent().is_some();
    let decision = if is_overlay {
        "overlay"
    } else if !is_floating {
        "tiled"
    } else if has_parent {
        "dialog"
    } else {
        "popup-like"
    };
    let app = with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().unwrap().app_id.clone())
    });
    json!({
        "t": now_ms(),
        "op": "float",
        "app": app.as_deref().map(apps::canonical),
        "app_id": app,
        "decision": decision,
        "by_rule": by_rule,
        "min": [min.w, min.h],
        "max": [max.w, max.h],
        "has_parent": has_parent,
        "max_huge": max.w >= 32767 || max.h >= 32767,
        "output": output.map(|o| [o.w, o.h]),
        "max_fits": crate::window::cap_fits(max, output),
    })
}

/// Logs a [`float_line`], adding whether xwayland-satellite (process `pid`) serves the window.
pub(crate) fn log_float(mut line: Value, pid: Option<i32>) {
    if !TELEMETRY {
        return;
    }
    line["x11"] = json!(pid.is_some_and(crate::utils::xwayland::is_satellite_pid));
    write(line);
}

/// Without the `telemetry` feature the usage log is compiled out: nothing is written.
#[cfg(not(feature = "telemetry"))]
pub(crate) fn write(_line: Value) {}

#[cfg(feature = "telemetry")]
pub(crate) use file::write;

/// The usage log file and its writer thread, built with the `telemetry` feature only.
#[cfg(feature = "telemetry")]
mod file {
    use std::fs::{File, OpenOptions};
    use std::io::Write as _;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{sync_channel, SyncSender};
    use std::sync::OnceLock;

    use serde_json::{json, Value};

    fn path() -> Option<PathBuf> {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(state.join("newm-rs/usage.jsonl"))
    }

    /// Lines the writer thread may have queued before new ones are dropped.
    const QUEUE: usize = 4096;

    /// Lines dropped since the last one queued.
    static DROPPED: AtomicU64 = AtomicU64::new(0);

    /// The writer thread's queue; `None` when the thread could not start.
    static WRITER: OnceLock<Option<SyncSender<String>>> = OnceLock::new();

    /// Queues one line for the writer thread; a full queue drops it, counted in the next line
    /// queued as `dropped`. A log that cannot be written is skipped, never an error. Tests write
    /// nothing.
    pub(crate) fn write(mut line: Value) {
        if cfg!(test) {
            return;
        }
        let Some(tx) = WRITER.get_or_init(spawn_writer) else {
            return;
        };
        let dropped = DROPPED.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            line["dropped"] = json!(dropped);
        }
        if tx.try_send(line.to_string()).is_err() {
            DROPPED.fetch_add(dropped + 1, Ordering::Relaxed);
        }
    }

    /// The writer thread: it opens the log once, again after a write fails, and a fresh one once
    /// the log reaches [`ROTATE_AT`].
    fn spawn_writer() -> Option<SyncSender<String>> {
        let (tx, rx) = sync_channel::<String>(QUEUE);
        std::thread::Builder::new()
            .name("usage-log".into())
            .spawn(move || {
                let Some(path) = path() else {
                    return;
                };
                let mut log = Log::new(path, ROTATE_AT);
                for line in rx {
                    log.write(&line);
                }
            })
            .ok()?;
        Some(tx)
    }

    /// Bytes the log holds before it moves to `usage.jsonl.1` (replacing the one there) and a fresh
    /// one starts: motion lines alone add megabytes a day, and the log is never read back.
    const ROTATE_AT: u64 = 64 << 20;

    /// The open usage log and how many bytes it holds.
    struct Log {
        path: PathBuf,
        rotate_at: u64,
        file: Option<File>,
        len: u64,
    }

    impl Log {
        fn new(path: PathBuf, rotate_at: u64) -> Self {
            Self {
                path,
                rotate_at,
                file: None,
                len: 0,
            }
        }

        fn write(&mut self, line: &str) {
            if self.file.is_none() {
                if let Some(dir) = self.path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                self.file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)
                    .ok();
                self.len = self
                    .file
                    .as_ref()
                    .and_then(|f| f.metadata().ok())
                    .map_or(0, |m| m.len());
            }
            let Some(f) = &mut self.file else {
                return;
            };
            if writeln!(f, "{line}").is_err() {
                self.file = None;
                return;
            }
            self.len += line.len() as u64 + 1;
            if self.len >= self.rotate_at {
                self.file = None;
                let mut old = self.path.clone().into_os_string();
                old.push(".1");
                let _ = std::fs::rename(&self.path, old);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_log_moves_aside_once_it_is_full() {
            let dir = std::env::temp_dir().join(format!("newm-rs-usage-{}", std::process::id()));
            let path = dir.join("usage.jsonl");
            let mut log = Log::new(path.clone(), 30);
            for k in 0..5 {
                log.write(&format!("{{\"k\":{k}}}"));
            }
            let old = std::fs::read_to_string(dir.join("usage.jsonl.1")).unwrap();
            let new = std::fs::read_to_string(&path).unwrap();
            assert_eq!(old, "{\"k\":0}\n{\"k\":1}\n{\"k\":2}\n{\"k\":3}\n");
            assert_eq!(new, "{\"k\":4}\n");
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}
