use knus::errors::DecodeError;
use niri_ipc::ColumnDisplay;

use crate::appearance::{
    BackgroundEffect, BackgroundEffectRule, BlockOutFrom, BorderRule, CornerRadius, ShadowRule,
    TabIndicatorRule,
};
use crate::layout::DefaultPresetSize;
use crate::utils::{MergeWith, RegexEq};
use crate::FloatOrInt;

#[derive(knus::Decode, Debug, Default, Clone, PartialEq)]
pub struct WindowRule {
    #[knus(children(name = "match"))]
    pub matches: Vec<Match>,
    #[knus(children(name = "exclude"))]
    pub excludes: Vec<Match>,

    // Rules applied at initial configure.
    #[knus(child)]
    pub default_column_width: Option<DefaultPresetSize>,
    #[knus(child)]
    pub default_window_height: Option<DefaultPresetSize>,
    #[knus(child, unwrap(argument))]
    pub open_on_output: Option<String>,
    #[knus(child, unwrap(argument))]
    pub open_on_workspace: Option<String>,
    #[knus(child, unwrap(argument))]
    pub open_maximized: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub open_maximized_to_edges: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub open_fullscreen: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub open_floating: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub open_focused: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub on_xdg_activate: Option<OnXdgActivate>,

    // Rules applied dynamically.
    #[knus(child, unwrap(argument))]
    pub min_width: Option<u16>,
    #[knus(child, unwrap(argument))]
    pub min_height: Option<u16>,
    #[knus(child, unwrap(argument))]
    pub max_width: Option<u16>,
    #[knus(child, unwrap(argument))]
    pub max_height: Option<u16>,

    #[knus(child, default)]
    pub focus_ring: BorderRule,
    #[knus(child, default)]
    pub border: BorderRule,
    #[knus(child, default)]
    pub shadow: ShadowRule,
    #[knus(child, default)]
    pub tab_indicator: TabIndicatorRule,
    #[knus(child, unwrap(argument))]
    pub draw_border_with_background: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knus(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knus(child, unwrap(argument))]
    pub clip_to_geometry: Option<bool>,
    #[knus(child, unwrap(argument))]
    pub baba_is_float: Option<bool>,
    #[knus(child)]
    pub block_out_from: Option<BlockOutFrom>,
    #[knus(child, unwrap(argument))]
    pub variable_refresh_rate: Option<VrrRule>,
    #[knus(child, unwrap(argument, str))]
    pub default_column_display: Option<ColumnDisplay>,
    #[knus(child)]
    pub default_floating_position: Option<FloatingPosition>,
    #[knus(child, unwrap(argument))]
    pub scroll_factor: Option<FloatOrInt<0, 100>>,
    #[knus(child, unwrap(argument))]
    pub pinch_sensitivity: Option<FloatOrInt<0, 100>>,
    #[knus(child, unwrap(argument))]
    pub tiled_state: Option<bool>,
    #[knus(child, default)]
    pub background_effect: BackgroundEffectRule,
    #[knus(child, default)]
    pub popups: PopupsRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VrrRule {
    /// No effect on VRR.
    Off,
    /// Enable VRR when the window is visible.
    On,
    /// Enable VRR when the window is the current fullscreen window.
    FullscreenOnly,
}

/// Rules for popup surfaces.
#[derive(knus::Decode, Debug, Default, Clone, PartialEq)]
pub struct PopupsRule {
    #[knus(child, unwrap(argument))]
    pub opacity: Option<f32>,
    #[knus(child)]
    pub geometry_corner_radius: Option<CornerRadius>,
    #[knus(child, default)]
    pub background_effect: BackgroundEffectRule,
}

/// Resolved popup-specific rules.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ResolvedPopupsRules {
    /// Extra opacity to draw popups with.
    pub opacity: Option<f32>,

    /// Corner radius to assume the popups have.
    pub geometry_corner_radius: Option<CornerRadius>,

    /// Background effect configuration for popups.
    pub background_effect: BackgroundEffect,
}

impl MergeWith<PopupsRule> for ResolvedPopupsRules {
    fn merge_with(&mut self, part: &PopupsRule) {
        if let Some(x) = part.opacity {
            self.opacity = Some(x);
        }
        if let Some(x) = part.geometry_corner_radius {
            self.geometry_corner_radius = Some(x);
        }
        self.background_effect.merge_with(&part.background_effect);
    }
}

#[derive(knus::Decode, Debug, Default, Clone, PartialEq)]
pub struct Match {
    #[knus(property, str)]
    pub app_id: Option<RegexEq>,
    #[knus(property, str)]
    pub title: Option<RegexEq>,
    #[knus(property)]
    pub is_active: Option<bool>,
    #[knus(property)]
    pub is_focused: Option<bool>,
    #[knus(property)]
    pub is_active_in_column: Option<bool>,
    #[knus(property)]
    pub is_floating: Option<bool>,
    #[knus(property)]
    pub is_window_cast_target: Option<bool>,
    #[knus(property)]
    pub is_urgent: Option<bool>,
    #[knus(property)]
    pub at_startup: Option<bool>,
}

#[derive(knus::Decode, Debug, Clone, Copy, PartialEq)]
pub struct FloatingPosition {
    #[knus(property)]
    pub x: FloatOrInt<-65535, 65535>,
    #[knus(property)]
    pub y: FloatOrInt<-65535, 65535>,
    #[knus(property, default)]
    pub relative_to: RelativeTo,
}

#[derive(knus::DecodeScalar, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnXdgActivate {
    Ignore,
    SetUrgent,
    Focus,
}

#[derive(knus::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RelativeTo {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
}

impl<S: knus::traits::ErrorSpan> knus::DecodeScalar<S> for VrrRule {
    fn raw_decode(
        val: &knus::span::Spanned<knus::ast::Literal, S>,
        _ctx: &mut knus::decode::Context<S>,
    ) -> Result<VrrRule, DecodeError<S>> {
        const ERROR: &str = "expected `true`, `\"fullscreen-only\"`, or `false`";
        match &**val {
            knus::ast::Literal::Bool(b) => Ok(if *b { VrrRule::On } else { VrrRule::Off }),
            knus::ast::Literal::String(s) => match &s[..] {
                // Also accept bool strings to reduce confusion.
                "true" => Ok(VrrRule::On),
                "false" => Ok(VrrRule::Off),
                "fullscreen-only" => Ok(VrrRule::FullscreenOnly),
                _ => Err(DecodeError::conversion(val, ERROR)),
            },
            _ => Err(DecodeError::conversion(val, ERROR)),
        }
    }

    fn type_check(
        type_name: &Option<knus::span::Spanned<knus::ast::TypeName, S>>,
        ctx: &mut knus::decode::Context<S>,
    ) {
        if let Some(typ) = type_name {
            ctx.emit_error(DecodeError::TypeName {
                span: typ.span().clone(),
                found: Some((**typ).clone()),
                expected: knus::errors::ExpectedType::no_type(),
                rust_type: stringify!(VrrRule),
            });
        }
    }
}
