//! Buffered messages used to decouple the gameplay systems from the hardware.

use bevy::prelude::*;

use crate::components::controller::Hand;

/// Request a haptic pulse on one controller.
///
/// Gameplay systems write these; only `apply_haptics` knows about OpenXR.
#[derive(Message, Clone, Copy, Debug)]
pub struct HapticPulse {
    pub hand: Hand,
    /// 0.0..=1.0.
    pub amplitude: f32,
    pub duration: std::time::Duration,
    /// Hz. 0.0 lets the runtime pick.
    pub frequency: f32,
}

impl HapticPulse {
    /// A short confirmation tick, the kind you want on pick-up and drop.
    pub fn click(hand: Hand, amplitude: f32) -> Self {
        Self {
            hand,
            amplitude,
            duration: std::time::Duration::from_millis(40),
            frequency: 0.0,
        }
    }
}
