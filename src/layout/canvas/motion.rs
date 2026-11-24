//! Motion in the usage log: how far, how fast and how hard the camera and the windows move once
//! an operation or a gesture sets them moving, so the disruption of rearrangements and camera
//! moves can be judged on real use.
//!
//! A motion episode begins when an operation starts animations ([`CanvasSpace::apply`]), a pan
//! begins, or a drag's preview moves the other windows. Each frame [`CanvasSpace::sample_motion`]
//! samples what is drawn: the camera's view point, with its view offset and the scale it draws at,
//! and every window that animates, with its render offset, its drag preview and the offset a
//! shrink holds it at. The episode ends once every animation has settled and no pan or drag is
//! on, or when the next operation begins one of its own (`cut` when it had not settled), and its
//! summary is one `{"op":"motion"}` line of the usage log, carrying the sequence number of the
//! operation that started it. Positions are logical pixels of the canvas at 1:1; `camera_zoomed`
//! is the camera's view point at the scale it draws at. A window counts as moved, and an episode
//! is logged at all, from half a pixel of path. Nothing is written per frame, and a frame looks
//! closer only at the windows that animate.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use newm_canvas::Id;
use serde_json::{json, Value};
use smithay::utils::{Logical, Point};

use super::{CanvasSpace, Entry};
use crate::layout::LayoutElement;

static SEQ: AtomicU64 = AtomicU64::new(1);

/// The episode of a drag's preview moving the other windows, which lasts while the drag is on.
pub(super) const DRAG: &str = "drag";

/// The least path, in 1:1 logical pixels, that counts as moving: less does not show.
const MOVED_PX: f64 = 0.5;

/// The episode of a pan, by touchpad or by pointer (the mod key and motion, a touch).
pub(super) fn pan_kind(is_touchpad: bool) -> &'static str {
    if is_touchpad {
        "pan-touchpad"
    } else {
        "pan-pointer"
    }
}

/// Whether this build writes the usage log: the `telemetry` feature.
pub(super) const TELEMETRY: bool = cfg!(feature = "telemetry");

/// The next number in this session's sequence of operations and motion episodes, across canvases.
pub(super) fn next_seq() -> u64 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

pub(super) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Rounds to a tenth, to keep the log short.
pub(super) fn r(x: f64) -> f64 {
    (x * 10.).round() / 10.
}

/// The path, the top speed and acceleration, and the largest single-frame jump of one point from
/// its samples. Speed is in px/s over each frame; acceleration is the change between two frames'
/// speeds over the time between their midpoints.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MotionStats {
    last: Option<(Duration, (f64, f64))>,
    /// The last frame's velocity and its length in seconds.
    velocity: Option<((f64, f64), f64)>,
    pub path: f64,
    pub max_speed: f64,
    pub max_accel: f64,
    pub max_jump: f64,
}

impl MotionStats {
    /// A point at rest at `p` at time `t`: its first frame accelerates from a standstill.
    pub fn at_rest(t: Duration, p: (f64, f64)) -> Self {
        Self {
            last: Some((t, p)),
            velocity: Some(((0., 0.), 0.)),
            ..Self::default()
        }
    }

    /// The point drawn at `p` at time `t`. A move in no time is a jump with no speed.
    pub fn sample(&mut self, t: Duration, p: (f64, f64)) {
        let Some((t0, p0)) = self.last.replace((t, p)) else {
            return;
        };
        let (dx, dy) = (p.0 - p0.0, p.1 - p0.1);
        let d = dx.hypot(dy);
        self.path += d;
        self.max_jump = self.max_jump.max(d);
        let dt = t.saturating_sub(t0).as_secs_f64();
        if dt <= 0. {
            return;
        }
        let v = (dx / dt, dy / dt);
        self.max_speed = self.max_speed.max(v.0.hypot(v.1));
        if let Some((v0, dt0)) = self.velocity {
            let accel = (v.0 - v0.0).hypot(v.1 - v0.1) / ((dt0 + dt) / 2.);
            self.max_accel = self.max_accel.max(accel);
        }
        self.velocity = Some((v, dt));
    }

    fn json(&self) -> Value {
        json!({
            "path": r(self.path),
            "max_speed": r(self.max_speed),
            "max_accel": r(self.max_accel),
            "max_jump": r(self.max_jump),
        })
    }
}

/// What sets the canvas moving in the next [`CanvasSpace::apply`].
#[derive(Debug, Clone, Copy)]
pub(super) struct Cause {
    kind: &'static str,
    seq: Option<u64>,
    /// Carries on an episode of the same kind under way instead of beginning one.
    carry: bool,
}

