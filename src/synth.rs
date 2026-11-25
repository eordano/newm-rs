//! newm's synth background: newm-synth's look drawn by a fragment shader behind everything.
//!
//! With the mode off nothing exists: no look, no texture, no element. On, the look's seeded
//! tables live in one 256×260 RGBA texture, uploaded when the seed changes. Each output has its
//! own element and its own scene time, so a redraw for any other reason repaints damaged areas
//! with the same picture as the rest of that output.
//!
//! Animated, the picture moves on the output's own frame clock, as niri's animations do: each
//! redraw sets the scene time from the frame's target presentation time
//! ([`Niri::synth_advance`]), and while the picture keeps moving the redraw asks for the next
//! frame ([`Niri::synth_after_render`] sets `unfinished_animations_remain`), so it runs at the
//! output's refresh rate, VRR included. The scene time is the presentation time minus an
//! anchor, so its steps are exactly the presentation intervals. When the picture stops (any
//! gate below closes) the anchor is dropped, and the next move sets a new one from the time the
//! picture shows, so it continues where it stopped without a jump.
//!
//! The picture moves on an output only while all of these hold ([`Niri::synth_moves`]): the
//! mode is animated and the look moves (or a sound level is still moving); the output shows the
//! background (monitors on, session unlocked, no screenshot UI, no opaque fullscreen window);
//! at least [`MIN_VISIBLE`] of it showed in the last frame; not on battery with `on-battery
//! "still"`; and not idle past `idle-still-after` without an idle inhibitor. `max-fps`, or
//! `on-battery "capped"` on battery, caps the moves: frames still come at the display's rate
//! while a move is due within a frame and a half, and otherwise one timer queues the redraw.
//! [`Niri::synth_refresh`], run after every event loop dispatch, starts outputs whose gates
//! opened. Blurred windows see a copy of the picture that follows it once a second
//! ([`XRAY_STEP`]), so the blur is not redone every frame.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use newm_synth::{quantise, Frame, Level, Look, Tables, TABLE_HEIGHT, TABLE_WIDTH};
use niri_config::{SynthBackground, SynthOnBattery};
use niri_ipc::SynthMode;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::{Element, Id, Kind};
use smithay::backend::renderer::gles::{
    GlesRenderer, GlesTexture, Uniform, UniformName, UniformType, UniformValue,
};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::backend::renderer::ImportMem as _;
use smithay::output::Output;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::RegistrationToken;
use smithay::utils::{Logical, Physical, Rectangle, Scale, Size};

use crate::niri::Niri;
use crate::render_helpers::shader_element::{ShaderProgram, ShaderRenderElement};
use crate::render_helpers::shaders::{ProgramType, Shaders};
use crate::utils::{get_monotonic_time, output_size};

/// The least share of an output the background must show for its picture to keep moving. Below
/// it only slivers between windows show, and moving them would cost a redraw and a page flip
/// every frame (and keep a laptop panel out of self refresh) for motion nobody can follow.
pub const MIN_VISIBLE: f64 = 0.05;

/// How far the blurred copy may fall behind the picture before it follows.
pub const XRAY_STEP: Duration = Duration::from_secs(1);

/// The refresh interval assumed for an output whose frame clock has none.
const DEFAULT_REFRESH: Duration = Duration::from_micros(16_667);

/// The synth background's state in the compositor.
#[derive(Debug)]
pub struct Synth {
    /// The mode an action set, which holds until the config's mode changes.
    override_mode: Option<SynthMode>,
    /// `None` while the mode is off.
    active: Option<Active>,
    /// `max-fps`: a cap on the moves a second; `None` moves on every frame.
    max_fps: Option<f64>,
    on_battery: SynthOnBattery,
    on_battery_max_fps: f64,
    /// `idle-still-after`; `None` never stops it.
    idle_still_after: Option<Duration>,
    /// The monotonic time of the last input.
    last_input: Duration,
    /// Whether an idle inhibitor (a video playing) is active.
    pub idle_inhibited: bool,
    /// The timer that queues the next capped move, and when it fires.
    wake: Option<(RegistrationToken, Duration)>,
    /// The power supply watch, while `on-battery` could change anything.
    power: Option<power::Watch>,
    /// Watching the power supplies failed: not tried again until the config changes.
    power_failed: bool,
    /// `react-to-sound`, from the config.
    react_to_sound: bool,
    /// The capture, only while the background listens (see [`Synth::wants_sound`]).
    sound: Option<SoundHandle>,
    /// Opening the capture failed (no PipeWire): not tried again until the config changes.
    sound_failed: bool,
    /// The loudest sound block's level since the last move, set by the capture.
    peak: Rc<Cell<f32>>,
    level: Level,
    /// The presentation time the level was last advanced to.
    level_at: Option<Duration>,
}

