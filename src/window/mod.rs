use std::cmp::{max, min};

use niri_config::utils::MergeWith as _;
use niri_config::window_rule::{Match, OnXdgActivate, VrrRule, WindowRule};
use niri_config::{
    BackgroundEffect, BlockOutFrom, BorderRule, CornerRadius, FloatOrInt, FloatingPosition,
    PresetSize, RelativeTo, ResolvedPopupsRules, ShadowRule, TabIndicatorRule,
};
use niri_ipc::ColumnDisplay;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::utils::{Logical, Size};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::{
    SurfaceCachedState, ToplevelSurface, XdgToplevelSurfaceRoleAttributes,
};

use crate::layout::ActivateWindow;
use crate::utils::{output_size, with_toplevel_role};

pub mod mapped;
pub use mapped::Mapped;

pub mod unmapped;
pub use unmapped::{InitialConfigureState, Unmapped};

/// Reference to a mapped or unmapped window.
#[derive(Debug, Clone, Copy)]
pub enum WindowRef<'a> {
    Unmapped(&'a Unmapped),
    Mapped(&'a Mapped),
}

/// Rules fully resolved for a window.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct ResolvedWindowRules {
    /// Default width for this window.
    ///
    /// - `None`: unset (global default should be used).
    /// - `Some(None)`: set to empty (window picks its own width).
    /// - `Some(Some(width))`: set to a particular width.
    pub default_width: Option<Option<PresetSize>>,

    /// Default height for this window.
    ///
    /// - `None`: unset (global default should be used).
    /// - `Some(None)`: set to empty (window picks its own height).
    /// - `Some(Some(height))`: set to a particular height.
    pub default_height: Option<Option<PresetSize>>,

    /// Default column display for this window.
    pub default_column_display: Option<ColumnDisplay>,

    /// Default floating position for this window.
    pub default_floating_position: Option<FloatingPosition>,

    /// Output to open this window on.
    pub open_on_output: Option<String>,

    /// Workspace to open this window on.
    pub open_on_workspace: Option<String>,

    /// Whether the window should open full-width.
    pub open_maximized: Option<bool>,

    /// Whether the window should open maximized to edges (true maximized).
    pub open_maximized_to_edges: Option<bool>,

    /// Whether the window should open fullscreen.
    pub open_fullscreen: Option<bool>,

    /// Whether the window should open floating.
    pub open_floating: Option<bool>,

    /// Whether the window should open focused.
    pub open_focused: Option<bool>,

    /// What to do on xdg-activation requests.
    pub on_xdg_activate: Option<OnXdgActivate>,

    /// Extra bound on the minimum window width.
    pub min_width: Option<u16>,
    /// Extra bound on the minimum window height.
    pub min_height: Option<u16>,
    /// Extra bound on the maximum window width.
    pub max_width: Option<u16>,
    /// Extra bound on the maximum window height.
    pub max_height: Option<u16>,

    /// Focus ring overrides.
    pub focus_ring: BorderRule,
    /// Window border overrides.
    pub border: BorderRule,
    /// Shadow overrides.
    pub shadow: ShadowRule,
    /// Tab indicator overrides.
    pub tab_indicator: TabIndicatorRule,

    /// Whether or not to draw the border with a solid background.
    ///
    /// `None` means using the SSD heuristic.
    pub draw_border_with_background: Option<bool>,

    /// Extra opacity to draw this window with.
    pub opacity: Option<f32>,

    /// Corner radius to assume this window has.
    pub geometry_corner_radius: Option<CornerRadius>,

    /// Whether to clip this window to its geometry, including the corner radius.
    pub clip_to_geometry: Option<bool>,

    /// Whether to bob this window up and down.
    pub baba_is_float: Option<bool>,

    /// Whether to block out this window from certain render targets.
    pub block_out_from: Option<BlockOutFrom>,

    /// Whether to enable VRR on this window's primary output if it is on-demand.
    pub variable_refresh_rate: Option<VrrRule>,

    /// Multiplier for all scroll events sent to this window.
    pub scroll_factor: Option<f64>,

    /// Pinch gesture sensitivity for this window.
    pub pinch_sensitivity: Option<f64>,

    /// Override whether to set the Tiled xdg-toplevel state on the window.
    pub tiled_state: Option<bool>,

    /// Background effect configuration.
    pub background_effect: BackgroundEffect,

    /// Rules for this window's popups.
    pub popups: ResolvedPopupsRules,
}