impl Cause {
    /// Operation `kind` numbered `seq` in the usage log.
    pub fn op(kind: &'static str, seq: u64) -> Self {
        Self {
            kind,
            seq: Some(seq),
            carry: false,
        }
    }

    pub fn new(kind: &'static str) -> Self {
        Self {
            kind,
            seq: None,
            carry: false,
        }
    }

    /// Carries on the episode of `kind` under way: a drag's preview moving again, a pan settling.
    pub fn carry(kind: &'static str) -> Self {
        Self {
            kind,
            seq: None,
            carry: true,
        }
    }
}

/// What is drawn at one moment: the camera's view point and scale, and each grid window by id.
#[derive(Debug)]
pub(super) struct Drawn {
    t: Duration,
    view: Point<f64, Logical>,
    scale: f64,
    windows: HashMap<Id, Point<f64, Logical>>,
}

#[derive(Debug)]
struct Track {
    stats: MotionStats,
    /// The 1:1 cell the window is drawn from, until a change of what is shown.
    cell: Option<Point<f64, Logical>>,
    moving: bool,
}

#[derive(Debug)]
struct Episode {
    kind: &'static str,
    seq: u64,
    start: Duration,
    /// The last sample's time: windows that begin to animate later were at rest then.
    last: Duration,
    frames: u32,
    /// Whether all was at rest when the episode began, and not cut out of another.
    from_rest: bool,
    /// Whether nothing moved at the last sample: no animation, no pan.
    still: bool,
    camera: MotionStats,
    zoomed: MotionStats,
    scale: (f64, f64),
    rest: HashMap<Id, Point<f64, Logical>>,
    windows: HashMap<Id, Track>,
}

impl Episode {
    fn new(kind: &'static str, seq: u64, from_rest: bool, drawn: Drawn) -> Self {
        let start = |p: (f64, f64)| {
            if from_rest {
                MotionStats::at_rest(drawn.t, p)
            } else {
                let mut s = MotionStats::default();
                s.sample(drawn.t, p);
                s
            }
        };
        let (v, s) = (drawn.view, drawn.scale);
        Self {
            kind,
            seq,
            start: drawn.t,
            last: drawn.t,
            frames: 0,
            from_rest,
            still: false,
            camera: start((v.x, v.y)),
            zoomed: start((v.x * s, v.y * s)),
            scale: (s, s),
            rest: drawn.windows,
            windows: HashMap::new(),
        }
    }

    fn moved(&self) -> bool {
        self.camera.path >= MOVED_PX || self.windows.values().any(|w| w.stats.path >= MOVED_PX)
    }
}

/// The motion episode under way, and what the next [`CanvasSpace::apply`] is for.
#[derive(Debug, Default)]
pub(super) struct Motion {
    cause: Option<Cause>,
    episode: Option<Episode>,
    /// A drawn state taken before the op that the next [`CanvasSpace::apply`] runs, for an op
    /// that changes what is drawn before it calls `apply` (a drop takes its preview down).
    before: Option<Drawn>,
    /// Every usage line, kept in tests, where nothing is written.
    #[cfg(test)]
    pub lines: Vec<Value>,
}

impl<W: LayoutElement> CanvasSpace<W> {
    /// Sets what the next [`Self::apply`] is for.
    pub(super) fn cause(&mut self, cause: Cause) {
        self.motion.cause = Some(cause);
    }

    /// Where grid window `e` is drawn on the canvas at 1:1 from `cell`: its render offset (a
    /// move animating, a drag) and the offset a shrink holds it at.
    fn drawn_from(e: &Entry<W>, cell: Point<f64, Logical>) -> Point<f64, Logical> {
        cell + e.tile.render_offset() + e.held
    }

    /// The 1:1 cell window `id` is drawn from: its preview's during a drag, else its own.
    fn drawn_cell(&self, id: Id) -> Option<Point<f64, Logical>> {
        self.shown_rect(id).map(|r| self.cell(r).loc)
    }

    /// What is drawn now. A window being dragged is left out unless it is `dragged`: it follows
    /// the pointer, so where it was is no rest for a later glide.
    pub(super) fn drawn_now(&self, dragged: Option<Id>) -> Drawn {
        let windows = self
            .entries
            .iter()
            .filter(|e| e.tile.sizing_mode().is_normal())
            .filter(|e| {
                e.tile.interactive_move_offset == Point::from((0., 0.)) || Some(e.id) == dragged
            })
            .filter_map(|e| Some((e.id, Self::drawn_from(e, self.drawn_cell(e.id)?))))
            .collect();
        Drawn {
            t: self.clock.now_unadjusted(),
            view: self.view_point(),
            scale: self.camera().scale,
            windows,
        }
    }

