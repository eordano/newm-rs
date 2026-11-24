//! newm's live settings: the options a settings app reads and overrides at runtime, in one
//! static registry ([`OPTIONS`]) that the compositors, the IPC docs and the tests derive from.
//!
//! An override lives in memory for the compositor's lifetime and is never written to disk. It
//! applies on top of the config file at every load ([`LiveOptions::loaded`]), so a reload keeps
//! it, and [`LiveOptions::export`] writes the overrides as config file nodes.
//!
//! Every key names a real config field that applies live. Left out: outputs, binds, window and
//! layer rules, workspaces, the environment and startup commands (they are not plain values or
//! apply only at start), the tile size (a change re-lays out every window), `one-workspace`
//! (it gathers windows) and `restore-session` (it acts only at start).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::str::FromStr;

use niri_ipc::{FloatingFollow, NewmOption};
use serde_json::Value;

use crate::input::{AccelProfile, ScrollFactor};
use crate::{Color, Config, FloatOrInt, PresetSize};

/// The kind of value a live setting takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `true` or `false`.
    Bool,
    /// A whole number between `min` and `max`.
    Int,
    /// A number between `min` and `max`.
    Float,
    /// One of `choices`, as a string.
    Choice,
    /// A CSS color string; the compositor answers `#rrggbbaa`.
    Color,
    /// A new window's length, one number between `min` and `max`: 0 is one tile, a value up to 1
    /// is a share of the canvas, and a larger one is pixels.
    Size,
}

impl Kind {
    /// The kind's name in [`NewmOption::kind`].
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::Choice => "choice",
            Kind::Color => "color",
            Kind::Size => "size",
        }
    }
}

/// How a bool writes as a config file node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node {
    /// Any other kind.
    Value,
    /// `name` or `name false`.
    Flag,
    /// `name` when on; off has no node, so the export says to delete it.
    Presence,
    /// A new window's size: `name {}`, `name { proportion p; }` or `name { fixed px; }`.
    Preset,
}

/// One live setting.
pub struct Spec {
    /// The path of the setting in the config file, nodes joined by dots.
    pub key: &'static str,
    pub kind: Kind,
    pub category: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<&'static str>,
    pub choices: &'static [&'static str],
    node: Node,
    get: fn(&Config) -> Value,
    put: fn(&mut Config, &Value),
}

/// The categories a setting belongs to, in the order a settings app shows them.
pub const CATEGORIES: [&str; 9] = [
    "Canvas",
    "Camera",
    "Pointer",
    "Touchpad",
    "Keyboard",
    "Cursor",
    "Appearance",
    "Animations",
    DISPLAYS,
];

/// The category of the settings each connected display brings: they are not in [`OPTIONS`],
/// since which exist depends on what is plugged in.
pub const DISPLAYS: &str = "Displays";

/// The scales a display's setting takes, and its step.
const SCALE_RANGE: (f64, f64) = (0.5, 4.);
const SCALE_STEP: f64 = 0.25;

/// A connected display, as the compositor reports it to the live options.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    /// The connector's name, such as `eDP-1`: what an `output "NAME"` node matches.
    pub name: String,
    /// What the display calls itself, such as `Nreal Air 2 Pro`; empty when it does not say.
    pub model: String,
    /// The scale it is drawn at now.
    pub scale: f64,
}

/// The key of a display's scale, `output.NAME.scale`.
fn scale_key(name: &str) -> String {
    format!("output.{name}.scale")
}

/// The display a scale key names.
fn scale_key_name(key: &str) -> Option<&str> {
    let name = key.strip_prefix("output.")?.strip_suffix(".scale")?;
    (!name.is_empty()).then_some(name)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_default()
}

fn i(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v.as_f64().map(|x| x as i64))
        .unwrap_or_default()
}

