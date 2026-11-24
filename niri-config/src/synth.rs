use niri_ipc::SynthMode;

use crate::utils::MergeWith;
use crate::{Color, FloatOrInt};

/// Declares the block's knobs once: the final config's fields (unset is `None`, meaning
/// newm-synth's default, the user's tuned look), the parsed part's fields with their ranges, the
/// merge, and the list of names.
macro_rules! synth_knobs {
    (
        $( $(#[$fdoc:meta])* $f:ident: $min:literal ..= $max:literal, )*
        ;
        $( $(#[$cdoc:meta])* $c:ident, )*
    ) => {
        /// newm's animated background, drawn by the compositor behind everything:
        /// `synth-background { ... }`.
        ///
        /// `mode`, `max-fps`, `on-battery`, `on-battery-max-fps`, `idle-still-after` and
        /// `react-to-sound` are newm's; the other knobs are the look's, named after the
        /// sliders of the playground it was tuned in. Unset knobs keep the tuned value, so
        /// `synth-background { mode "animated"; }` draws the look exactly as tuned.
        #[derive(Debug, Clone, PartialEq)]
        pub struct SynthBackground {
            /// `mode "off"` (the default: not drawn), `"still"` (one frame) or `"animated"`.
            /// The `toggle-synth-background` and `set-synth-background` actions change it until
            /// the config changes this setting.
            pub mode: SynthMode,
            /// A cap on how many times a second the picture moves: `max-fps 30`. Unset (the default)
            /// it moves on every frame the display shows, at the output's own refresh rate.
            pub max_fps: Option<f64>,
            /// What it does while the machine runs on battery: `on-battery "still"` (the
            /// default: it stops moving until the power comes back), `"animated"` (as on mains)
            /// or `"capped"` (it moves at most `on-battery-max-fps` times a second).
            pub on_battery: SynthOnBattery,
            /// The cap `on-battery "capped"` applies: `on-battery-max-fps 30`.
            pub on_battery_max_fps: f64,
            /// Seconds without input after which it stops moving until the next input:
            /// `idle-still-after 300`. 0 never stops it. An idle inhibitor (a video playing) keeps
            /// it moving.
            pub idle_still_after: f64,
            /// Whether the look moves with the sound playing: `react-to-sound`. Off unless set;
            /// the `audio-*` knobs say how much, but only this switch listens. It listens only
            /// while the mode is animated, the background shows and some `audio-*` amount is
            /// not zero, and it never keeps an idle sound card awake.
            pub react_to_sound: bool,
            /// Seeds the stars and the ridges.
            pub seed: Option<u32>,
            $( $(#[$fdoc])* pub $f: Option<f64>, )*
            $( $(#[$cdoc])* pub $c: Option<Color>, )*
        }

        impl Default for SynthBackground {
            fn default() -> Self {
                Self {
                    mode: SynthMode::Off,
                    max_fps: None,
                    on_battery: SynthOnBattery::Still,
                    on_battery_max_fps: 30.,
                    idle_still_after: 300.,
                    react_to_sound: false,
                    seed: None,
                    $( $f: None, )*
                    $( $c: None, )*
                }
            }
        }

        #[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
        pub struct SynthBackgroundPart {
            #[knuffel(child, unwrap(argument, str))]
            pub mode: Option<SynthMode>,
            #[knuffel(child, unwrap(argument))]
            pub max_fps: Option<FloatOrInt<1, 1000>>,
            #[knuffel(child, unwrap(argument, str))]
            pub on_battery: Option<SynthOnBattery>,
            #[knuffel(child, unwrap(argument))]
            pub on_battery_max_fps: Option<FloatOrInt<1, 1000>>,
            #[knuffel(child, unwrap(argument))]
            pub idle_still_after: Option<FloatOrInt<0, 86400>>,
            #[knuffel(child)]
            pub react_to_sound: Option<crate::utils::Flag>,
            #[knuffel(child, unwrap(argument))]
            pub seed: Option<u32>,
            $(
                #[knuffel(child, unwrap(argument))]
                pub $f: Option<FloatOrInt<{ $min }, { $max }>>,
            )*
            $(
                #[knuffel(child)]
                pub $c: Option<Color>,
            )*
        }

        impl MergeWith<SynthBackgroundPart> for SynthBackground {
            fn merge_with(&mut self, part: &SynthBackgroundPart) {
                if let Some(x) = part.mode {
                    self.mode = x;
                }
                if let Some(x) = part.max_fps {
                    self.max_fps = Some(x.0);
                }
                if let Some(x) = part.on_battery {
                    self.on_battery = x;
                }
                if let Some(x) = part.on_battery_max_fps {
                    self.on_battery_max_fps = x.0;
                }
                if let Some(x) = part.idle_still_after {
                    self.idle_still_after = x.0;
                }
                if let Some(x) = part.react_to_sound {
                    self.react_to_sound = x.0;
                }
                if let Some(x) = part.seed {
                    self.seed = Some(x);
                }
                $(
                    if let Some(x) = part.$f {
                        self.$f = Some(x.0);
                    }
                )*
                $(
                    if let Some(x) = part.$c {
                        self.$c = Some(x);
                    }
                )*
            }
        }

        /// The look's knobs as the config spells them (with `_` for `-`): the numbers, then
        /// the colours. With `seed` they are the 42 values the look was tuned with.
        pub const SYNTH_KNOBS: &[&str] = &[ $( stringify!($f), )* $( stringify!($c), )* ];
    };
}

/// What the background does on battery: `on-battery "still"`, `"animated"` or `"capped"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthOnBattery {
    Animated,
    Still,
    Capped,
}

impl std::str::FromStr for SynthOnBattery {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "animated" => Ok(Self::Animated),
            "still" => Ok(Self::Still),
            "capped" => Ok(Self::Capped),
            _ => Err(miette::miette!(
                r#"invalid on-battery, can be "animated", "still" or "capped""#
            )),
        }
    }
}

synth_knobs! {
    /// The horizon's height, from -0.5 (bottom of the screen) to 0.5 (top).
    horizon: -1 ..= 1,
    /// How fast the sky darkens upward from the horizon glow.
    sky_fall: 0 ..= 100,
    /// How many star cells hold a star: 1 is 12% of them.
    stars: 0 ..= 8,
    /// The sun's radius in screen heights.
    sun_size: 0 ..= 1,
    /// The sun's centre above the horizon, in screen heights.
    sun_y: -1 ..= 1,
    /// The sun's centre across from the middle, in screen heights.
    sun_x: -4 ..= 4,
    /// Horizontal cuts across the sun's lower part; under 0.5 none.
    stripes: 0 ..= 64,
    /// How wide the cuts grow toward the sun's bottom.
    stripe_width: 0 ..= 1,
    /// How far the glow around the sun reaches; 0 none.
    halo: 0 ..= 1,
    /// How strongly the floor reflects the sun.
    reflection: 0 ..= 4,
    /// Ridge layers, up to 6.
    layers: 0 ..= 6,
    /// The nearest ridge's height in screen heights.
    ridge_height: -1 ..= 1,
    /// How much each finer octave of the ridges weighs against the one before.
    roughness: 0 ..= 1,
    /// Ridge noise cells per screen height.
    ridge_frequency: 0 ..= 100,
    /// How fast the ridges drift sideways.
    ridge_speed: -100 ..= 100,
    /// How much the far ridges fade toward the glow colour.
    haze: 0 ..= 2,
    /// How strongly the ridge tops are lined with the grid colour.
    rim: 0 ..= 8,
    /// The floor's perspective: the camera's height over the grid.
    perspective: 0 ..= 10,
    /// Grid lines across, per floor unit.
    grid_x: 0 ..= 64,
    /// Grid lines in depth, per floor unit.
    grid_z: 0 ..= 64,
    /// Grid line width in pixels at their distance.
    line_width: 0 ..= 16,
    /// The soft glow around the grid lines.
    grid_glow: 0 ..= 8,
    /// How fast the grid comes toward the viewer; the sun's cuts move with it.
    grid_speed: -10 ..= 10,
    /// How much the floor bends down toward the sides.
    curve: -1 ..= 1,
    /// How fast the slow cycle runs: one cycle takes 60 / cycle-speed seconds.
    cycle_speed: -100 ..= 100,
    /// How far the cycle moves the sun up and down.
    cycle_sun: -1 ..= 1,
    /// How far, in degrees, the cycle turns the hue.
    cycle_hue: -360 ..= 360,
    /// A fixed hue turn in degrees.
    hue: -360 ..= 360,
    /// Saturation: 1 as is, 0 grey.
    saturation: 0 ..= 4,
    /// The factor every colour is multiplied by.
    brightness: 0 ..= 4,
    /// Scanline darkening, 0 to 1.
    scanlines: 0 ..= 1,
    /// Bits per channel; under 7.5 the picture is dithered down to that many.
    bits: 1 ..= 8,
    /// How much the sun grows with sound.
    audio_sun: 0 ..= 4,
    /// How much the grid brightens with sound.
    audio_grid: 0 ..= 4,
    /// How much the ridges rise with sound.
    audio_ridge: 0 ..= 4,
    ;
    /// The sky's colour high up.
    sky_color,
    /// The glow at the horizon, the haze and the floor's near colour.
    glow_color,
    /// The grid lines and the ridge rims.
    grid_color,
    /// The top of the sun.
    sun_top_color,
    /// The bottom of the sun.
    sun_bottom_color,
    /// The nearest ridges.
    ridge_color,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Config};

    #[test]
    fn synth_background_is_off_unless_configured() {
        let config = Config::parse_mem("").unwrap();
        assert_eq!(config.synth_background, SynthBackground::default());
        assert_eq!(config.synth_background.mode, SynthMode::Off);
        assert_eq!(
            config.synth_background.max_fps, None,
            "the display's own rate"
        );
        assert_eq!(config.synth_background.on_battery, SynthOnBattery::Still);
        assert_eq!(config.synth_background.idle_still_after, 300.);
        assert!(!config.synth_background.react_to_sound);
    }

    #[test]
    fn pacing_and_power_knobs_parse() {
        let s = Config::parse_mem(
            "synth-background { max-fps 144; on-battery \"capped\"; on-battery-max-fps 20; idle-still-after 0; }",
        )
        .unwrap()
        .synth_background;
        assert_eq!(s.max_fps, Some(144.));
        assert_eq!(s.on_battery, SynthOnBattery::Capped);
        assert_eq!(s.on_battery_max_fps, 20.);
        assert_eq!(s.idle_still_after, 0.);
        for (text, want) in [
            ("animated", SynthOnBattery::Animated),
            ("still", SynthOnBattery::Still),
        ] {
            let c = Config::parse_mem(&format!("synth-background {{ on-battery \"{text}\"; }}"));
            assert_eq!(c.unwrap().synth_background.on_battery, want);
        }
    }

    #[test]
    fn react_to_sound_is_a_flag() {
        let on = |text: &str| {
            Config::parse_mem(&format!("synth-background {{ {text}; }}"))
                .unwrap()
                .synth_background
                .react_to_sound
        };
        assert!(on("react-to-sound"));
        assert!(on("react-to-sound true"));
        assert!(!on("react-to-sound false"));
        assert!(!on("audio-grid 0.5"), "an amount alone does not listen");
    }

    #[test]
    fn synth_background_parses_every_knob() {
        // One line per knob, each a distinct value, so a knob landing in the wrong field shows.
        let mut text =
            String::from("synth-background {\n mode \"animated\"\n max-fps 2\n seed 9\n");
        let numbers: Vec<_> = SYNTH_KNOBS
            .iter()
            .filter(|k| !k.ends_with("_color"))
            .collect();
        let value = |i: usize, knob: &str| {
            // Every range holds 0.01 to 0.35, but bits start at 1.
            if knob == "bits" {
                2.
            } else {
                f64::from(i as u32 + 1) / 100.
            }
        };
        for (i, knob) in numbers.iter().enumerate() {
            text += &format!(" {} {:?}\n", knob.replace('_', "-"), value(i, knob));
        }
        for knob in SYNTH_KNOBS.iter().filter(|k| k.ends_with("_color")) {
            text += &format!(" {} \"#102030\"\n", knob.replace('_', "-"));
        }
        text += "}\n";
        let config = Config::parse_mem(&text).unwrap();
        let s = &config.synth_background;
        assert_eq!(s.mode, SynthMode::Animated);
        assert_eq!(s.max_fps, Some(2.));
        assert_eq!(s.seed, Some(9));
        assert_eq!(SYNTH_KNOBS.len(), 41, "41 knobs and the seed make the 42");
        let debug = format!("{s:?}");
        for (i, knob) in numbers.iter().enumerate() {
            let want = format!("{knob}: Some({:?})", value(i, knob));
            assert!(debug.contains(&want), "{want} in {debug}");
        }
        assert_eq!(
            s.ridge_color.map(|c| (c.r, c.g, c.b)),
            Some((16. / 255., 32. / 255., 48. / 255.))
        );
    }

    #[test]
    fn synth_background_actions_bind() {
        let config = Config::parse_mem(
            r#"
            binds {
                Mod+B { toggle-synth-background; }
                Mod+Shift+B { set-synth-background "still"; }
            }
            "#,
        )
        .unwrap();
        let actions: Vec<Action> = config
            .binds
            .0
            .iter()
            .filter_map(|bind| bind.press_action().cloned())
            .collect();
        assert_eq!(
            actions,
            [
                Action::ToggleSynthBackground,
                Action::SetSynthBackground(SynthMode::Still),
            ]
        );
        assert!(Config::parse_mem("binds { Mod+B { set-synth-background \"on\"; } }").is_err());
    }

    #[test]
    fn synth_background_rejects_what_it_cannot_draw() {
        for bad in [
            r#"mode "on""#,
            r#"look "synthwave""#,
            "layers 7",
            "bits 0",
            "scanlines 2",
            "max-fps 0",
            "max-fps 1001",
            r#"on-battery "dim""#,
            "on-battery-max-fps 0",
            "idle-still-after -1",
            r#"sky-color "not a colour""#,
        ] {
            let text = format!("synth-background {{ {bad}; }}");
            assert!(Config::parse_mem(&text).is_err(), "{bad} parsed");
        }
    }
}