    /// Takes what the next [`Self::apply`] is for, and what is drawn before its op; `None`
    /// without telemetry.
    pub(super) fn motion_before(&mut self) -> Option<(Option<Cause>, Drawn)> {
        let cause = self.motion.cause.take();
        let before = self.motion.before.take();
        TELEMETRY.then(|| (cause, before.unwrap_or_else(|| self.drawn_now(None))))
    }

    /// Has the next [`Self::apply`] take `before` as what was drawn before its op: for a drop,
    /// what was drawn while the preview and the dragged window's offset were on.
    pub(super) fn motion_from(&mut self, before: Drawn) {
        self.motion.before = Some(before);
    }

    /// After an op: begins an episode for `cause` from `before` when the op started motion, else
    /// keeps the one under way, whose windows may be drawn from other cells now.
    pub(super) fn motion_after(&mut self, before: Option<(Option<Cause>, Drawn)>, started: bool) {
        let Some((cause, before)) = before else {
            return;
        };
        if started {
            self.begin_motion(cause.unwrap_or(Cause::new("layout")), before);
        } else if let Some(ep) = &mut self.motion.episode {
            for w in ep.windows.values_mut() {
                w.cell = None;
            }
        }
    }

    /// Begins an episode for `cause` from what was drawn `before`, ending the one under way, or
    /// carries on the one under way when `cause` says so and it is of the same kind.
    pub(super) fn begin_motion(&mut self, cause: Cause, before: Drawn) {
        if !TELEMETRY {
            return;
        }
        if cause.carry
            && let Some(ep) = &mut self.motion.episode
            && ep.kind == cause.kind
        {
            for w in ep.windows.values_mut() {
                w.cell = None;
            }
            ep.rest = before.windows;
            return;
        }
        let cut = self.motion.episode.is_some();
        self.end_motion();
        let seq = cause.seq.unwrap_or_else(next_seq);
        self.motion.episode = Some(Episode::new(cause.kind, seq, !cut, before));
    }

    /// Samples the drawn frame into the episode under way, and ends it once all has settled.
    /// Only the windows that animate, or stopped since the last frame, are looked at closer.
    pub(super) fn sample_motion(&mut self) {
        if !TELEMETRY {
            return;
        }
        let _span = tracy_client::span!("CanvasSpace::sample_motion");
        let Some(mut ep) = self.motion.episode.take() else {
            return;
        };
        let t = self.clock.now_unadjusted();
        let view = self.view_point();
        let scale = self.camera().scale;
        ep.camera.sample(t, (view.x, view.y));
        ep.zoomed.sample(t, (view.x * scale, view.y * scale));
        ep.scale = (ep.scale.0.min(scale), ep.scale.1.max(scale));
        let mut moving = false;
        for e in &self.entries {
            let animating = e.tile.are_transitions_ongoing() && e.tile.sizing_mode().is_normal();
            moving |= animating;
            match ep.windows.get(&e.id).map(|w| w.moving) {
                Some(was) if !animating && !was => continue,
                None if !animating => continue,
                Some(_) => (),
                None => {
                    let stats = match ep.rest.get(&e.id) {
                        Some(p) if ep.from_rest => MotionStats::at_rest(ep.last, (p.x, p.y)),
                        Some(p) => {
                            let mut s = MotionStats::default();
                            s.sample(ep.last, (p.x, p.y));
                            s
                        }
                        None => MotionStats::default(),
                    };
                    let track = Track {
                        stats,
                        cell: None,
                        moving: true,
                    };
                    ep.windows.insert(e.id, track);
                }
            }
            let Some(track) = ep.windows.get_mut(&e.id) else {
                continue;
            };
            let cell = match track.cell {
                Some(cell) => cell,
                None => {
                    let Some(cell) = self.drawn_cell(e.id) else {
                        continue;
                    };
                    track.cell = Some(cell);
                    cell
                }
            };
            let p = Self::drawn_from(e, cell);
            track.stats.sample(t, (p.x, p.y));
            track.moving = animating;
        }
        if t > ep.last {
            ep.frames += 1;
        }
        ep.last = t;
        ep.still = !moving
            && !self.view_offset.is_animation_ongoing()
            && !self.view_offset.is_gesture()
            && self.camera_zoom.is_none()
            && self.shrinking.is_empty();
        if ep.still && !(ep.kind == DRAG && self.preview_at.is_some()) {
            self.emit_motion(&ep, false);
        } else {
            self.motion.episode = Some(ep);
        }
    }