impl Default for Synth {
    fn default() -> Self {
        Self {
            override_mode: None,
            active: None,
            max_fps: None,
            on_battery: SynthOnBattery::Still,
            on_battery_max_fps: 30.,
            idle_still_after: None,
            last_input: get_monotonic_time(),
            idle_inhibited: false,
            wake: None,
            power: None,
            power_failed: false,
            react_to_sound: false,
            sound: None,
            sound_failed: false,
            peak: Rc::default(),
            level: Level::new(),
            level_at: None,
        }
    }
}

/// The capture: PipeWire, or in tests a stand-in whose level the tests set through `peak`.
#[derive(Debug)]
enum SoundHandle {
    /// Held for its `Drop`, which disconnects the stream.
    #[cfg(feature = "synth")]
    #[cfg_attr(test, allow(dead_code))]
    PipeWire(#[allow(dead_code)] sound::Sound),
    #[cfg(test)]
    Fake,
}

#[derive(Debug)]
struct Active {
    look: Look,
    mode: SynthMode,
    /// The seeded tables on the GPU; `None` until the next draw uploads them.
    texture: RefCell<Option<GlesTexture>>,
    /// Each output's picture, by output name; made at its first draw.
    outputs: RefCell<HashMap<String, Pace>>,
    /// The copy blurred windows see.
    xray: Picture,
    /// The sound level the pictures show, as a step of the look's level quantum and the level
    /// it draws as.
    drawn_step: u32,
    drawn_level: f32,
}

/// A picture an element draws: its id, the counter that moves when it changes, its scene time.
#[derive(Debug)]
struct Picture {
    id: Id,
    commit: CommitCounter,
    time: Duration,
}

impl Picture {
    fn new(time: Duration) -> Self {
        Self {
            id: Id::new(),
            commit: CommitCounter::default(),
            time,
        }
    }
}

/// One output's picture and how it moves.
#[derive(Debug)]
struct Pace {
    picture: Picture,
    /// The presentation time minus the scene time while the picture moves; `None` while it
    /// stands, so the next move continues from where it stopped.
    anchor: Option<Duration>,
    /// The presentation time of the last move, for the cap.
    moved_at: Option<Duration>,
    /// Whether frames keep coming to move the picture.
    running: bool,
    /// The share of the output the background showed in the last frame, 0 to 1.
    visible: f64,
}

impl Pace {
    fn new(time: Duration) -> Self {
        Self {
            picture: Picture::new(time),
            anchor: None,
            moved_at: None,
            running: false,
            visible: 1.,
        }
    }
}

impl Active {
    /// The output's pace, made on first use at the scene time the other outputs show.
    fn pace<'a>(
        outputs: &'a mut HashMap<String, Pace>,
        name: &str,
        xray: &Picture,
    ) -> &'a mut Pace {
        if !outputs.contains_key(name) {
            let start = outputs
                .values()
                .map(|p| p.picture.time)
                .max()
                .unwrap_or(xray.time);
            outputs.insert(name.to_owned(), Pace::new(start));
        }
        outputs.get_mut(name).unwrap()
    }
}

/// The look a config block asks for: the user's tuned look with the block's knobs on top.
pub fn look(c: &SynthBackground) -> Look {
    let mut l = Look::default();
    if let Some(seed) = c.seed {
        l.seed = seed;
    }
    macro_rules! numbers {
        ($($f:ident),* $(,)?) => {$(
            if let Some(x) = c.$f {
                l.$f = x as f32;
            }
        )*};
    }
    numbers!(
        horizon,
        sky_fall,
        stars,
        sun_size,
        sun_y,
        sun_x,
        stripes,
        stripe_width,
        halo,
        reflection,
        layers,
        ridge_height,
        roughness,
        ridge_frequency,
        ridge_speed,
        haze,
        rim,
        perspective,
        grid_x,
        grid_z,
        line_width,
        grid_glow,
        grid_speed,
        curve,
        cycle_speed,
        cycle_sun,
        cycle_hue,
        hue,
        saturation,
        brightness,
        scanlines,
        bits,
        audio_sun,
        audio_grid,
        audio_ridge,
    );
    macro_rules! colours {
        ($($f:ident),* $(,)?) => {$(
            if let Some(x) = c.$f {
                l.$f = [x.r, x.g, x.b];
            }
        )*};
    }
    colours!(
        sky_color,
        glow_color,
        grid_color,
        sun_top_color,
        sun_bottom_color,
        ridge_color,
    );
    l
}

