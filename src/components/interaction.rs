//! Components for picking things up.

use bevy::prelude::*;

/// A prop a controller can pick up.
#[derive(Component, Clone, Copy, Debug)]
pub struct Grabbable {
    /// How close a controller's grip has to be, in metres.
    pub reach: f32,
}

impl Default for Grabbable {
    fn default() -> Self {
        Self { reach: 0.12 }
    }
}

/// Present on a [`Grabbable`] while it is held, pointing at the holding
/// controller. Its absence is what makes a prop eligible to be grabbed, so
/// queries filter on `Without<Grabbed>` rather than on a boolean field.
#[derive(Component, Clone, Copy, Debug)]
pub struct Grabbed {
    pub controller: Entity,
}

/// Present on a [`Grabbable`] while a controller is close enough to take it.
#[derive(Component, Clone, Copy, Debug)]
pub struct InReach;

/// The two materials a [`Grabbable`] swaps between.
///
/// Swapping handles is cheaper than mutating a `StandardMaterial`, and avoids
/// the classic mistake of editing an asset that several entities share.
#[derive(Component, Clone, Debug)]
pub struct GrabbableMaterials {
    pub idle: Handle<StandardMaterial>,
    pub highlight: Handle<StandardMaterial>,
}
