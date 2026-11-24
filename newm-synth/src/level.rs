//! The sound level the look moves with: an RMS from the sound card's output, mapped to 0..1 on
//! a decibel scale, held at its peak between frames, smoothed with an attack and a release, and
//! stepped so that a frame is drawn again only when the level moves the picture visibly.

use std::time::Duration;

use crate::look::Look;

/// The quietest sound that moves the look, in dB below full scale: −50 dBFS is level 0.
pub const FLOOR_DB: f32 = -50.;
/// The span of loudness the level covers: −10 dBFS and louder is level 1.
pub const RANGE_DB: f32 = 40.;
/// Seconds the level takes to rise most of the way (63%) to a louder sound.
pub const ATTACK: f32 = 0.05;
/// Seconds the level takes to fall most of the way to a quieter one.
pub const RELEASE: f32 = 0.4;

/// A level in 0..1 for a block of samples with root mean square `rms` (full scale 1).
pub fn level_of_rms(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0. {
        return 0.;
    }
    ((20. * rms.log10() - FLOOR_DB) / RANGE_DB).clamp(0., 1.)
}

/// The root mean square of mono samples.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// The smoothed level, fed by sound blocks and advanced once per frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Level {
    level: f32,
    /// The loudest block since the last frame: a beat between two frames still counts.
    peak: f32,
}

impl Level {
    pub fn new() -> Self {
        Self::default()
    }

    /// A block of sound came in, as a level (see [`level_of_rms`]).
    pub fn feed(&mut self, level: f32) {
        if level.is_finite() {
            self.peak = self.peak.max(level.clamp(0., 1.));
        }
    }

    /// Moves the level `dt` on toward the loudest block since the last call (0 when none
    /// came), with the attack rising and the release falling. Returns the new level.
    pub fn advance(&mut self, dt: Duration) -> f32 {
        let target = std::mem::take(&mut self.peak);
        let tau = if target > self.level { ATTACK } else { RELEASE };
        let k = 1. - (-dt.as_secs_f32() / tau).exp();
        self.level += (target - self.level) * k;
        if self.level < 1e-4 {
            self.level = 0.;
        }
        self.level
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    /// Whether sound came in since the last frame.
    pub fn pending(&self) -> bool {
        self.peak > 0.
    }
}

impl Look {
    /// Whether sound moves anything: some audio amount is not zero.
    pub fn hears(&self) -> bool {
        self.audio_sun != 0. || self.audio_grid != 0. || self.audio_ridge != 0.
    }

    /// The step the level is drawn in on an output `height` physical pixels tall, or `None`
    /// when sound moves nothing. One step is the smallest change that shows: one 8-bit level
    /// of the grid lines' brightest channel (the grid brightens by `2 · audio-grid` at full
    /// level, before the finish's brightness), or a quarter pixel of the nearest ridge's top
    /// or of the sun's edge. A level between two steps draws as the nearer one, so a frame
    /// with the level one step on changes some pixel, and one within a step changes none.
    pub fn level_quantum(&self, height: u32) -> Option<f32> {
        if !self.hears() {
            return None;
        }
        let h = height.max(1) as f32;
        let mut q: f32 = 1.;
        if self.audio_grid != 0. {
            let grid = self.grid_color.iter().fold(0f32, |m, &c| m.max(c.abs()));
            let levels = 2. * self.audio_grid.abs() * grid * 255. * self.brightness.abs();
            if levels > 0. {
                q = q.min(1. / levels);
            }
        }
        if self.audio_ridge != 0. && self.layers > 0. {
            let px = self.audio_ridge.abs() * self.ridge_height.abs() * h;
            if px > 0. {
                q = q.min(0.25 / px);
            }
        }
        if self.audio_sun != 0. && self.sun_size > 0. {
            let px = self.audio_sun.abs() * self.sun_size * h;
            if px > 0. {
                q = q.min(0.25 / px);
            }
        }
        Some(q.clamp(1. / 1024., 1.))
    }
}

/// `level` to the nearest step of `quantum`: the step number and the level it draws as.
pub fn quantise(level: f32, quantum: f32) -> (u32, f32) {
    let step = (level.clamp(0., 1.) / quantum + 0.5) as u32;
    (step, (step as f32 * quantum).min(1.))
}