fn b(v: &Value) -> bool {
    v.as_bool().unwrap_or_default()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn color_value(c: Color) -> Value {
    let [r, g, b, a] = c
        .to_array_unpremul()
        .map(|x| (x.clamp(0., 1.) * 255.).round() as u8);
    Value::from(format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
}

fn color(v: &Value) -> Color {
    Color::from_str(s(v)).unwrap_or_default()
}

fn size_value(size: Option<PresetSize>) -> Value {
    match size {
        None => Value::from(0),
        Some(PresetSize::Proportion(p)) => Value::from(p),
        Some(PresetSize::Fixed(px)) => Value::from(px),
    }
}

/// 0 is one tile, a value up to 1 a share of the canvas, a larger one pixels.
fn size(v: &Value) -> Option<PresetSize> {
    match f(v) {
        x if x <= 0. => None,
        x if x <= 1. => Some(PresetSize::Proportion(x)),
        x => Some(PresetSize::Fixed(x.round() as i32)),
    }
}

fn accel_value(profile: Option<AccelProfile>) -> Value {
    Value::from(match profile {
        None => "default",
        Some(AccelProfile::Adaptive) => "adaptive",
        Some(AccelProfile::Flat) => "flat",
    })
}

fn accel(v: &Value) -> Option<AccelProfile> {
    match s(v) {
        "adaptive" => Some(AccelProfile::Adaptive),
        "flat" => Some(AccelProfile::Flat),
        _ => None,
    }
}

fn scroll_value(factor: Option<ScrollFactor>) -> Value {
    Value::from(factor.and_then(|x| x.base).map_or(1., |x| x.0))
}

fn put_scroll(factor: &mut Option<ScrollFactor>, v: &Value) {
    factor.get_or_insert_with(ScrollFactor::default).base = Some(FloatOrInt(f(v)));
}

fn follow_value(mode: FloatingFollow) -> Value {
    Value::from(mode.name())
}

fn follow(v: &Value) -> FloatingFollow {
    FloatingFollow::from_str(s(v)).unwrap_or_default()
}

const FOLLOW: &[&str] = &["screen", "parent", "canvas"];
const ACCEL: &[&str] = &["default", "adaptive", "flat"];

macro_rules! spec {
    (
        $key:literal, $kind:ident, $node:ident, $category:literal, $label:literal, $help:literal,
        [$min:expr, $max:expr, $step:expr, $unit:expr, $choices:expr],
        |$c:ident| $get:expr, |$cm:ident, $v:ident| $put:expr $(,)?
    ) => {
        Spec {
            key: $key,
            kind: Kind::$kind,
            category: $category,
            label: $label,
            help: $help,
            min: $min,
            max: $max,
            step: $step,
            unit: $unit,
            choices: $choices,
            node: Node::$node,
            get: |$c| Value::from($get),
            put: |$cm, $v| $put,
        }
    };
}

/// Every live setting, in the order a settings app lists them.
pub static OPTIONS: &[Spec] =
    &[
        spec!(
            "layout.canvas.center-on-switch",
            Bool,
            Flag,
            "Canvas",
            "Centre the window picked in the switcher",
            "Confirming a window in Alt+Tab or KRunner centres it on screen.",
            [None, None, None, None, &[]],
            |c| c.layout.canvas.center_on_switch,
            |c, v| c.layout.canvas.center_on_switch = b(v),
        ),
        spec!(
            "layout.canvas.super-tap-overview",
            Bool,
            Flag,
            "Canvas",
            "Super tap opens the overview",
            "Pressing and releasing Super alone opens or closes the overview.",
            [None, None, None, None, &[]],
            |c| c.layout.canvas.super_tap_overview,
            |c, v| c.layout.canvas.super_tap_overview = b(v),
        ),
        spec!(
        "layout.canvas.floating-follow", Choice, Value, "Canvas",
        "Floating windows follow",
        "Floating windows stay on screen, move with their parent window, or move and scale with \
         the canvas.",
        [None, None, None, None, FOLLOW],
        |c| follow_value(c.layout.canvas.floating_follow),
        |c, v| c.layout.canvas.floating_follow = follow(v),
    ),
        spec!(
        "layout.canvas.overview-dim", Float, Value, "Canvas",
        "Overview dims the area off screen by",
        "The overview darkens what lies outside the screen's area with a black overlay of this \
         opacity.",
        [Some(0.), Some(1.), Some(0.05), None, &[]],
        |c| c.layout.canvas.overview_dim, |c, v| c.layout.canvas.overview_dim = f(v),
    ),
        spec!(
        "layout.canvas.default-width", Size, Preset, "Canvas",
        "New windows open this wide",
        "A new tiled window takes this width when no window rule sets one, where 0 is one tile \
         and a value up to 1 is a share of the canvas's width.",
        [Some(0.), Some(7680.), Some(1.), Some("px"), &[]],
        |c| size_value(c.layout.canvas.default_width),
        |c, v| c.layout.canvas.default_width = size(v),
    ),
        spec!(
        "layout.canvas.default-height", Size, Preset, "Canvas",
        "New windows open this tall",
        "A new tiled window takes this height when no window rule sets one, where 0 is one tile \
         and a value up to 1 is a share of the canvas's height.",
        [Some(0.), Some(4320.), Some(1.), Some("px"), &[]],
        |c| size_value(c.layout.canvas.default_height),
        |c, v| c.layout.canvas.default_height = size(v),
    ),
        spec!(
        "layout.canvas.distance-weight", Float, Value, "Canvas",
        "Cost per tile a window moves to make room",
        "Higher values keep the windows that make room for a moved one closer to where they were.",
        [Some(0.), Some(100.), Some(0.05), None, &[]],
        |c| c.layout.canvas.distance_weight, |c, v| c.layout.canvas.distance_weight = f(v),
    ),
        spec!(
            "layout.canvas.diagonal-weight",
            Float,
            Value,
            "Canvas",
            "Cost of making room along both axes",
            "Higher values make the windows that make room move along their row or their column \
         only.",
            [Some(0.), Some(100.), Some(0.05), None, &[]],
            |c| c.layout.canvas.diagonal_weight,
            |c, v| c.layout.canvas.diagonal_weight = f(v),
        ),
        spec!(
        "layout.canvas.zoom-step", Float, Value, "Camera",
        "Scale change per zoom step",
        "One step out multiplies the camera's scale by this factor, and one step in divides by it.",
        [Some(0.05), Some(1.), Some(0.05), None, &[]],
        |c| c.layout.canvas.zoom_step, |c, v| c.layout.canvas.zoom_step = f(v),
    ),
        spec!(
            "layout.canvas.zoom-min",
            Int,
            Value,
            "Camera",
            "Closest zoom level",
            "Zooming in stops at this level, and negative levels magnify.",
            [Some(-16.), Some(16.), Some(1.), Some("steps"), &[]],
            |c| c.layout.canvas.zoom_min,
            |c, v| c.layout.canvas.zoom_min = i(v) as i32,
        ),
        spec!(
            "layout.canvas.zoom-max",
            Int,
            Value,
            "Camera",
            "Farthest zoom level",
            "Zooming out and zoom to fit stop at this level.",
            [Some(-16.), Some(16.), Some(1.), Some("steps"), &[]],
            |c| c.layout.canvas.zoom_max,
            |c, v| c.layout.canvas.zoom_max = i(v) as i32,
        ),
        spec!(
        "layout.canvas.magnified-input", Bool, Flag, "Camera",
        "Clicks reach apps while zoomed in",
        "While the camera magnifies, pointer input goes to the window under the pointer instead \
         of only picking it.",
        [None, None, None, None, &[]],
        |c| c.layout.canvas.magnified_input, |c, v| c.layout.canvas.magnified_input = b(v),
    ),
        spec!(
        "layout.canvas.pan-acceleration", Float, Value, "Camera",
        "Share of pointer acceleration a Mod pan takes",
        "At 0 the camera follows the hand's motion, and at 1 it follows the accelerated pointer.",
        [Some(0.), Some(1.), Some(0.05), None, &[]],
        |c| c.layout.canvas.pan_acceleration, |c, v| c.layout.canvas.pan_acceleration = f(v),
    ),
        spec!(
        "layout.canvas.pan-momentum", Float, Value, "Camera",
        "Pan glide after releasing Mod",
        "At 0 the camera stops where the hand stops, and at 1 it glides on like a touchpad swipe.",
        [Some(0.), Some(1.), Some(0.05), None, &[]],
        |c| c.layout.canvas.pan_momentum, |c, v| c.layout.canvas.pan_momentum = f(v),
    ),
        spec!(
            "input.mouse.natural-scroll",
            Bool,
            Presence,
            "Pointer",
            "Natural scrolling",
            "The wheel moves the content instead of the view.",
            [None, None, None, None, &[]],
            |c| c.input.mouse.natural_scroll,
            |c, v| c.input.mouse.natural_scroll = b(v),
        ),
        spec!(
            "input.mouse.accel-speed",
            Float,
            Value,
            "Pointer",
            "Pointer speed",
            "The pointer moves slower below 0 and faster above it.",
            [Some(-1.), Some(1.), Some(0.05), None, &[]],
            |c| c.input.mouse.accel_speed.0,
            |c, v| c.input.mouse.accel_speed = FloatOrInt(f(v)),
        ),
        spec!(
            "input.mouse.accel-profile",
            Choice,
            Value,
            "Pointer",
            "Acceleration profile",
            "Adaptive speeds the pointer up with the hand's speed, and flat keeps one ratio.",
            [None, None, None, None, ACCEL],
            |c| accel_value(c.input.mouse.accel_profile),
            |c, v| c.input.mouse.accel_profile = accel(v),
        ),
        spec!(
            "input.mouse.scroll-factor",
            Float,
            Value,
            "Pointer",
            "Scroll distance multiplier",
            "Every wheel scroll moves this many times as far.",
            [Some(0.), Some(10.), Some(0.1), None, &[]],
            |c| scroll_value(c.input.mouse.scroll_factor),
            |c, v| put_scroll(&mut c.input.mouse.scroll_factor, v),
        ),
        spec!(
            "input.touchpad.tap",
            Bool,
            Presence,
            "Touchpad",
            "Tap to click",
            "A tap on the touchpad clicks.",
            [None, None, None, None, &[]],
            |c| c.input.touchpad.tap,
            |c, v| c.input.touchpad.tap = b(v),
        ),
        spec!(
            "input.touchpad.natural-scroll",
            Bool,
            Presence,
            "Touchpad",
            "Natural scrolling",
            "The content moves with the fingers, as on a phone.",
            [None, None, None, None, &[]],
            |c| c.input.touchpad.natural_scroll,
            |c, v| c.input.touchpad.natural_scroll = b(v),
        ),
        spec!(
            "input.touchpad.accel-speed",
            Float,
            Value,
            "Touchpad",
            "Pointer speed",
            "The pointer moves slower below 0 and faster above it.",
            [Some(-1.), Some(1.), Some(0.05), None, &[]],
            |c| c.input.touchpad.accel_speed.0,
            |c, v| c.input.touchpad.accel_speed = FloatOrInt(f(v)),
        ),
        spec!(
            "input.touchpad.accel-profile",
            Choice,
            Value,
            "Touchpad",
            "Acceleration profile",
            "Adaptive speeds the pointer up with the hand's speed, and flat keeps one ratio.",
            [None, None, None, None, ACCEL],
            |c| accel_value(c.input.touchpad.accel_profile),
            |c, v| c.input.touchpad.accel_profile = accel(v),
        ),
        spec!(
            "input.touchpad.scroll-factor",
            Float,
            Value,
            "Touchpad",
            "Scroll distance multiplier",
            "Every two-finger scroll moves this many times as far.",
            [Some(0.), Some(10.), Some(0.1), None, &[]],
            |c| scroll_value(c.input.touchpad.scroll_factor),
            |c, v| put_scroll(&mut c.input.touchpad.scroll_factor, v),
        ),
        spec!(
            "input.keyboard.repeat-delay",
            Int,
            Value,
            "Keyboard",
            "Key repeat starts after",
            "A held key starts repeating after this delay.",
            [Some(50.), Some(2000.), Some(10.), Some("ms"), &[]],
            |c| c.input.keyboard.repeat_delay,
            |c, v| c.input.keyboard.repeat_delay = i(v) as u16,
        ),
        spec!(
            "input.keyboard.repeat-rate",
            Int,
            Value,
            "Keyboard",
            "Key repeats per second",
            "A held key repeats this many times per second, and 0 turns repeat off.",
            [Some(0.), Some(100.), Some(1.), Some("/s"), &[]],
            |c| c.input.keyboard.repeat_rate,
            |c, v| c.input.keyboard.repeat_rate = i(v) as u8,
        ),
        spec!(
            "cursor.xcursor-size",
            Int,
            Value,
            "Cursor",
            "Cursor size",
            "The cursor theme draws the cursor at this size.",
            [Some(8.), Some(128.), Some(1.), Some("px"), &[]],
            |c| c.cursor.xcursor_size,
            |c, v| c.cursor.xcursor_size = i(v) as u8,
        ),
        spec!(
            "cursor.hide-when-typing",
            Bool,
            Flag,
            "Cursor",
            "Hide the cursor while typing",
            "A key press hides the cursor until the pointer moves.",
            [None, None, None, None, &[]],
            |c| c.cursor.hide_when_typing,
            |c, v| c.cursor.hide_when_typing = b(v),
        ),
        spec!(
            "layout.gaps",
            Float,
            Value,
            "Appearance",
            "Gap between windows",
            "Windows keep this much space between each other and the screen's edges.",
            [Some(0.), Some(200.), Some(1.), Some("px"), &[]],
            |c| c.layout.gaps,
            |c, v| c.layout.gaps = f(v),
        ),
        spec!(
            "layout.focus-ring.width",
            Float,
            Value,
            "Appearance",
            "Focus ring width",
            "The ring around the focused window is this wide.",
            [Some(0.), Some(64.), Some(1.), Some("px"), &[]],
            |c| c.layout.focus_ring.width,
            |c, v| c.layout.focus_ring.width = f(v),
        ),
        spec!(
            "layout.focus-ring.active-color",
            Color,
            Value,
            "Appearance",
            "Focus ring color",
            "The ring around the focused window takes this color.",
            [None, None, None, None, &[]],
            |c| color_value(c.layout.focus_ring.active_color),
            |c, v| c.layout.focus_ring.active_color = color(v),
        ),
        spec!(
            "layout.border.width",
            Float,
            Value,
            "Appearance",
            "Border width",
            "Window borders are this wide.",
            [Some(0.), Some(64.), Some(1.), Some("px"), &[]],
            |c| c.layout.border.width,
            |c, v| c.layout.border.width = f(v),
        ),
        spec!(
            "layout.border.active-color",
            Color,
            Value,
            "Appearance",
            "Focused window's border color",
            "The focused window's border takes this color.",
            [None, None, None, None, &[]],
            |c| color_value(c.layout.border.active_color),
            |c, v| c.layout.border.active_color = color(v),
        ),
        spec!(
            "layout.border.inactive-color",
            Color,
            Value,
            "Appearance",
            "Other windows' border color",
            "The borders of the windows without focus take this color.",
            [None, None, None, None, &[]],
            |c| color_value(c.layout.border.inactive_color),
            |c, v| c.layout.border.inactive_color = color(v),
        ),
        spec!(
            "animations.off",
            Bool,
            Presence,
            "Animations",
            "Turn animations off",
            "Every animation jumps straight to its end.",
            [None, None, None, None, &[]],
            |c| c.animations.off,
            |c, v| c.animations.off = b(v),
        ),
        spec!(
            "animations.slowdown",
            Float,
            Value,
            "Animations",
            "Animation slowdown",
            "Every animation takes this many times as long.",
            [Some(0.1), Some(10.), Some(0.1), None, &[]],
            |c| c.animations.slowdown,
            |c, v| c.animations.slowdown = f(v),
        ),
    ];

/// The setting under `key`, among all of them.
pub fn spec(key: &str) -> Option<&'static Spec> {
    OPTIONS.iter().find(|spec| spec.key == key)
}

fn num(x: f64) -> String {
    format!("{x}")
}

impl Spec {
    /// Checks `value` against the setting's kind and bounds.
    pub fn check(&self, value: &Value) -> Result<(), String> {
        let bounds = |x: f64| {
            if let Some(min) = self.min.filter(|min| x < *min) {
                return Err(format!(
                    "value {} is below the minimum {}",
                    num(x),
                    num(min)
                ));
            }
            if let Some(max) = self.max.filter(|max| x > *max) {
                return Err(format!(
                    "value {} is above the maximum {}",
                    num(x),
                    num(max)
                ));
            }
            Ok(())
        };
        match self.kind {
            Kind::Bool if value.is_boolean() => Ok(()),
            Kind::Bool => Err(format!("{} takes true or false, not {value}", self.key)),
            Kind::Int => match value.as_f64() {
                Some(x) if x.fract() == 0. => bounds(x),
                _ => Err(format!("{} takes a whole number, not {value}", self.key)),
            },
            Kind::Float | Kind::Size => match value.as_f64() {
                Some(x) if x.is_finite() => bounds(x),
                _ => Err(format!("{} takes a number, not {value}", self.key)),
            },
            Kind::Choice => match value.as_str() {
                Some(x) if self.choices.contains(&x) => Ok(()),
                _ => Err(format!(
                    "{} takes one of {}, not {value}",
                    self.key,
                    self.choices.join(", ")
                )),
            },
            Kind::Color => match value.as_str().map(Color::from_str) {
                Some(Ok(_)) => Ok(()),
                _ => Err(format!(
                    "{} takes a CSS color string, not {value}",
                    self.key
                )),
            },
        }
    }

    /// The setting's value in `config`.
    pub fn get(&self, config: &Config) -> Value {
        (self.get)(config)
    }

    /// Sets the setting in `config` to `value`, which [`Spec::check`] accepted.
    pub fn put(&self, config: &mut Config, value: &Value) {
        (self.put)(config, value);
    }

    /// The config file line for `value`, or a comment when no node sets it.
    fn kdl_line(&self, value: &Value) -> String {
        let name = self.key.rsplit('.').next().unwrap_or(self.key);
        match (self.kind, self.node) {
            (Kind::Bool, Node::Flag) if b(value) => name.to_owned(),
            (Kind::Bool, Node::Flag) => format!("{name} false"),
            (Kind::Bool, _) if b(value) => name.to_owned(),
            (Kind::Bool, _) => format!("// {name}: off, so the config file has no \"{name}\" node"),
            (Kind::Int, _) => format!("{name} {}", i(value)),
            (Kind::Float, _) => format!("{name} {}", num(f(value))),
            (Kind::Choice, _) if s(value) == "default" => {
                format!("// {name}: the default, so the config file has no \"{name}\" node")
            }
            (Kind::Choice | Kind::Color, _) => format!("{name} {value}"),
            (Kind::Size, Node::Preset) => match size(value) {
                None => format!("{name} {{}}"),
                // A proportion is a float in KDL: `proportion 1` does not parse.
                Some(PresetSize::Proportion(p)) => format!("{name} {{ proportion {p:?}; }}"),
                Some(PresetSize::Fixed(px)) => format!("{name} {{ fixed {px}; }}"),
            },
            (Kind::Size, _) => format!("{name} {}", num(f(value))),
        }
    }
}

/// The live overrides of one compositor on top of its config file.
#[derive(Debug, Clone, Default)]
pub struct LiveOptions {
    /// The config file as last loaded, before the overrides.
    file: Config,
    /// The overrides by key, each checked and in the setting's canonical form.
    overrides: BTreeMap<&'static str, Value>,
    /// Keys this compositor does not apply live.
    skip: &'static [&'static str],
    /// The connected displays, as last reported (see [`LiveOptions::set_displays`]).
    displays: Vec<Display>,
    /// The scale each display had when it was last seen without an override: what the config
    /// file or, when the file names no scale for it, the compositor's own choice gives.
    own_scales: BTreeMap<String, f64>,
    /// The overridden scales by display name.
    scales: BTreeMap<String, f64>,
}

