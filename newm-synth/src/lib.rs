//! newm's animated background: a synthwave night, the look the user tuned in a WebGL
//! playground.
//!
//! - [`Look`] holds the 42 knobs, with the tuned values as defaults.
//! - [`Tables`] holds the seeded randomness (stars and ridge noise), the same bytes everywhere.
//! - [`Frame`] works out one frame's uniforms in f64, every time term wrapped to its period.
//! - [`Level`] turns the sound playing into the level the look moves with, in visible steps.
//! - [`FRAGMENT_SHADER`] draws a frame on the GPU, for niri's shader elements.
//! - [`render`] draws the same pixels on the CPU, in f32: the reference for a lock-screen still and
//!   for compositors that draw without a shader.
//!
//! `tests/lab.frag` and `tests/lab-settings.json` are the playground's shader and the user's
//! settings as they sent them; newm-rs checks the shader against them.

mod cpu;
mod frame;
mod level;
mod look;
pub mod png;
pub mod power;

pub use cpu::{pack, render, shade, Painter};
pub use frame::Frame;
pub use level::{level_of_rms, quantise, rms, Level, ATTACK, FLOOR_DB, RANGE_DB, RELEASE};
pub use look::{Look, Tables, MAX_LAYERS, RIDGE_TABLE, STAR_TABLE, TABLE_HEIGHT, TABLE_WIDTH};

/// The look's fragment shader, GLSL ES 1.00, for niri's shader elements: it reads the uniforms
/// of [`Frame`] and the [`Tables`] texture as `synth_tables`. Define `NEWM_LAB_HASH` before it
/// to get the playground's sine hashes back, for comparing with the playground.
pub const FRAGMENT_SHADER: &str = include_str!("look.frag");