/// The share of `full` that the `opaque` rectangles leave showing, 0 to 1. A region cut into
/// more than 512 pieces counts as showing: too fragmented to be worth the count.
pub fn visible_share(
    full: Rectangle<i32, Physical>,
    opaque: impl IntoIterator<Item = Rectangle<i32, Physical>>,
) -> f64 {
    let area = |r: &Rectangle<i32, Physical>| f64::from(r.size.w) * f64::from(r.size.h);
    let total = area(&full);
    if total <= 0. {
        return 0.;
    }
    let mut left = vec![full];
    for r in opaque {
        if !left.iter().any(|l| l.overlaps(r)) {
            continue;
        }
        left = Rectangle::subtract_rects_many(left, [r]);
        if left.is_empty() {
            return 0.;
        }
        if left.len() > 512 {
            return 1.;
        }
    }
    left.iter().map(area).sum::<f64>() / total
}

impl Synth {
    pub fn new(config: &SynthBackground) -> Self {
        let mut synth = Self::default();
        synth.apply_config(config);
        synth
    }

    /// The mode in effect: an action's, else the config's.
    pub fn mode(&self) -> SynthMode {
        self.active.as_ref().map_or(SynthMode::Off, |a| a.mode)
    }

    /// Whether anything of the background exists: a look, a texture or an element.
    pub fn is_allocated(&self) -> bool {
        self.active.is_some()
    }

    /// Applies the config's block. The scene times keep running; the tables are uploaded again
    /// only when the seed changed. Returns whether the picture changed.
    pub fn apply_config(&mut self, config: &SynthBackground) -> bool {
        self.max_fps = config.max_fps;
        self.on_battery = config.on_battery;
        self.on_battery_max_fps = config.on_battery_max_fps;
        self.idle_still_after = (config.idle_still_after > 0.)
            .then(|| Duration::from_secs_f64(config.idle_still_after));
        self.react_to_sound = config.react_to_sound;
        self.sound_failed = false;
        self.power_failed = false;
        let mode = self.override_mode.unwrap_or(config.mode);
        self.set(mode, look(config))
    }

