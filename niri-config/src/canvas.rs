use niri_ipc::FloatingFollow;

use crate::utils::{Flag, MergeWith, RegexEq};
use crate::{DefaultPresetSize, FloatOrInt, PresetSize};

/// newm's tile canvas: `layout { canvas { ... } }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    pub tile_width: f64,
    pub tile_height: f64,
    pub zoom_step: f64,
    pub zoom_min: i32,
    pub zoom_max: i32,
    /// What a window that makes room for a moved one pays, per tile of its area, for each tile
    /// it moves: `distance-weight 0.5`. Higher keeps the windows that make room closer.
    pub distance_weight: f64,
    /// What a window that makes room pays, per tile of its area, when it moves along both its
    /// row and its column: `diagonal-weight 1`.
    pub diagonal_weight: f64,
    pub super_tap_overview: bool,
    /// Whether confirming a window in the Alt+Tab switcher centres it on screen.
    pub center_on_switch: bool,
    /// How dark the overview draws what lies outside the area the screen shows: the alpha of a
    /// black overlay, from 0 (none) to 1.
    pub overview_dim: f64,
    /// How much of the pointer's acceleration a pan with the mod key held takes, from 0 (the
    /// camera follows the hand's motion, however fast) to 1 (it follows the accelerated
    /// pointer, as the cursor does).
    pub pan_acceleration: f64,
    /// How much of its momentum a pan with the mod key held keeps when the mod key is released,
    /// from 0 (the camera stops where the hand stopped) to 1 (it glides on as a touchpad swipe
    /// does). Touchpad swipes always glide.
    pub pan_momentum: f64,
    /// The width a new tiled window opens at when no window rule sets its
    /// `default-column-width`: `default-width { fixed 960; }` or `{ proportion 0.5; }`. Unset,
    /// one tile.
    ///
    /// Like a rule's size, it becomes the nearest whole number of tiles at 1:1, at least one; a
    /// proportion is of the canvas's width at 1:1 (the working area less the gaps). The window
    /// still never opens smaller than the tiles its minimum size needs, nor wider than the
    /// tiles that fit on the screen at 1:1. Floating windows keep niri's sizing.
    pub default_width: Option<PresetSize>,
    /// The height a new tiled window opens at when no window rule sets its
    /// `default-window-height`, as [`Self::default_width`] does the width:
    /// `default-height { proportion 0.6667; }`. Unset, one tile.
    pub default_height: Option<PresetSize>,
    /// Whether newm-rs saves the open windows and, at every start, relaunches their apps and puts
    /// each window back where it was. On unless `restore-session false`.
    pub restore_session: bool,
    /// Apps never relaunched nor put back by a restored session, by app ID:
    /// `restore-exclude app-id="^mpv$"`, once per pattern.
    pub restore_exclude: Vec<RegexEq>,
    /// Whether floating windows move with the camera as the canvas pans and zooms:
    /// `floating-follow "screen"` (they stay on screen), `"parent"` (a dialog moves and scales
    /// with its tiled parent) or `"canvas"` (every floating window does with the canvas). The
    /// `cycle-floating-follow` and `set-floating-follow` actions change it until the config
    /// changes this setting.
    pub floating_follow: FloatingFollow,
    /// Whether every output keeps exactly one workspace: `one-workspace`. Nothing then switches
    /// to, creates or moves anything to another workspace, named workspaces and window rules'
    /// `open-on-workspace` are ignored, and turning it on gathers the windows of an output's
    /// other workspaces onto its active one. Off unless set.
    pub one_workspace: bool,
    /// Whether pointer input reaches the app under the pointer while the camera magnifies
    /// (zoomed in past 1:1): `magnified-input`. A click then lands where the window is drawn.
    /// Zoomed out, a click only picks a window. Off unless set: a magnified click only picks.
    pub magnified_input: bool,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            tile_width: 720.,
            tile_height: 465.,
            zoom_step: 0.8,
            zoom_min: -2,
            zoom_max: 4,
            distance_weight: 0.5,
            diagonal_weight: 1.0,
            super_tap_overview: false,
            center_on_switch: false,
            overview_dim: 0.4,
            pan_acceleration: 1.,
            pan_momentum: 1.,
            default_width: None,
            default_height: None,
            restore_session: true,
            restore_exclude: Vec::new(),
            floating_follow: FloatingFollow::Screen,
            one_workspace: false,
            magnified_input: false,
        }
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct RestoreExclude {
    #[knuffel(property, str)]
    pub app_id: RegexEq,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct CanvasPart {
    #[knuffel(child, unwrap(argument))]
    pub tile_width: Option<FloatOrInt<1, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub tile_height: Option<FloatOrInt<1, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub zoom_step: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub zoom_min: Option<i32>,
    #[knuffel(child, unwrap(argument))]
    pub zoom_max: Option<i32>,
    #[knuffel(child, unwrap(argument))]
    pub distance_weight: Option<FloatOrInt<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub diagonal_weight: Option<FloatOrInt<0, 100>>,
    #[knuffel(child)]
    pub super_tap_overview: Option<Flag>,
    #[knuffel(child)]
    pub center_on_switch: Option<Flag>,
    #[knuffel(child, unwrap(argument))]
    pub overview_dim: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub pan_acceleration: Option<FloatOrInt<0, 1>>,
    #[knuffel(child, unwrap(argument))]
    pub pan_momentum: Option<FloatOrInt<0, 1>>,
    #[knuffel(child)]
    pub default_width: Option<DefaultPresetSize>,
    #[knuffel(child)]
    pub default_height: Option<DefaultPresetSize>,
    #[knuffel(child)]
    pub restore_session: Option<Flag>,
    #[knuffel(children(name = "restore-exclude"))]
    pub restore_exclude: Vec<RestoreExclude>,
    #[knuffel(child, unwrap(argument, str))]
    pub floating_follow: Option<FloatingFollow>,
    #[knuffel(child)]
    pub one_workspace: Option<Flag>,
    #[knuffel(child)]
    pub magnified_input: Option<Flag>,
}

impl MergeWith<CanvasPart> for Canvas {
    fn merge_with(&mut self, part: &CanvasPart) {
        if let Some(x) = part.tile_width {
            self.tile_width = x.0;
        }
        if let Some(x) = part.tile_height {
            self.tile_height = x.0;
        }
        if let Some(x) = part.zoom_step {
            self.zoom_step = x.0.clamp(0.05, 1.);
        }
        if let Some(x) = part.zoom_min {
            self.zoom_min = x.clamp(-16, 16);
        }
        if let Some(x) = part.zoom_max {
            self.zoom_max = x.clamp(-16, 16);
        }
        if self.zoom_min > self.zoom_max {
            std::mem::swap(&mut self.zoom_min, &mut self.zoom_max);
        }
        if let Some(x) = part.distance_weight {
            self.distance_weight = x.0;
        }
        if let Some(x) = part.diagonal_weight {
            self.diagonal_weight = x.0;
        }
        if let Some(x) = part.super_tap_overview {
            self.super_tap_overview = x.0;
        }
        if let Some(x) = part.center_on_switch {
            self.center_on_switch = x.0;
        }
        if let Some(x) = part.overview_dim {
            self.overview_dim = x.0;
        }
        if let Some(x) = part.pan_acceleration {
            self.pan_acceleration = x.0;
        }
        if let Some(x) = part.pan_momentum {
            self.pan_momentum = x.0;
        }
        if let Some(x) = part.default_width {
            self.default_width = x.0;
        }
        if let Some(x) = part.default_height {
            self.default_height = x.0;
        }
        if let Some(x) = part.restore_session {
            self.restore_session = x.0;
        }
        if let Some(x) = part.floating_follow {
            self.floating_follow = x;
        }
        if let Some(x) = part.one_workspace {
            self.one_workspace = x.0;
        }
        if let Some(x) = part.magnified_input {
            self.magnified_input = x.0;
        }
        self.restore_exclude
            .extend(part.restore_exclude.iter().map(|x| x.app_id.clone()));
    }
}

#[cfg(test)]
mod tests {
    use niri_ipc::{CanvasDirection, CanvasEdge, CanvasSide};

    use super::*;
    use crate::{Action, Config};

    #[test]
    fn parse_canvas() {
        let config = Config::parse_mem(
            r#"
            layout {
                canvas {
                    tile-width 600
                    tile-height 400
                    zoom-step 0.75
                    zoom-min -1
                    zoom-max 7
                    distance-weight 0.25
                    diagonal-weight 2
                    super-tap-overview
                    center-on-switch
                    overview-dim 0.25
                    default-width { fixed 960; }
                    default-height { proportion 0.6667; }
                    floating-follow "parent"
                    one-workspace
                    magnified-input
                }
            }
            binds {
                Mod+Shift+Q { resize-edge "left" 1; }
                Mod+Shift+E { resize-edge "right" -1; }
                Mod+N { zoom-in; }
                Mod+U { zoom-out; }
                Mod+0 { zoom-reset; }
                Mod+Z { undo; }
                Mod+Shift+Z { redo; }
                Mod+I { gather-islands; }
                Mod+Shift+A { send-to-edge "left"; }
                Mod+Ctrl+K { swap-neighbour "up"; }
                Mod+Q { quit-app; }
                Mod+F { cycle-floating-follow; }
                Mod+Shift+F { set-floating-follow "canvas"; }
            }
            "#,
        )
        .unwrap();
        assert_eq!(
            config.layout.canvas,
            Canvas {
                tile_width: 600.,
                tile_height: 400.,
                zoom_step: 0.75,
                zoom_min: -1,
                zoom_max: 7,
                distance_weight: 0.25,
                diagonal_weight: 2.0,
                super_tap_overview: true,
                center_on_switch: true,
                overview_dim: 0.25,
                pan_acceleration: 1.,
                pan_momentum: 1.,
                default_width: Some(PresetSize::Fixed(960)),
                default_height: Some(PresetSize::Proportion(0.6667)),
                restore_session: true,
                restore_exclude: Vec::new(),
                floating_follow: FloatingFollow::Parent,
                one_workspace: true,
                magnified_input: true,
            }
        );
        let actions: Vec<Action> = config
            .binds
            .0
            .iter()
            .filter_map(|bind| bind.press_action().cloned())
            .collect();
        assert_eq!(
            actions,
            [
                Action::ResizeEdge(CanvasEdge::Left, 1),
                Action::ResizeEdge(CanvasEdge::Right, -1),
                Action::ZoomIn,
                Action::ZoomOut,
                Action::ZoomReset,
                Action::Undo,
                Action::Redo,
                Action::GatherIslands,
                Action::SendToEdge(CanvasSide::Left),
                Action::SwapNeighbour(CanvasDirection::Up),
                Action::QuitApp,
                Action::CycleFloatingFollow,
                Action::SetFloatingFollow(FloatingFollow::Canvas),
            ]
        );
    }

    #[test]
    fn canvas_defaults() {
        let config = Config::parse_mem("").unwrap();
        assert_eq!(config.layout.canvas, Canvas::default());
        assert_eq!(config.layout.canvas.overview_dim, 0.4);
        assert!(!config.layout.canvas.center_on_switch);
        assert_eq!(config.layout.canvas.default_width, None);
        assert_eq!(config.layout.canvas.default_height, None);
        assert!(config.layout.canvas.restore_session);
        assert!(config.layout.canvas.restore_exclude.is_empty());
        assert_eq!(config.layout.canvas.floating_follow, FloatingFollow::Screen);
        assert!(!config.layout.canvas.one_workspace);
    }

    #[test]
    fn canvas_one_workspace_is_off_unless_set() {
        let parse = |body: &str| {
            Config::parse_mem(&format!("layout {{\n canvas {{\n{body}\n }}\n }}"))
                .map(|config| config.layout.canvas.one_workspace)
        };
        assert!(!parse("").unwrap());
        assert!(parse("one-workspace").unwrap());
        assert!(parse("one-workspace true").unwrap());
        assert!(!parse("one-workspace false").unwrap());
        assert!(parse("one-workspace 1").is_err());
    }

    #[test]
    fn canvas_floating_follow_is_one_of_three_modes() {
        let parse = |body: &str| {
            Config::parse_mem(&format!("layout {{\n canvas {{\n{body}\n }}\n }}"))
                .map(|config| config.layout.canvas.floating_follow)
        };
        assert_eq!(parse("").unwrap(), FloatingFollow::Screen);
        assert_eq!(
            parse("floating-follow \"screen\"").unwrap(),
            FloatingFollow::Screen
        );
        assert_eq!(
            parse("floating-follow \"parent\"").unwrap(),
            FloatingFollow::Parent
        );
        assert_eq!(
            parse("floating-follow \"canvas\"").unwrap(),
            FloatingFollow::Canvas
        );
        assert!(parse("floating-follow \"window\"").is_err());
        assert!(parse("floating-follow").is_err());
        let mut mode = FloatingFollow::Screen;
        let mut seen = Vec::new();
        for _ in 0..3 {
            mode = mode.next();
            seen.push(mode.name());
        }
        assert_eq!(seen, ["parent", "canvas", "screen"]);
    }

    #[test]
    fn canvas_restore_session_is_on_unless_turned_off() {
        let parse = |body: &str| {
            Config::parse_mem(&format!("layout {{\n canvas {{\n{body}\n }}\n }}"))
                .map(|config| config.layout.canvas)
        };
        assert!(parse("").unwrap().restore_session);
        assert!(parse("restore-session").unwrap().restore_session);
        assert!(parse("restore-session true").unwrap().restore_session);
        assert!(!parse("restore-session false").unwrap().restore_session);
        let canvas =
            parse("restore-exclude app-id=\"^mpv$\"\nrestore-exclude app-id=\"(?i)steam\"")
                .unwrap();
        let patterns: Vec<&str> = canvas
            .restore_exclude
            .iter()
            .map(|r| r.0.as_str())
            .collect();
        assert_eq!(patterns, ["^mpv$", "(?i)steam"]);
        assert!(canvas.restore_exclude[1].0.is_match("Steam"));
        assert!(parse("restore-exclude").is_err(), "a pattern is required");
        assert!(
            parse("restore-exclude app-id=\"(\"").is_err(),
            "an invalid regex"
        );
    }

    #[test]
    fn canvas_default_size_is_a_preset_size_per_axis() {
        let parse = |body: &str| {
            Config::parse_mem(&format!("layout {{\n canvas {{\n{body}\n }}\n }}"))
                .map(|config| config.layout.canvas)
        };
        let canvas = parse("default-height { fixed 700; }").unwrap();
        assert_eq!(canvas.default_width, None, "each axis is set on its own");
        assert_eq!(canvas.default_height, Some(PresetSize::Fixed(700)));
        let canvas = parse("default-width { proportion 0.5; }").unwrap();
        assert_eq!(canvas.default_width, Some(PresetSize::Proportion(0.5)));
        assert_eq!(canvas.default_height, None);
        let canvas = parse("default-width {}").unwrap();
        assert_eq!(canvas.default_width, None, "an empty node keeps one tile");
        assert!(parse("default-width { fixed 960; proportion 0.5; }").is_err());
        assert!(parse("default-height 700").is_err());
    }

    #[test]
    fn canvas_pan_acceleration_is_a_share() {
        let parse = |a: &str| {
            Config::parse_mem(&format!(
                "layout {{\n canvas {{\n pan-acceleration {a}\n }}\n }}"
            ))
            .map(|config| config.layout.canvas.pan_acceleration)
        };
        assert_eq!(parse("0").unwrap(), 0.);
        assert_eq!(parse("0.25").unwrap(), 0.25);
        assert!(parse("1.5").is_err());
        assert!(parse("-0.1").is_err());
    }

    #[test]
    fn canvas_pan_momentum_is_a_share() {
        let parse = |m: &str| {
            Config::parse_mem(&format!(
                "layout {{\n canvas {{\n pan-momentum {m}\n }}\n }}"
            ))
            .map(|config| config.layout.canvas.pan_momentum)
        };
        assert_eq!(parse("0").unwrap(), 0.);
        assert_eq!(parse("0.5").unwrap(), 0.5);
        assert!(parse("2").is_err());
        assert_eq!(
            Config::parse_mem("").unwrap().layout.canvas.pan_momentum,
            1.,
            "a pan glides on by default"
        );
    }

    #[test]
    fn canvas_overview_dim_is_an_alpha() {
        let parse = |dim: &str| {
            Config::parse_mem(&format!(
                "layout {{\n canvas {{\n overview-dim {dim}\n }}\n }}"
            ))
            .map(|config| config.layout.canvas.overview_dim)
        };
        assert_eq!(parse("0").unwrap(), 0.);
        assert_eq!(parse("1").unwrap(), 1.);
        assert!(parse("1.5").is_err());
        assert!(parse("-0.1").is_err());
    }
}