impl<'a> WindowRef<'a> {
    pub fn toplevel(self) -> &'a ToplevelSurface {
        match self {
            WindowRef::Unmapped(unmapped) => unmapped.toplevel(),
            WindowRef::Mapped(mapped) => mapped.toplevel(),
        }
    }

    pub fn is_focused(self) -> bool {
        match self {
            WindowRef::Unmapped(_) => false,
            WindowRef::Mapped(mapped) => mapped.is_focused(),
        }
    }

    pub fn is_urgent(self) -> bool {
        match self {
            WindowRef::Unmapped(_) => false,
            WindowRef::Mapped(mapped) => mapped.is_urgent(),
        }
    }

    pub fn is_active_in_column(self) -> bool {
        match self {
            WindowRef::Unmapped(_) => true,
            WindowRef::Mapped(mapped) => mapped.is_active_in_column(),
        }
    }

    pub fn is_floating(self) -> bool {
        match self {
            // FIXME: This means you cannot set initial configure rules based on is-floating. I'm
            // not sure there's a good way to support it, since this matcher makes a cycle with the
            // open-floating rule.
            //
            // That said, I don't think there are a lot of useful initial configure properties you
            // may want to set through an is-floating matcher? Like, if you're configuring a
            // specific window to open as floating, you can also set those properties in that same
            // window rule, rather than relying on a different is-floating rule.
            WindowRef::Unmapped(_) => false,
            WindowRef::Mapped(mapped) => mapped.is_floating(),
        }
    }

    pub fn is_window_cast_target(self) -> bool {
        match self {
            WindowRef::Unmapped(_) => false,
            WindowRef::Mapped(mapped) => mapped.is_window_cast_target(),
        }
    }
}

impl ResolvedWindowRules {
    pub fn compute(rules: &[WindowRule], window: WindowRef, is_at_startup: bool) -> Self {
        let _span = tracy_client::span!("ResolvedWindowRules::compute");

        let mut resolved = ResolvedWindowRules::default();

        with_toplevel_role(window.toplevel(), |role| {
            // Ensure server_pending like in Smithay's with_pending_state().
            if role.server_pending.is_none() {
                role.server_pending = Some(role.current_server_state().clone());
            }

            let mut open_on_output = None;
            let mut open_on_workspace = None;

            for rule in rules {
                let matches = |m: &Match| {
                    if let Some(at_startup) = m.at_startup {
                        if at_startup != is_at_startup {
                            return false;
                        }
                    }

                    window_matches(window, role, m)
                };

                if !(rule.matches.is_empty() || rule.matches.iter().any(matches)) {
                    continue;
                }

                if rule.excludes.iter().any(matches) {
                    continue;
                }

                if let Some(x) = rule.default_column_width {
                    resolved.default_width = Some(x.0);
                }

                if let Some(x) = rule.default_window_height {
                    resolved.default_height = Some(x.0);
                }

                if let Some(x) = rule.default_column_display {
                    resolved.default_column_display = Some(x);
                }

                if let Some(x) = rule.default_floating_position {
                    resolved.default_floating_position = Some(x);
                }

                if let Some(x) = rule.open_on_output.as_deref() {
                    open_on_output = Some(x);
                }

                if let Some(x) = rule.open_on_workspace.as_deref() {
                    open_on_workspace = Some(x);
                }

                if let Some(x) = rule.open_maximized {
                    resolved.open_maximized = Some(x);
                }

                if let Some(x) = rule.open_maximized_to_edges {
                    resolved.open_maximized_to_edges = Some(x);
                }

                if let Some(x) = rule.open_fullscreen {
                    resolved.open_fullscreen = Some(x);
                }

                if let Some(x) = rule.open_floating {
                    resolved.open_floating = Some(x);
                }

                if let Some(x) = rule.open_focused {
                    resolved.open_focused = Some(x);
                }

                if let Some(x) = rule.on_xdg_activate {
                    resolved.on_xdg_activate = Some(x);
                }

                if let Some(x) = rule.min_width {
                    resolved.min_width = Some(x);
                }
                if let Some(x) = rule.min_height {
                    resolved.min_height = Some(x);
                }
                if let Some(x) = rule.max_width {
                    resolved.max_width = Some(x);
                }
                if let Some(x) = rule.max_height {
                    resolved.max_height = Some(x);
                }

                resolved.focus_ring.merge_with(&rule.focus_ring);
                resolved.border.merge_with(&rule.border);
                resolved.shadow.merge_with(&rule.shadow);
                resolved.tab_indicator.merge_with(&rule.tab_indicator);

                if let Some(x) = rule.draw_border_with_background {
                    resolved.draw_border_with_background = Some(x);
                }
                if let Some(x) = rule.opacity {
                    resolved.opacity = Some(x);
                }
                if let Some(x) = rule.geometry_corner_radius {
                    resolved.geometry_corner_radius = Some(x);
                }
                if let Some(x) = rule.clip_to_geometry {
                    resolved.clip_to_geometry = Some(x);
                }
                if let Some(x) = rule.baba_is_float {
                    resolved.baba_is_float = Some(x);
                }
                if let Some(x) = rule.block_out_from {
                    resolved.block_out_from = Some(x);
                }
                if let Some(x) = rule.variable_refresh_rate {
                    resolved.variable_refresh_rate = Some(x);
                }
                if let Some(x) = rule.scroll_factor {
                    resolved.scroll_factor = Some(x.0);
                }
                if let Some(x) = rule.pinch_sensitivity {
                    resolved.pinch_sensitivity = Some(x.0);
                }
                if let Some(x) = rule.tiled_state {
                    resolved.tiled_state = Some(x);
                }

                resolved
                    .background_effect
                    .merge_with(&rule.background_effect);

                resolved.popups.merge_with(&rule.popups);
            }

            resolved.open_on_output = open_on_output.map(|x| x.to_owned());
            resolved.open_on_workspace = open_on_workspace.map(|x| x.to_owned());
        });

        resolved
    }