    /// Whether frames keep coming to move the picture on some output.
    pub fn is_moving(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|a| a.outputs.borrow().values().any(|p| p.running))
    }

    /// Whether the cap's timer is armed.
    pub fn is_waiting(&self) -> bool {
        self.wake.is_some()
    }

    /// The commit counter of `output`'s picture, which moves whenever it changes.
    pub fn commit(&self, output: &str) -> Option<CommitCounter> {
        let active = self.active.as_ref()?;
        let outputs = active.outputs.borrow();
        outputs.get(output).map(|p| p.picture.commit)
    }

    /// The scene time `output`'s picture shows.
    pub fn scene_time(&self, output: &str) -> Option<Duration> {
        let active = self.active.as_ref()?;
        let outputs = active.outputs.borrow();
        outputs.get(output).map(|p| p.picture.time)
    }

    /// The scene time and commit of the copy blurred windows see.
    pub fn xray_picture(&self) -> Option<(Duration, CommitCounter)> {
        self.active.as_ref().map(|a| (a.xray.time, a.xray.commit))
    }

    /// Records input at monotonic time `now`, which ends an idle stop.
    pub fn input(&mut self, now: Duration) {
        self.last_input = now;
    }

    /// Whether the power supply watch says the machine runs on battery.
    pub fn on_battery(&self) -> bool {
        self.power.as_ref().is_some_and(|p| p.on_battery)
    }

    /// For tests: the machine runs on battery or not, as the power supply watch would report.
    #[cfg(test)]
    pub fn set_on_battery(&mut self, on_battery: bool) {
        self.power = Some(power::Watch::fake(on_battery));
    }

    /// The cap on moves a second in effect, if any.
    fn cap(&self) -> Option<f64> {
        match (self.on_battery(), self.on_battery) {
            (true, SynthOnBattery::Capped) => Some(
                self.on_battery_max_fps
                    .min(self.max_fps.unwrap_or(f64::INFINITY)),
            ),
            _ => self.max_fps,
        }
    }

    /// Whether the power or the idle time stop the picture at monotonic time `now`.
    fn held(&self, now: Duration) -> bool {
        let battery = self.on_battery() && self.on_battery == SynthOnBattery::Still;
        let idle = !self.idle_inhibited
            && self
                .idle_still_after
                .is_some_and(|after| now.saturating_sub(self.last_input) >= after);
        battery || idle
    }

    /// Whether the picture has anything to move: animated, and a look that moves or a sound
    /// level that is still moving (sound came in, or the level has not settled where the
    /// picture shows it).
    fn wants_ticks(&self) -> bool {
        self.active.as_ref().is_some_and(|a| {
            a.mode == SynthMode::Animated
                && (a.look.moves()
                    || (self.sound.is_some()
                        && (self.peak.get() > 0.
                            || self.level.pending()
                            || self.level.level() > 0.
                            || a.drawn_step != 0)))
        })
    }

    /// Whether the power supplies need watching: animated, and `on-battery` not "animated".
    fn wants_power(&self) -> bool {
        self.on_battery != SynthOnBattery::Animated
            && !self.power_failed
            && self
                .active
                .as_ref()
                .is_some_and(|a| a.mode == SynthMode::Animated)
    }

    /// Whether the background should listen: `react-to-sound` on, animated, shown on some
    /// output (`visible`), and some audio amount not zero. Nothing else opens the capture.
    pub fn wants_sound(&self, visible: bool) -> bool {
        self.react_to_sound
            && visible
            && !self.sound_failed
            && self
                .active
                .as_ref()
                .is_some_and(|a| a.mode == SynthMode::Animated && a.look.hears())
    }

    /// Whether the capture is open.
    pub fn is_listening(&self) -> bool {
        self.sound.is_some()
    }

    /// For tests: a sound block of `level` came in, as the capture would report it.
    #[cfg(test)]
    pub fn hear(&self, level: f32) {
        self.peak.set(self.peak.get().max(level));
    }

    /// The step the drawn picture shows the level at.
    pub fn drawn_step(&self) -> Option<u32> {
        self.active.as_ref().map(|a| a.drawn_step)
    }

    /// Sets the mode until the config's mode changes. Returns whether the picture changed.
    pub fn set_mode(&mut self, mode: SynthMode, config: &SynthBackground) -> bool {
        self.override_mode = Some(mode);
        self.apply_config(config)
    }

    /// Starts or stops the motion: from animated back to the configured mode when that is off
    /// or still (to still when the config says animated), from anything else to animated.
    /// Returns whether the picture changed.
    pub fn toggle(&mut self, config: &SynthBackground) -> bool {
        let mode = match (self.mode(), config.mode) {
            (SynthMode::Animated, SynthMode::Animated) => SynthMode::Still,
            (SynthMode::Animated, resting) => resting,
            _ => SynthMode::Animated,
        };
        self.set_mode(mode, config)
    }

    /// Forgets an action's mode because the config's mode changed.
    pub fn config_mode_changed(&mut self) {
        self.override_mode = None;
    }

    fn set(&mut self, mode: SynthMode, look: Look) -> bool {
        if mode == SynthMode::Off {
            return self.active.take().is_some();
        }
        match &mut self.active {
            Some(active) => {
                active.mode = mode;
                if active.look == look {
                    return false;
                }
                if active.look.seed != look.seed {
                    active.texture.replace(None);
                }
                active.look = look;
                for pace in active.outputs.get_mut().values_mut() {
                    pace.picture.commit.increment();
                }
                active.xray.commit.increment();
                true
            }
            None => {
                self.active = Some(Active {
                    look,
                    mode,
                    texture: RefCell::new(None),
                    outputs: RefCell::new(HashMap::new()),
                    xray: Picture::new(Duration::ZERO),
                    drawn_step: 0,
                    drawn_level: 0.,
                });
                true
            }
        }
    }

    /// The element for output `name`, `size` physical pixels large at `scale`, or `None` while
    /// the mode is off or the GPU cannot draw it.
    pub fn render(
        &self,
        renderer: &mut GlesRenderer,
        name: &str,
        size: Size<i32, Physical>,
        scale: Scale<f64>,
    ) -> Option<ShaderRenderElement> {
        let active = self.active.as_ref()?;
        let (time, id, commit) = {
            let mut outputs = active.outputs.borrow_mut();
            let pace = Active::pace(&mut outputs, name, &active.xray);
            (
                pace.picture.time,
                pace.picture.id.clone(),
                pace.picture.commit,
            )
        };
        self.element_at(renderer, time, id, commit, size, scale)
    }

    /// The element for the copy blurred windows see, sized as [`Synth::render`]'s.
    pub fn render_xray(
        &self,
        renderer: &mut GlesRenderer,
        size: Size<i32, Physical>,
        scale: Scale<f64>,
    ) -> Option<ShaderRenderElement> {
        let active = self.active.as_ref()?;
        let x = &active.xray;
        self.element_at(renderer, x.time, x.id.clone(), x.commit, size, scale)
    }

    fn element_at(
        &self,
        renderer: &mut GlesRenderer,
        time: Duration,
        id: Id,
        commit: CommitCounter,
        size: Size<i32, Physical>,
        scale: Scale<f64>,
    ) -> Option<ShaderRenderElement> {
        let active = self.active.as_ref()?;
        if size.w <= 0 || size.h <= 0 {
            return None;
        }
        ensure_program(renderer)?;
        let mut texture = active.texture.borrow_mut();
        if texture.is_none() {
            match upload_tables(renderer, active.look.seed) {
                Ok(t) => *texture = Some(t),
                Err(err) => {
                    warn!("error uploading the synth background tables: {err:?}");
                    return None;
                }
            }
        }
        let frame = Frame::new(
            &active.look,
            size.w as usize,
            size.h as usize,
            time.as_secs_f64(),
            active.drawn_level,
        );
        Some(element(&frame, texture.clone()?, id, commit, size, scale))
    }
}

