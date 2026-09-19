//! Physics, via [Avian](https://github.com/avianphysics/avian).
//!
//! Thin on purpose: the plugin group and the tuning that the rest of the app
//! depends on. What each body and collider is lives next to the thing it
//! belongs to — the scene in `world.rs`, grabbing in `interaction.rs`.

use avian3d::prelude::*;
use bevy::prelude::*;

pub struct GamePhysicsPlugin;

impl Plugin for GamePhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default());
    }
}

/// How a grabbed prop is carried, and how hard it can be thrown.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GrabTuning {
    /// Speed cap applied to a thrown prop, in metres per second. Hand tracking
    /// jitter can produce a huge one-frame delta; without a cap a prop can
    /// leave the room on release.
    pub max_throw_speed: f32,
    /// The same, for spin, in radians per second.
    pub max_throw_spin: f32,
}

impl Default for GrabTuning {
    fn default() -> Self {
        Self {
            max_throw_speed: 8.0,
            max_throw_spin: 20.0,
        }
    }
}