    pub fn apply_min_size(&self, min_size: Size<i32, Logical>) -> Size<i32, Logical> {
        let mut size = min_size;

        if let Some(x) = self.min_width {
            size.w = max(size.w, i32::from(x));
        }
        if let Some(x) = self.min_height {
            size.h = max(size.h, i32::from(x));
        }

        size
    }

    pub fn apply_max_size(&self, max_size: Size<i32, Logical>) -> Size<i32, Logical> {
        let mut size = max_size;

        if let Some(x) = self.max_width {
            if size.w == 0 {
                size.w = i32::from(x);
            } else if x > 0 {
                size.w = min(size.w, i32::from(x));
            }
        }
        if let Some(x) = self.max_height {
            if size.h == 0 {
                size.h = i32::from(x);
            } else if x > 0 {
                size.h = min(size.h, i32::from(x));
            }
        }

        size
    }

    pub fn apply_min_max_size(
        &self,
        min_size: Size<i32, Logical>,
        max_size: Size<i32, Logical>,
    ) -> (Size<i32, Logical>, Size<i32, Logical>) {
        let min_size = self.apply_min_size(min_size);
        let max_size = self.apply_max_size(max_size);
        (min_size, max_size)
    }

    /// Whether the window opens floating, on an output of logical size `output`: as its
    /// `open-floating` rule says, else as its own hints say (see [`OpenHint`]).
    pub fn compute_open_floating(
        &self,
        toplevel: &ToplevelSurface,
        output: Option<Size<i32, Logical>>,
    ) -> bool {
        self.open_floating
            .unwrap_or_else(|| self.open_hint(toplevel, output) != OpenHint::Tiled)
    }

    /// Whether the window opens as an overlay: no `open-floating` rule decides, and its own hints
    /// make it one on an output of logical size `output` (see [`OpenHint::Overlay`]).
    pub fn opens_as_overlay(
        &self,
        toplevel: &ToplevelSurface,
        output: Option<Size<i32, Logical>>,
    ) -> bool {
        self.open_floating.is_none() && self.open_hint(toplevel, output) == OpenHint::Overlay
    }

    fn open_hint(
        &self,
        toplevel: &ToplevelSurface,
        output: Option<Size<i32, Logical>>,
    ) -> OpenHint {
        let (min_size, max_size) = self.open_min_max(toplevel);
        OpenHint::compute(toplevel.parent().is_some(), min_size, max_size, output)
    }

    /// The minimum and maximum size a new window opens with: its own, then the rules'.
    pub fn open_min_max(
        &self,
        toplevel: &ToplevelSurface,
    ) -> (Size<i32, Logical>, Size<i32, Logical>) {
        let (min_size, max_size) = with_states(toplevel.wl_surface(), |state| {
            let mut guard = state.cached_state.get::<SurfaceCachedState>();
            let current = guard.current();
            (current.min_size, current.max_size)
        });
        self.apply_min_max_size(min_size, max_size)
    }

