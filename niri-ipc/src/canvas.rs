use std::collections::HashMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Edge of a tiled window on newm's tile canvas.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum CanvasEdge {
    /// The left edge.
    Left,
    /// The right edge.
    Right,
    /// The top edge.
    Top,
    /// The bottom edge.
    Bottom,
}

/// Side of newm's tile canvas.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum CanvasSide {
    /// The left side.
    Left,
    /// The right side.
    Right,
}

/// Direction on newm's tile canvas.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum CanvasDirection {
    /// Towards the left.
    Left,
    /// Towards the right.
    Right,
    /// Upwards.
    Up,
    /// Downwards.
    Down,
}

/// Whether floating windows on newm's tile canvas move with its camera.
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum FloatingFollow {
    /// Floating windows stay where they are on screen.
    #[default]
    Screen,
    /// A dialog moves and scales with the tiled window it belongs to; other floating windows
    /// stay where they are on screen.
    Parent,
    /// Every floating window moves and scales with the canvas.
    Canvas,
}

impl FloatingFollow {
    /// The mode after this one: screen, parent, canvas, then screen again.
    pub fn next(self) -> Self {
        match self {
            Self::Screen => Self::Parent,
            Self::Parent => Self::Canvas,
            Self::Canvas => Self::Screen,
        }
    }

    /// The name the config and `set-floating-follow` use.
    pub fn name(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Parent => "parent",
            Self::Canvas => "canvas",
        }
    }
}

impl FromStr for FloatingFollow {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "screen" => Ok(Self::Screen),
            "parent" => Ok(Self::Parent),
            "canvas" => Ok(Self::Canvas),
            _ => Err(r#"invalid floating follow, can be "screen", "parent" or "canvas""#),
        }
    }
}

impl FromStr for CanvasEdge {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            _ => Err(r#"invalid edge, can be "left", "right", "top" or "bottom""#),
        }
    }
}

impl FromStr for CanvasSide {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(r#"invalid side, can be "left" or "right""#),
        }
    }
}

impl FromStr for CanvasDirection {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            _ => Err(r#"invalid direction, can be "left", "right", "up" or "down""#),
        }
    }
}

/// One of newm's live settings, as [`Request::NewmOptions`](crate::Request::NewmOptions) lists
/// it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct NewmOption {
    /// The setting's path in the config file, such as `layout.canvas.zoom-step`.
    pub key: String,
    /// The kind of value: `bool`, `int`, `float`, `choice`, `color` or `size`.
    pub kind: String,
    /// The value in effect.
    pub value: serde_json::Value,
    /// The value the config file gives, or the default when the file gives none.
    pub config_value: serde_json::Value,
    /// Whether a live override replaces the config file's value.
    pub overridden: bool,
    /// The smallest value an `int` or `float` takes.
    pub min: Option<f64>,
    /// The largest value an `int` or `float` takes.
    pub max: Option<f64>,
    /// The step a control moves an `int` or `float` by.
    pub step: Option<f64>,
    /// The values a `choice` takes.
    pub choices: Option<Vec<String>>,
    /// The unit of an `int` or `float`, such as `px` or `ms`.
    pub unit: Option<String>,
    /// The group a settings app shows it in: `Canvas`, `Camera`, `Pointer`, `Touchpad`,
    /// `Keyboard`, `Cursor`, `Appearance` or `Animations`.
    pub category: String,
    /// A short sentence fragment that the control completes.
    pub label: String,
    /// One plain sentence on what the setting does.
    pub help: String,
}

/// Windows a client launches and places on the focused canvas as one block, as
/// [`Request::NewmPlaceBlock`](crate::Request::NewmPlaceBlock) carries them.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct NewmBlock {
    /// The block's windows, each with its command and its tiles in the block.
    pub windows: Vec<NewmBlockWindow>,
    /// Where the block goes.
    pub anchor: NewmBlockAnchor,
    /// The index in `windows` of the window that takes the focus once the block is complete.
    #[serde(default)]
    pub focus: Option<usize>,
    /// Whether the camera moves to show the block.
    #[serde(default)]
    pub fly: bool,
}

/// One window of a [`NewmBlock`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub struct NewmBlockWindow {
    /// The program and its arguments.
    pub command: Vec<String>,
    /// The directory the program starts in, when it exists.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment variables the program gets on top of the compositor's.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// The app ID the window maps with: the first new window with it takes the slot.
    pub app_id: String,
    /// The window's tiles relative to the block's origin: column, row, width and height.
    pub rect: (i32, i32, i32, i32),
}

/// Where a [`NewmBlock`] goes on the canvas.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum NewmBlockAnchor {
    /// The free spot nearest the middle of the view, one tile clear of every window.
    Nearest,
    /// Centred on the view.
    Camera,
    /// With its origin on this tile, or nowhere when a window holds one of its tiles.
    Tiles(i32, i32),
}
