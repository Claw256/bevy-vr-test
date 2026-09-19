//! The frame pipeline for VR-specific systems.
//!
//! Ordering matters more in VR than on a flat screen: the pose and button state
//! read from OpenXR is only valid after the runtime has synced the action set,
//! and anything that moves the player must settle before gameplay reacts to it.

use bevy::prelude::*;

/// Runs in [`PreUpdate`], around `OxrActionSetSyncSet`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VrInputSet {
    /// Tell the backend which action sets to sync. Must run *before* the sync.
    RequestSync,
    /// Copy the freshly synced OpenXR action state into components. Must run
    /// *after* the sync.
    Read,
}

/// Runs in [`Update`], in this order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VrSet {
    /// Move the tracking root (the player's "play space" in world coordinates).
    Locomotion,
    /// React to the input: grabbing, releasing, highlighting.
    Interaction,
    /// Send output back to the hardware — haptics — and refresh the UI.
    Feedback,
}