    /// Whether a new window takes the focus: as its `open-focused` rule says, else when it brings
    /// an activation token. Without either, an overlay stays unfocused, as does any window under
    /// the strict focus policy; others are activated by the layout's heuristics.
    pub fn open_activation(
        &self,
        has_token: bool,
        is_overlay: bool,
        strict_policy: bool,
    ) -> ActivateWindow {
        match self.open_focused {
            Some(true) => ActivateWindow::Yes,
            Some(false) => ActivateWindow::No,
            None if has_token => ActivateWindow::Yes,
            None if is_overlay || strict_policy => ActivateWindow::No,
            None => ActivateWindow::Smart,
        }
    }
}

/// How a window opens by its own hints, when no `open-floating` rule decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenHint {
    Tiled,
    /// A dialog of a parent window, or a fixed-height window.
    Floating,
    /// On the canvas, a window with no parent that caps its size on both axes within the output
    /// it opens on, its height not fixed, as Chromium's picture-in-picture windows do (at 80% of
    /// the display). It need not fill a tile span: it floats in a corner at its own size, and
    /// takes the focus only when it asks for activation. A cap larger than the output, such as
    /// Firefox's (8102 or 13017 px on each axis), limits nothing and the window tiles.
    Overlay,
}

impl OpenHint {
    /// The hint for a window with or without a parent, its minimum and maximum size, opening on
    /// an output of logical size `output`.
    pub fn compute(
        has_parent: bool,
        min_size: Size<i32, Logical>,
        max_size: Size<i32, Logical>,
        output: Option<Size<i32, Logical>>,
    ) -> Self {
        if has_parent || (min_size.h > 0 && min_size.h == max_size.h) {
            Self::Floating
        } else if cfg!(feature = "canvas") && cap_fits(max_size, output) {
            Self::Overlay
        } else {
            Self::Tiled
        }
    }
}

/// Whether maximum size `max` caps both axes within `output`, the logical size of the output a
/// window opens on. Without a known output no cap counts.
pub fn cap_fits(max: Size<i32, Logical>, output: Option<Size<i32, Logical>>) -> bool {
    max.w > 0 && max.h > 0 && output.is_some_and(|o| max.w <= o.w && max.h <= o.h)
}

/// The logical size of `output`, in whole pixels, for [`OpenHint::compute`].
pub fn open_output_size(output: &Output) -> Size<i32, Logical> {
    output_size(output).to_i32_round()
}

/// Where an overlay opens when no `default-floating-position` rule places it: the bottom right
/// corner of the working area, `gap` from its edges.
pub fn overlay_position(gap: f64) -> FloatingPosition {
    FloatingPosition {
        x: FloatOrInt(gap),
        y: FloatOrInt(gap),
        relative_to: RelativeTo::BottomRight,
    }
}