/// What decides whether an output shows the background, apart from the compositor so each case
/// can be checked alone.
#[derive(Debug, Clone, Copy)]
pub struct Visibility {
    pub monitors_active: bool,
    pub locked: bool,
    pub screenshot_ui: bool,
    /// An opaque fullscreen window covers the whole output.
    pub covered: bool,
}

impl Visibility {
    pub fn shows(self) -> bool {
        self.monitors_active && !self.locked && !self.screenshot_ui && !self.covered
    }
}

impl Niri {
    /// The `toggle-synth-background` action.
    pub fn synth_toggle(&mut self) {
        let config = self.config.borrow().synth_background.clone();
        if self.synth.toggle(&config) {
            self.queue_redraw_all();
        }
    }

    /// The `set-synth-background` action.
    pub fn synth_set_mode(&mut self, mode: SynthMode) {
        let config = self.config.borrow().synth_background.clone();
        if self.synth.set_mode(mode, &config) {
            self.queue_redraw_all();
        }
    }

    /// Whether `output` would show the synth background if it were on.
    pub fn synth_visibility(&self, output: &Output) -> Visibility {
        let covered = self.layout.monitor_for_output(output).is_some_and(|mon| {
            // A fullscreen window drawn above the top layer fills the output over its
            // fullscreen backdrop; with an opaque backdrop nothing behind it shows.
            mon.render_above_top_layer()
                && self.config.borrow().layout.fullscreen_backdrop_color.a >= 1.
        });
        Visibility {
            monitors_active: self.monitors_active,
            locked: self.is_locked(),
            screenshot_ui: self.screenshot_ui.is_open(),
            covered,
        }
    }

    /// The tallest output that shows the background, in physical pixels; 0 when none does.
    fn synth_visible_height(&self) -> u32 {
        let mut height = 0;
        for output in self.output_state.keys() {
            if self.synth_visibility(output).shows() {
                let scale = Scale::from(output.current_scale().fractional_scale());
                let h: i32 = output_size(output).to_physical_precise_round(scale).h;
                height = height.max(u32::try_from(h).unwrap_or(0));
            }
        }
        height
    }

    fn synth_any_visible(&self) -> bool {
        self.output_state
            .keys()
            .any(|output| self.synth_visibility(output).shows())
    }

    /// The share of `output` the background showed in its last frame; 1 before any.
    pub fn synth_visible_share(&self, output: &Output) -> f64 {
        self.synth.active.as_ref().map_or(1., |a| {
            a.outputs
                .borrow()
                .get(&output.name())
                .map_or(1., |p| p.visible)
        })
    }

    /// Whether the picture should move on `output` at monotonic time `now`: every gate in the
    /// module docs open.
    pub fn synth_moves(&self, output: &Output, now: Duration) -> bool {
        self.synth.wants_ticks()
            && !self.synth.held(now)
            && self.synth_visibility(output).shows()
            && self.synth_visible_share(output) >= MIN_VISIBLE
    }

    fn synth_refresh_interval(&self, output: &Output) -> Duration {
        self.output_state
            .get(output)
            .and_then(|s| s.frame_clock.refresh_interval())
            .unwrap_or(DEFAULT_REFRESH)
    }

