//! Components describing a tracked controller and the state of its inputs.

use bevy::prelude::*;

/// Which hand an entity is tracking.
///
/// The variants map onto the OpenXR *subaction paths* that let a single action
/// (e.g. "squeeze") be queried per hand.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hand {
    Left,
    Right,
}

impl Hand {
    pub const ALL: [Hand; 2] = [Hand::Left, Hand::Right];

    pub const fn subaction_path(self) -> &'static str {
        match self {
            Hand::Left => "/user/hand/left",
            Hand::Right => "/user/hand/right",
        }
    }
}

/// Marker for an entity whose transform follows a controller's grip pose.
#[derive(Component, Clone, Copy, Debug)]
#[require(ControllerInput)]
pub struct Controller;

/// This frame's snapshot of one controller's inputs.
///
/// Written once per frame by `read_controller_input`; every other system reads
/// it instead of talking to OpenXR directly.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ControllerInput {
    /// Grip squeeze, 0.0..=1.0.
    pub squeeze: f32,
    /// Index trigger, 0.0..=1.0.
    pub trigger: f32,
    /// Thumbstick, each axis -1.0..=1.0.
    pub stick: Vec2,
    /// Whether the runtime currently has a pose for this hand.
    pub active: bool,
    /// Squeeze is past [`ControllerInput::GRAB_THRESHOLD`] this frame.
    pub gripping: bool,
    /// ...and whether it was last frame, so edges can be detected.
    pub was_gripping: bool,
}

impl ControllerInput {
    /// Squeeze value at which a grab starts.
    pub const GRAB_THRESHOLD: f32 = 0.55;

    pub fn just_gripped(&self) -> bool {
        self.gripping && !self.was_gripping
    }

    pub fn just_released(&self) -> bool {
        !self.gripping && self.was_gripping
    }
}