fn window_matches(window: WindowRef, role: &XdgToplevelSurfaceRoleAttributes, m: &Match) -> bool {
    // Must be ensured by the caller.
    let server_pending = role.server_pending.as_ref().unwrap();

    if let Some(is_focused) = m.is_focused {
        if window.is_focused() != is_focused {
            return false;
        }
    }

    if let Some(is_urgent) = m.is_urgent {
        if window.is_urgent() != is_urgent {
            return false;
        }
    }

    if let Some(is_active) = m.is_active {
        // Our "is-active" definition corresponds to the window having a pending Activated state.
        let pending_activated = server_pending
            .states
            .contains(xdg_toplevel::State::Activated);
        if is_active != pending_activated {
            return false;
        }
    }

    if let Some(app_id_re) = &m.app_id {
        let Some(app_id) = &role.app_id else {
            return false;
        };
        if !app_id_re.0.is_match(app_id) {
            return false;
        }
    }

    if let Some(title_re) = &m.title {
        let Some(title) = &role.title else {
            return false;
        };
        if !title_re.0.is_match(title) {
            return false;
        }
    }

    if let Some(is_active_in_column) = m.is_active_in_column {
        if window.is_active_in_column() != is_active_in_column {
            return false;
        }
    }

    if let Some(is_floating) = m.is_floating {
        if window.is_floating() != is_floating {
            return false;
        }
    }

    if let Some(is_window_cast_target) = m.is_window_cast_target {
        if window.is_window_cast_target() != is_window_cast_target {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_hints() {
        let size = |w, h| Size::from((w, h));
        let none = size(0, 0);
        let fhd = Some(size(1920, 1080));
        let hint = |parent, min, max| OpenHint::compute(parent, min, max, fhd);
        assert_eq!(hint(true, none, none), OpenHint::Floating);
        assert_eq!(hint(true, none, size(800, 600)), OpenHint::Floating);
        assert_eq!(
            hint(false, size(400, 300), size(400, 300)),
            OpenHint::Floating
        );
        assert_eq!(hint(false, size(0, 300), size(0, 300)), OpenHint::Floating);
        assert_eq!(hint(false, none, none), OpenHint::Tiled);
        assert_eq!(hint(false, size(500, 400), none), OpenHint::Tiled);
        assert_eq!(hint(false, none, size(800, 0)), OpenHint::Tiled);

        // Chromium's picture-in-picture windows on a 2560x1440 display.
        let qhd = Some(size(2560, 1440));
        let pip = OpenHint::compute(false, size(240, 52), size(2048, 1152), qhd);
        let overlay = if cfg!(feature = "canvas") {
            OpenHint::Overlay
        } else {
            OpenHint::Tiled
        };
        assert_eq!(pip, overlay);
    }

    /// Chromium's picture-in-picture caps itself at 80% of the display and stays an overlay;
    /// Firefox's cap is far larger than any output and limits nothing, so it tiles.
    #[test]
    fn a_cap_counts_only_within_the_output() {
        let size = |w, h| Size::from((w, h));
        let fhd = Some(size(1920, 1080));
        let overlay = if cfg!(feature = "canvas") {
            OpenHint::Overlay
        } else {
            OpenHint::Tiled
        };
        let firefox = OpenHint::compute(false, size(500, 120), size(8102, 8102), fhd);
        assert_eq!(firefox, OpenHint::Tiled, "Firefox's 8102 cap tiles");
        let wide = OpenHint::compute(
            false,
            size(500, 120),
            size(13017, 13017),
            Some(size(4096, 1152)),
        );
        assert_eq!(wide, OpenHint::Tiled, "and its 13017 cap on a wide output");
        let pip = OpenHint::compute(false, size(240, 52), size(1536, 864), fhd);
        assert_eq!(pip, overlay, "PiP at 80% of 1920x1080");
        let pip = OpenHint::compute(
            false,
            size(248, 90),
            size(3277, 922),
            Some(size(4096, 1152)),
        );
        assert_eq!(pip, overlay, "PiP at 80% of 4096x1152");
        let whole = OpenHint::compute(false, size(240, 52), size(1920, 1080), fhd);
        assert_eq!(whole, overlay, "a cap of the whole output still fits");
        let taller = OpenHint::compute(false, size(240, 52), size(1536, 1081), fhd);
        assert_eq!(taller, OpenHint::Tiled, "one axis past the output: no cap");
        let unknown = OpenHint::compute(false, size(240, 52), size(1536, 864), None);
        assert_eq!(unknown, OpenHint::Tiled, "no output, no cap");

        let fixed = OpenHint::compute(false, size(812, 124), size(812, 124), fhd);
        assert_eq!(fixed, OpenHint::Floating, "a fixed height floats");
        let fixed = OpenHint::compute(false, size(400, 300), size(9000, 300), fhd);
        assert_eq!(fixed, OpenHint::Floating, "whatever its width cap");
        let dialog = OpenHint::compute(true, size(0, 0), size(8102, 8102), fhd);
        assert_eq!(dialog, OpenHint::Floating, "a child floats");

        assert!(cap_fits(size(1536, 864), fhd));
        assert!(!cap_fits(size(1536, 0), fhd));
        assert!(!cap_fits(size(8102, 8102), fhd));
    }

    #[test]
    fn open_activation() {
        let rules = |open_focused| ResolvedWindowRules {
            open_focused,
            ..ResolvedWindowRules::default()
        };
        let none = rules(None);
        assert_eq!(
            none.open_activation(false, false, false),
            ActivateWindow::Smart
        );
        assert_eq!(none.open_activation(false, true, false), ActivateWindow::No);
        assert_eq!(none.open_activation(false, false, true), ActivateWindow::No);
        assert_eq!(none.open_activation(true, true, true), ActivateWindow::Yes);
        let focused = rules(Some(true));
        assert_eq!(
            focused.open_activation(false, true, true),
            ActivateWindow::Yes
        );
        let unfocused = rules(Some(false));
        assert_eq!(
            unfocused.open_activation(true, false, false),
            ActivateWindow::No
        );
    }
}