    /// Measures how much of `output` the background shows in a frame of `elements` (front to
    /// back, as the frame draws them): the output less the opaque regions of everything in
    /// front of the background's element. Only an animated background measures.
    pub fn synth_measure<E: Element>(&self, output: &Output, elements: &[E]) {
        let Some(active) = &self.synth.active else {
            return;
        };
        if active.mode != SynthMode::Animated {
            return;
        }
        let _span = tracy_client::span!("Niri::synth_measure");
        let mut outputs = active.outputs.borrow_mut();
        let Some(pace) = outputs.get_mut(&output.name()) else {
            return;
        };
        let Some(i) = elements.iter().position(|e| *e.id() == pace.picture.id) else {
            return;
        };
        let scale = Scale::from(output.current_scale().fractional_scale());
        let full = Rectangle::from_size(output_size(output).to_physical_precise_round(scale));
        let opaque = elements[..i].iter().flat_map(|e| {
            let loc = e.geometry(scale).loc;
            e.opaque_regions(scale)
                .into_iter()
                .map(move |r| Rectangle::new(r.loc + loc, r.size))
        });
        pace.visible = visible_share(full, opaque);
    }

    /// Moves `output`'s picture to the frame's target presentation time `target`, before the
    /// frame renders. A picture that is not running stands and forgets its anchor; a capped one
    /// moves only once its next move is due (within half a frame).
    pub fn synth_advance(&mut self, output: &Output, target: Duration) {
        if self.synth.active.is_none() {
            return;
        }
        let _span = tracy_client::span!("Niri::synth_advance");
        let refresh = self.synth_refresh_interval(output);
        let height = self.synth_visible_height();
        let cap = self.synth.cap();
        let Synth {
            active,
            sound,
            peak,
            level,
            level_at,
            ..
        } = &mut self.synth;
        let Some(active) = active else {
            return;
        };
        let mut outputs = active.outputs.borrow_mut();
        let pace = Active::pace(&mut outputs, &output.name(), &active.xray);
        if !pace.running {
            pace.anchor = None;
            return;
        }
        if let (Some(fps), Some(at)) = (cap, pace.moved_at) {
            let due = at + Duration::from_secs_f64(1. / fps);
            if target + refresh / 2 < due {
                return;
            }
        }
        let anchor = *pace
            .anchor
            .get_or_insert(target.saturating_sub(pace.picture.time));
        let time = if active.look.moves() {
            target.saturating_sub(anchor)
        } else {
            pace.picture.time
        };
        pace.moved_at = Some(target);

        let dt = level_at.map_or(Duration::ZERO, |at| target.saturating_sub(at));
        *level_at = Some(level_at.map_or(target, |at| at.max(target)));
        let (step, drawn_level) = match (sound.is_some(), active.look.level_quantum(height)) {
            (true, Some(q)) => {
                level.feed(peak.take());
                if dt.is_zero() {
                    (active.drawn_step, active.drawn_level)
                } else {
                    quantise(level.advance(dt), q)
                }
            }
            _ => (0, 0.),
        };
        let level_changed = step != active.drawn_step;
        if time != pace.picture.time || level_changed {
            pace.picture.time = time;
            pace.picture.commit.increment();
        }
        active.drawn_step = step;
        active.drawn_level = drawn_level;
        if time.abs_diff(active.xray.time) >= XRAY_STEP || level_changed {
            active.xray.time = time;
            active.xray.commit.increment();
        }
    }

    /// After `output` rendered the frame for `target`: decides whether frames keep coming to
    /// move its picture. Returns whether the output should redraw on its next frame; a capped
    /// picture whose next move is further off arms the timer instead.
    pub fn synth_after_render(&mut self, output: &Output, target: Duration) -> bool {
        if self.synth.active.is_none() {
            return false;
        }
        let moves = self.synth_moves(output, get_monotonic_time());
        let refresh = self.synth_refresh_interval(output);
        let cap = self.synth.cap();
        let Some(active) = &self.synth.active else {
            return false;
        };
        let wake = {
            let mut outputs = active.outputs.borrow_mut();
            let pace = Active::pace(&mut outputs, &output.name(), &active.xray);
            pace.running = moves;
            if !moves {
                pace.anchor = None;
                return false;
            }
            match (cap, pace.moved_at) {
                (Some(fps), Some(at)) => {
                    let due = at + Duration::from_secs_f64(1. / fps);
                    if due <= target + refresh * 5 / 2 {
                        return true;
                    }
                    due.saturating_sub(refresh * 3 / 2)
                }
                _ => return true,
            }
        };
        self.synth_wake_at(wake);
        false
    }