    /// Ends the episode under way, if any, as another begins: `cut` when it had not settled.
    fn end_motion(&mut self) {
        if let Some(ep) = self.motion.episode.take() {
            self.emit_motion(&ep, !ep.still);
        }
    }

    fn emit_motion(&mut self, ep: &Episode, cut: bool) {
        if !ep.moved() {
            return;
        }
        let line = self.motion_line(ep, cut);
        self.emit(line);
    }

    fn motion_line(&self, ep: &Episode, cut: bool) -> Value {
        let moved: Vec<(&Id, &Track)> = ep
            .windows
            .iter()
            .filter(|(_, w)| w.stats.path >= MOVED_PX)
            .collect();
        let top = |of: fn(&MotionStats) -> f64| -> Value {
            moved
                .iter()
                .max_by(|a, b| of(&a.1.stats).total_cmp(&of(&b.1.stats)))
                .map_or(
                    Value::Null,
                    |&(&id, w)| json!({ "v": r(of(&w.stats)), "app": self.app_of(id), "id": id }),
                )
        };
        let path = moved.iter().fold(0., |sum, (_, w)| sum + w.stats.path);
        json!({
            "t": now_ms(),
            "op": "motion",
            "seq": ep.seq,
            "kind": ep.kind,
            "cut": cut,
            "ms": r(ep.last.saturating_sub(ep.start).as_secs_f64() * 1000.),
            "frames": ep.frames,
            "scale": [r(ep.scale.0 * 1000.) / 1000., r(ep.scale.1 * 1000.) / 1000.],
            "camera": ep.camera.json(),
            "camera_zoomed": ep.zoomed.json(),
            "windows": {
                "moved": moved.len(),
                "path": r(path),
                "max_path": top(|s| s.path),
                "max_speed": top(|s| s.max_speed),
                "max_accel": top(|s| s.max_accel),
                "max_jump": top(|s| s.max_jump),
            },
        })
    }

    /// The usage lines this canvas wrote, in tests.
    #[cfg(test)]
    pub fn usage_lines(&self) -> &[Value] {
        &self.motion.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6 * b.abs().max(1.)
    }

    #[test]
    fn constant_speed_has_no_acceleration() {
        let mut s = MotionStats::default();
        for k in 0..=10 {
            s.sample(ms(16 * k), (3. * (16 * k) as f64, 4. * (16 * k) as f64));
        }
        assert!(close(s.max_speed, 5000.), "{s:?}");
        assert!(close(s.path, 5. * 160.), "{s:?}");
        assert!(close(s.max_jump, 5. * 16.), "{s:?}");
        assert!(s.max_accel < 1e-6, "{s:?}");
    }

    #[test]
    fn a_jump_is_the_largest_single_frame_move() {
        let mut s = MotionStats::at_rest(ms(0), (0., 0.));
        s.sample(ms(16), (0., 0.));
        s.sample(ms(16), (300., 400.));
        s.sample(ms(32), (300., 400.));
        assert!(close(s.max_jump, 500.), "{s:?}");
        assert!(close(s.path, 500.), "{s:?}");
        assert_eq!(s.max_speed, 0., "a move in no time has no speed: {s:?}");
        let mut s = MotionStats::at_rest(ms(0), (0., 0.));
        s.sample(ms(10), (100., 0.));
        s.sample(ms(20), (100., 0.));
        assert!(close(s.max_jump, 100.), "{s:?}");
        assert!(close(s.max_speed, 10_000.), "{s:?}");
    }

    #[test]
    fn steady_acceleration_from_rest() {
        let a = 2000.;
        let mut s = MotionStats::at_rest(ms(0), (0., 0.));
        for k in 1..=20 {
            let t = (8 * k) as f64 / 1000.;
            s.sample(ms(8 * k), (a * t * t / 2., 0.));
        }
        assert!(close(s.max_accel, a), "{s:?}");
        let t = 0.160;
        assert!(close(s.path, a * t * t / 2.), "{s:?}");
        assert!(
            close(s.max_speed, a * (t - 0.004)),
            "the last frame's mean speed: {s:?}"
        );
    }

    #[test]
    fn unknown_start_speed_counts_no_acceleration_on_the_first_frame() {
        let mut s = MotionStats::default();
        s.sample(ms(0), (0., 0.));
        s.sample(ms(10), (10., 0.));
        assert_eq!(s.max_accel, 0.);
        let mut s = MotionStats::at_rest(ms(0), (0., 0.));
        s.sample(ms(10), (10., 0.));
        assert!(close(s.max_accel, 1000. / 0.005), "{s:?}");
    }
}