#[derive(Default)]
struct Block {
    name: &'static str,
    lines: Vec<String>,
    blocks: Vec<Block>,
}

impl Block {
    fn at(&mut self, path: &[&'static str]) -> &mut Block {
        let Some((first, rest)) = path.split_first() else {
            return self;
        };
        let idx = match self.blocks.iter().position(|b| b.name == *first) {
            Some(idx) => idx,
            None => {
                self.blocks.push(Block {
                    name: first,
                    ..Block::default()
                });
                self.blocks.len() - 1
            }
        };
        self.blocks[idx].at(rest)
    }

    fn render(&self, depth: usize, out: &mut String) {
        let pad = "    ".repeat(depth);
        for line in &self.lines {
            let _ = writeln!(out, "{pad}{line}");
        }
        for block in &self.blocks {
            let _ = writeln!(out, "{pad}{} {{", block.name);
            block.render(depth + 1, out);
            let _ = writeln!(out, "{pad}}}");
        }
    }
}

impl LiveOptions {
    /// Live options over `file`, the config file as loaded, without the settings in `skip`.
    pub fn new(file: Config, skip: &'static [&'static str]) -> Self {
        Self {
            file,
            overrides: BTreeMap::new(),
            skip,
            displays: Vec::new(),
            own_scales: BTreeMap::new(),
            scales: BTreeMap::new(),
        }
    }