    /// Arms the timer to queue a redraw of the running outputs at monotonic time `at`, unless
    /// it already fires sooner.
    fn synth_wake_at(&mut self, at: Duration) {
        if let Some((token, armed)) = self.synth.wake {
            if armed <= at {
                return;
            }
            self.event_loop.remove(token);
            self.synth.wake = None;
        }
        let timer = Timer::from_duration(at.saturating_sub(get_monotonic_time()));
        let token = self
            .event_loop
            .insert_source(timer, |_, _, state| {
                state.niri.synth.wake = None;
                state.niri.synth_wake();
                TimeoutAction::Drop
            })
            .unwrap();
        self.synth.wake = Some((token, at));
    }

    /// Queues a redraw of every output whose picture runs: a capped move is due.
    pub fn synth_wake(&mut self) {
        let Some(active) = &self.synth.active else {
            return;
        };
        let running: Vec<String> = active
            .outputs
            .borrow()
            .iter()
            .filter(|(_, p)| p.running)
            .map(|(n, _)| n.clone())
            .collect();
        let outputs: Vec<_> = self
            .output_state
            .keys()
            .filter(|o| running.contains(&o.name()))
            .cloned()
            .collect();
        for output in outputs {
            self.queue_redraw(&output);
        }
    }

    /// The power supplies changed: re-reads whether the machine runs on battery.
    pub fn synth_power_changed(&mut self) {
        if let Some(watch) = &mut self.synth.power {
            let was = watch.on_battery;
            watch.read();
            if watch.on_battery != was {
                debug!("synth background: on battery {}", watch.on_battery);
            }
        }
    }

    /// Opens and closes the sound capture and the power watch as needed, and starts the
    /// outputs whose picture should move but has no frames coming. Cheap: it returns at once
    /// while the background is off.
    pub fn synth_refresh(&mut self) {
        if !self.synth.is_allocated()
            && self.synth.wake.is_none()
            && self.synth.sound.is_none()
            && self.synth.power.is_none()
        {
            return;
        }
        let visible = self.synth_any_visible();
        let listen = self.synth.wants_sound(visible);
        if listen && self.synth.sound.is_none() {
            match open_sound(&self.event_loop, self.synth.peak.clone()) {
                Ok(sound) => self.synth.sound = Some(sound),
                Err(err) => {
                    warn!("synth background: not listening to sound: {err:?}");
                    self.synth.sound_failed = true;
                }
            }
        } else if !listen && self.synth.sound.is_some() {
            self.synth.sound = None;
            self.synth.peak.set(0.);
            self.synth.level = Level::new();
        }

        let power = self.synth.wants_power();
        if power && self.synth.power.is_none() {
            match power::Watch::open(&self.event_loop) {
                Ok(watch) => self.synth.power = Some(watch),
                Err(err) => {
                    warn!("synth background: not watching the power supply: {err:?}");
                    self.synth.power_failed = true;
                }
            }
        } else if !power {
            self.synth.power = None;
        }

        if self.synth.active.is_none() {
            if let Some((token, _)) = self.synth.wake.take() {
                self.event_loop.remove(token);
            }
            return;
        }
        let now = get_monotonic_time();
        let start: Vec<Output> = self
            .output_state
            .keys()
            .filter(|output| self.synth_moves(output, now))
            .cloned()
            .collect();
        for output in start {
            let Some(active) = &self.synth.active else {
                return;
            };
            let started = {
                let mut outputs = active.outputs.borrow_mut();
                let pace = Active::pace(&mut outputs, &output.name(), &active.xray);
                !std::mem::replace(&mut pace.running, true)
            };
            if started {
                self.queue_redraw(&output);
            }
        }
    }
}
const UNIFORMS: [(&str, UniformType); 19] = [
    ("synth_res", UniformType::_2f),
    ("synth_phase", UniformType::_4f),
    ("synth_scene", UniformType::_4f),
    ("synth_sky", UniformType::_3f),
    ("synth_glow", UniformType::_3f),
    ("synth_grid", UniformType::_3f),
    ("synth_sun_top", UniformType::_3f),
    ("synth_sun_bottom", UniformType::_3f),
    ("synth_sun", UniformType::_4f),
    ("synth_stripes", UniformType::_4f),
    ("synth_layers", UniformType::_1f),
    ("synth_octaves", UniformType::_4f),
    ("synth_octave4", UniformType::_1f),
    ("synth_layer", UniformType::Matrix4x4),
    ("synth_floor", UniformType::_4f),
    ("synth_lines", UniformType::_4f),
    ("synth_colour", UniformType::Matrix3x3),
    ("synth_post", UniformType::_4f),
    ("synth_lab_seed", UniformType::_1f),
];

fn compile(renderer: &mut GlesRenderer, src: &str) -> Result<ShaderProgram, String> {
    let names = UNIFORMS.map(|(name, ty)| UniformName::new(name, ty));
    ShaderProgram::compile(renderer, src, &names, &["synth_tables"]).map_err(|e| format!("{e:?}"))
}

/// Compiles the synth program on first use, so a session that never turns the background on
/// never compiles it.
fn ensure_program(renderer: &mut GlesRenderer) -> Option<()> {
    if Shaders::get(renderer).synth.borrow().is_some() {
        return Some(());
    }
    let _span = tracy_client::span!("synth::compile");
    match compile(renderer, newm_synth::FRAGMENT_SHADER) {
        Ok(program) => {
            Shaders::get(renderer).synth.replace(Some(program));
            Some(())
        }
        Err(err) => {
            warn!("error compiling the synth background shader: {err}");
            None
        }
    }
}

/// Uploads the seed's tables: 256 star rows and 4 ridge rows of RGBA, 266 KiB.
fn upload_tables(renderer: &mut GlesRenderer, seed: u32) -> anyhow::Result<GlesTexture> {
    let tables = Tables::new(seed);
    let size = (TABLE_WIDTH as i32, TABLE_HEIGHT as i32);
    Ok(renderer.import_memory(tables.rgba(), Fourcc::Abgr8888, size.into(), false)?)
}

fn uniforms(f: &Frame) -> Rc<[Uniform<'static>]> {
    let v3 = |c: [f32; 3]| UniformValue::_3f(c[0], c[1], c[2]);
    let v4 = |c: [f32; 4]| UniformValue::_4f(c[0], c[1], c[2], c[3]);
    Rc::new([
        Uniform::new("synth_res", f.res),
        Uniform::new("synth_phase", v4(f.phase)),
        Uniform::new("synth_scene", v4(f.scene)),
        Uniform::new("synth_sky", v3(f.sky)),
        Uniform::new("synth_glow", v3(f.glow)),
        Uniform::new("synth_grid", v3(f.grid)),
        Uniform::new("synth_sun_top", v3(f.sun_top)),
        Uniform::new("synth_sun_bottom", v3(f.sun_bottom)),
        Uniform::new("synth_sun", v4(f.sun)),
        Uniform::new("synth_stripes", v4(f.stripes)),
        Uniform::new("synth_layers", f.layers),
        Uniform::new("synth_octaves", v4(f.octaves)),
        Uniform::new("synth_octave4", f.octave4),
        Uniform::new(
            "synth_layer",
            UniformValue::Matrix4x4 {
                matrices: f.layer.to_vec(),
                transpose: false,
            },
        ),
        Uniform::new("synth_floor", v4(f.floor)),
        Uniform::new("synth_lines", v4(f.lines)),
        Uniform::new(
            "synth_colour",
            UniformValue::Matrix3x3 {
                matrices: vec![f.colour],
                transpose: false,
            },
        ),
        Uniform::new("synth_post", v4(f.post)),
        Uniform::new("synth_lab_seed", f.lab_seed),
    ])
}

/// The element that draws a frame over a whole output. It is opaque, so the solid backdrop
/// under it costs nothing.
fn element(
    frame: &Frame,
    texture: GlesTexture,
    id: Id,
    commit: CommitCounter,
    size: Size<i32, Physical>,
    scale: Scale<f64>,
) -> ShaderRenderElement {
    let logical: Size<f64, Logical> = size.to_f64().to_logical(scale);
    ShaderRenderElement::new(
        ProgramType::Synth,
        logical,
        Some(vec![Rectangle::from_size(logical)]),
        scale.x as f32,
        1.,
        uniforms(frame),
        HashMap::from([("synth_tables".to_owned(), texture)]),
        Kind::Unspecified,
    )
    .with_id(id, commit)
}

/// Opens the capture; in tests a stand-in, so no test talks to a real PipeWire.
fn open_sound(
    event_loop: &smithay::reexports::calloop::LoopHandle<'static, crate::niri::State>,
    peak: Rc<Cell<f32>>,
) -> anyhow::Result<SoundHandle> {
    #[cfg(test)]
    {
        let _ = (event_loop, peak);
        Ok(SoundHandle::Fake)
    }
    #[cfg(all(not(test), feature = "synth"))]
    {
        Ok(SoundHandle::PipeWire(sound::open(
            event_loop.clone(),
            peak,
        )?))
    }
    // Built with the background alone (synth-visual): nothing to listen with.
    #[cfg(all(not(test), not(feature = "synth")))]
    {
        let _ = (event_loop, peak);
        anyhow::bail!("built without sound: react-to-sound needs the synth feature")
    }
}

mod power;
#[cfg(feature = "synth")]
mod sound;
#[cfg(test)]
mod tests;