    /// Takes the displays connected now. Each brings a scale setting to [`Self::options`]; a
    /// display that is unplugged keeps its override for when it comes back.
    pub fn set_displays(&mut self, displays: Vec<Display>) {
        for display in &displays {
            if !self.scales.contains_key(&display.name) {
                self.own_scales.insert(display.name.clone(), display.scale);
            }
        }
        self.displays = displays;
    }

    /// The scale the config file gives the display `name`, if it gives one.
    fn file_scale(&self, name: &str) -> Option<f64> {
        let outputs = self.file.outputs.0.iter();
        let mut named = outputs.filter(|o| o.name.eq_ignore_ascii_case(name));
        named.find_map(|o| o.scale.map(|scale| scale.0))
    }

    /// The config file as last loaded.
    pub fn file(&self) -> &Config {
        &self.file
    }

    /// The settings this compositor applies live.
    pub fn specs(&self) -> impl Iterator<Item = &'static Spec> + '_ {
        OPTIONS.iter().filter(|spec| !self.skip.contains(&spec.key))
    }

    fn spec(&self, key: &str) -> Result<&'static Spec, String> {
        self.specs()
            .find(|spec| spec.key == key)
            .ok_or_else(|| format!("unknown option {key:?}"))
    }

    /// Takes the config file as loaded again and answers it with the overrides applied.
    pub fn loaded(&mut self, file: Config) -> Config {
        self.file = file;
        self.effective()
    }

    /// The config file with the overrides applied.
    pub fn effective(&self) -> Config {
        let mut config = self.file.clone();
        self.apply(&mut config);
        config
    }

    /// Applies the overrides to `config`.
    pub fn apply(&self, config: &mut Config) {
        for spec in self.specs() {
            if let Some(value) = self.overrides.get(spec.key) {
                spec.put(config, value);
            }
        }
        let canvas = &mut config.layout.canvas;
        if canvas.zoom_min > canvas.zoom_max {
            std::mem::swap(&mut canvas.zoom_min, &mut canvas.zoom_max);
        }
        for (name, scale) in &self.scales {
            let outputs = &mut config.outputs.0;
            let scale = Some(FloatOrInt(*scale));
            match outputs
                .iter_mut()
                .find(|o| o.name.eq_ignore_ascii_case(name))
            {
                Some(output) => output.scale = scale,
                None => outputs.push(crate::Output {
                    name: name.clone(),
                    scale,
                    ..Default::default()
                }),
            }
        }
    }

    /// Overrides `key` with `value`, or says why not.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        if let Some(name) = scale_key_name(key) {
            let known = self.displays.iter().any(|d| d.name == name);
            if !known && !self.scales.contains_key(name) {
                return Err(format!("no display named {name:?} is connected"));
            }
            let scale = value
                .as_f64()
                .filter(|v| (SCALE_RANGE.0..=SCALE_RANGE.1).contains(v))
                .ok_or_else(|| {
                    format!(
                        "{key} takes a number from {} to {}",
                        num(SCALE_RANGE.0),
                        num(SCALE_RANGE.1)
                    )
                })?;
            self.scales.insert(name.to_owned(), scale);
            return Ok(());
        }
        let spec = self.spec(key)?;
        spec.check(&value)?;
        let mut config = self.effective();
        spec.put(&mut config, &value);
        let canvas = &config.layout.canvas;
        if canvas.zoom_min > canvas.zoom_max {
            return Err(format!(
                "zoom-min {} is above zoom-max {}",
                canvas.zoom_min, canvas.zoom_max
            ));
        }
        self.overrides.insert(spec.key, spec.get(&config));
        Ok(())
    }

    /// Drops the override of `key`.
    pub fn reset(&mut self, key: &str) -> Result<(), String> {
        if let Some(name) = scale_key_name(key) {
            self.scales.remove(name);
            return Ok(());
        }
        let spec = self.spec(key)?;
        self.overrides.remove(spec.key);
        Ok(())
    }

    /// Drops every override.
    pub fn reset_all(&mut self) {
        self.overrides.clear();
        self.scales.clear();
    }

    /// The overridden display scales by display name.
    pub fn display_scales(&self) -> &BTreeMap<String, f64> {
        &self.scales
    }

    /// The overrides by key.
    pub fn overrides(&self) -> &BTreeMap<&'static str, Value> {
        &self.overrides
    }

    /// Every setting this compositor applies live, with its values.
    pub fn options(&self) -> Vec<NewmOption> {
        let effective = self.effective();
        self.specs()
            .map(|spec| NewmOption {
                key: spec.key.to_owned(),
                kind: spec.kind.name().to_owned(),
                value: spec.get(&effective),
                config_value: spec.get(&self.file),
                overridden: self.overrides.contains_key(spec.key),
                min: spec.min,
                max: spec.max,
                step: spec.step,
                choices: (!spec.choices.is_empty())
                    .then(|| spec.choices.iter().map(|&c| c.to_owned()).collect()),
                unit: spec.unit.map(str::to_owned),
                category: spec.category.to_owned(),
                label: spec.label.to_owned(),
                help: spec.help.to_owned(),
            })
            .chain(
                self.displays
                    .iter()
                    .map(|display| self.scale_option(display)),
            )
            .collect()
    }

    /// The scale setting of a connected display.
    fn scale_option(&self, display: &Display) -> NewmOption {
        let name = &display.name;
        let overridden = self.scales.get(name).copied();
        let own = self
            .file_scale(name)
            .or_else(|| self.own_scales.get(name).copied())
            .unwrap_or(display.scale);
        let called = if display.model.is_empty() {
            name.clone()
        } else {
            format!("{} ({name})", display.model)
        };
        NewmOption {
            key: scale_key(name),
            kind: Kind::Float.name().to_owned(),
            value: Value::from(overridden.unwrap_or(display.scale)),
            config_value: Value::from(own),
            overridden: overridden.is_some(),
            min: Some(SCALE_RANGE.0),
            max: Some(SCALE_RANGE.1),
            step: Some(SCALE_STEP),
            choices: None,
            unit: Some(String::from("x")),
            category: DISPLAYS.to_owned(),
            label: format!("Scale of {called}"),
            help: format!(
                "How large everything is drawn on {called}: a larger scale gives bigger text and \
                 less room, a smaller one more room. The room decides how many tiles fit, and a \
                 window never grows past its screen."
            ),
        }
    }

    /// The overrides as config file nodes, nested as in the file, after a comment naming
    /// `build`.
    pub fn export(&self, build: &str) -> String {
        let mut root = Block::default();
        for spec in self.specs() {
            let Some(value) = self.overrides.get(spec.key) else {
                continue;
            };
            let path: Vec<&'static str> = spec.key.split('.').collect();
            root.at(&path[..path.len() - 1])
                .lines
                .push(spec.kdl_line(value));
        }
        let mut out = format!("// Live settings exported from {build}.\n");
        root.render(0, &mut out);
        for (name, scale) in &self.scales {
            let _ = writeln!(out, "output {name:?} {{\n    scale {}\n}}", num(*scale));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid value of `spec` other than the default config's.
    fn other_value(spec: &Spec, config: &Config) -> Value {
        let current = spec.get(config);
        match spec.kind {
            Kind::Bool => Value::from(true),
            Kind::Int | Kind::Float | Kind::Size => {
                let (min, max) = (spec.min.unwrap(), spec.max.unwrap());
                let x = f(&current);
                let to = if x + spec.step.unwrap() <= max {
                    x + spec.step.unwrap()
                } else {
                    min
                };
                let to = if spec.key == "layout.canvas.zoom-min" {
                    x - 1.
                } else {
                    to
                };
                if spec.kind == Kind::Int {
                    Value::from(to as i64)
                } else {
                    Value::from(to)
                }
            }
            Kind::Choice => Value::from(
                *spec
                    .choices
                    .iter()
                    .find(|c| current.as_str() != Some(**c) && **c != "default")
                    .unwrap(),
            ),
            Kind::Color => Value::from("#12345678"),
        }
    }

    #[test]
    fn every_option_is_described_and_takes_its_own_value() {
        let config = Config::default();
        let mut seen = std::collections::HashSet::new();
        for spec in OPTIONS {
            assert!(seen.insert(spec.key), "{} twice", spec.key);
            assert!(CATEGORIES.contains(&spec.category), "{}", spec.key);
            assert!(
                !spec.label.is_empty() && !spec.label.ends_with('.'),
                "{}",
                spec.key
            );
            assert!(spec.help.ends_with('.'), "{}", spec.key);
            assert!(!spec.help.contains("  "), "{}", spec.key);
            let numeric = matches!(spec.kind, Kind::Int | Kind::Float | Kind::Size);
            assert_eq!(
                numeric,
                spec.min.is_some() && spec.max.is_some(),
                "{}",
                spec.key
            );
            assert_eq!(
                spec.kind == Kind::Choice,
                !spec.choices.is_empty(),
                "{}",
                spec.key
            );
            let mut changed = config.clone();
            let value = other_value(spec, &config);
            spec.check(&value).unwrap();
            spec.put(&mut changed, &value);
            assert_eq!(spec.get(&changed), value, "{}", spec.key);
            assert_ne!(changed, config, "{} changes no field", spec.key);
        }
    }

    #[test]
    fn set_checks_the_value_and_reset_brings_the_file_back() {
        let file = Config::parse_mem("layout {\n    canvas {\n        pan-momentum 0.5\n    }\n}")
            .unwrap();
        let mut live = LiveOptions::new(file, &[]);
        let key = "layout.canvas.pan-momentum";
        assert_eq!(
            live.set(key, Value::from(3)).unwrap_err(),
            "value 3 is above the maximum 1"
        );
        assert_eq!(
            live.set(key, Value::from(-0.5)).unwrap_err(),
            "value -0.5 is below the minimum 0"
        );
        assert!(live.set(key, Value::from(true)).is_err());
        assert!(live.set("layout.nope", Value::from(1)).is_err());
        live.set(key, Value::from(0.25)).unwrap();
        assert_eq!(live.effective().layout.canvas.pan_momentum, 0.25);
        let option = live.options().into_iter().find(|o| o.key == key).unwrap();
        assert!(option.overridden);
        assert_eq!(option.value, Value::from(0.25));
        assert_eq!(option.config_value, Value::from(0.5));
        assert_eq!(option.kind, "float");

        let reloaded = live.loaded(Config::parse_mem("layout {\n    gaps 3\n}").unwrap());
        assert_eq!(
            reloaded.layout.canvas.pan_momentum, 0.25,
            "a reload keeps the override"
        );
        assert_eq!(reloaded.layout.gaps, 3.);

        live.reset(key).unwrap();
        assert_eq!(live.effective().layout.canvas.pan_momentum, 1.);
        assert!(live.overrides().is_empty());
    }

    #[test]
    fn values_come_back_in_canonical_form_and_bounds_hold() {
        let mut live = LiveOptions::new(Config::default(), &[]);
        live.set("layout.border.active-color", Value::from("red"))
            .unwrap();
        live.set("input.keyboard.repeat-rate", Value::from(30.0))
            .unwrap();
        assert_eq!(
            live.overrides()["layout.border.active-color"],
            Value::from("#ff0000ff")
        );
        assert_eq!(
            live.overrides()["input.keyboard.repeat-rate"],
            Value::from(30)
        );
        assert!(live
            .set("input.keyboard.repeat-rate", Value::from(2.5))
            .is_err());
        assert!(live
            .set(
                "layout.canvas.default-width",
                serde_json::json!({ "proportion": 0.5 })
            )
            .is_err());
        live.set("layout.canvas.default-width", Value::from(960.4))
            .unwrap();
        assert_eq!(
            live.overrides()["layout.canvas.default-width"],
            Value::from(960)
        );
        live.set("layout.canvas.default-height", Value::from(0.5))
            .unwrap();
        assert_eq!(
            live.effective().layout.canvas.default_height,
            Some(PresetSize::Proportion(0.5))
        );
        live.set("layout.canvas.zoom-max", Value::from(-3))
            .unwrap_err();
        live.set("layout.canvas.floating-follow", Value::from("diagonal"))
            .unwrap_err();
        let skipping = LiveOptions::new(Config::default(), &["animations.off"]);
        assert!(skipping.options().iter().all(|o| o.key != "animations.off"));
    }

    fn display(name: &str, model: &str, scale: f64) -> Display {
        Display {
            name: name.to_owned(),
            model: model.to_owned(),
            scale,
        }
    }

    fn scale_of<'a>(options: &'a [NewmOption], name: &str) -> &'a NewmOption {
        let key = scale_key(name);
        let found = options.iter().find(|o| o.key == key);
        found.unwrap_or_else(|| panic!("no option {key}"))
    }

    #[test]
    fn every_connected_display_brings_its_scale() {
        let file = Config::parse_mem("output \"eDP-1\" { scale 2; }").unwrap();
        let mut live = LiveOptions::new(file, &[]);
        assert!(live.options().iter().all(|o| o.category != DISPLAYS));
        live.set_displays(vec![
            display("eDP-1", "NE135A1M-NY1", 2.),
            display("DP-2", "Air 2 Pro", 2.25),
        ]);
        let options = live.options();
        let panel = scale_of(&options, "eDP-1");
        assert_eq!(
            (&panel.value, &panel.config_value),
            (&Value::from(2.), &Value::from(2.))
        );
        assert!(!panel.overridden);
        let glasses = scale_of(&options, "DP-2");
        assert_eq!(glasses.kind, "float");
        assert_eq!(glasses.category, DISPLAYS);
        assert_eq!(glasses.label, "Scale of Air 2 Pro (DP-2)");
        assert_eq!(
            glasses.value,
            Value::from(2.25),
            "the scale the compositor chose"
        );
        assert_eq!(glasses.config_value, Value::from(2.25));
        assert_eq!(
            (glasses.min, glasses.max, glasses.step),
            (Some(0.5), Some(4.), Some(0.25))
        );
        assert_eq!(
            options.last().unwrap().key,
            "output.DP-2.scale",
            "after the fixed ones"
        );
    }

    #[test]
    fn a_display_scale_overrides_the_file_and_the_compositors_choice() {
        let file = Config::parse_mem("output \"eDP-1\" { scale 2; }").unwrap();
        let mut live = LiveOptions::new(file, &[]);
        live.set_displays(vec![display("eDP-1", "", 2.), display("DP-2", "", 2.25)]);

        live.set("output.DP-2.scale", Value::from(1.25)).unwrap();
        live.set("output.eDP-1.scale", Value::from(1.5)).unwrap();
        let config = live.effective();
        let scale = |name: &str| {
            let output = config.outputs.0.iter().find(|o| o.name == name).unwrap();
            output.scale.unwrap().0
        };
        assert_eq!(scale("DP-2"), 1.25, "an output node is added for it");
        assert_eq!(scale("eDP-1"), 1.5, "the file's node takes the override");
        assert_eq!(config.outputs.0.len(), 2);

        // The compositor reports the new scales; the own ones stay what they were.
        live.set_displays(vec![display("eDP-1", "", 1.5), display("DP-2", "", 1.25)]);
        let options = live.options();
        let glasses = scale_of(&options, "DP-2");
        assert_eq!(glasses.value, Value::from(1.25));
        assert_eq!(
            glasses.config_value,
            Value::from(2.25),
            "what it chose by itself"
        );
        assert!(glasses.overridden);
        assert_eq!(scale_of(&options, "eDP-1").config_value, Value::from(2.));

        live.reset("output.DP-2.scale").unwrap();
        assert!(live.effective().outputs.0.iter().all(|o| o.name != "DP-2"));
        live.reset_all();
        assert!(live.display_scales().is_empty());
    }

    #[test]
    fn display_scales_are_checked_kept_while_unplugged_and_exported() {
        let mut live = LiveOptions::new(Config::default(), &[]);
        live.set_displays(vec![display("DP-2", "", 2.25)]);
        assert!(
            live.set("output.DP-9.scale", Value::from(1.)).is_err(),
            "not connected"
        );
        assert!(live.set("output.DP-2.scale", Value::from(0.25)).is_err());
        assert!(live.set("output.DP-2.scale", Value::from(5)).is_err());
        assert!(live.set("output.DP-2.scale", Value::from("big")).is_err());
        assert!(live.set("output..scale", Value::from(1.)).is_err());
        live.set("output.DP-2.scale", Value::from(1)).unwrap();

        live.set_displays(Vec::new());
        assert!(
            live.options().iter().all(|o| o.category != DISPLAYS),
            "unplugged"
        );
        assert_eq!(
            live.display_scales().get("DP-2"),
            Some(&1.),
            "kept for its return"
        );
        live.set("output.DP-2.scale", Value::from(1.5)).unwrap();

        let kdl = live.export("newm-rs test");
        let parsed = Config::parse_mem(&kdl).unwrap_or_else(|e| panic!("{kdl}\n{e:?}"));
        let output = parsed.outputs.0.iter().find(|o| o.name == "DP-2").unwrap();
        assert_eq!(output.scale.unwrap().0, 1.5, "{kdl}");
    }

    #[test]
    fn the_export_parses_back_to_the_overrides() {
        let mut live = LiveOptions::new(Config::default(), &[]);
        let config = Config::default();
        for spec in OPTIONS {
            live.set(spec.key, other_value(spec, &config)).unwrap();
        }
        let kdl = live.export("newm-rs test");
        assert!(kdl.starts_with("// Live settings exported from newm-rs test.\n"));
        let parsed = Config::parse_mem(&kdl).unwrap_or_else(|e| panic!("{kdl}\n{e:?}"));
        for spec in OPTIONS {
            assert_eq!(
                spec.get(&parsed),
                live.overrides()[spec.key],
                "{} in\n{kdl}",
                spec.key
            );
        }

        let mut off = LiveOptions::new(config, &[]);
        off.set("input.touchpad.tap", Value::from(false)).unwrap();
        off.set("input.mouse.accel-profile", Value::from("default"))
            .unwrap();
        off.set("layout.canvas.default-height", Value::from(0))
            .unwrap();
        off.set("layout.canvas.super-tap-overview", Value::from(false))
            .unwrap();
        let kdl = off.export("newm-rs test");
        let parsed = Config::parse_mem(&kdl).unwrap_or_else(|e| panic!("{kdl}\n{e:?}"));
        assert!(!parsed.input.touchpad.tap);
        assert_eq!(parsed.input.mouse.accel_profile, None);
        assert_eq!(parsed.layout.canvas.default_height, None);
        assert!(kdl.contains("super-tap-overview false"), "{kdl}");
    }
}
