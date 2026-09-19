//! Components for picking things up.

use avian3d::prelude::*;
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

/// Present on a [`Grabbable`] while it is held. Its absence is what makes a
/// prop eligible to be grabbed, so queries filter on `Without<Grabbed>` rather
/// than on a boolean field.
///
/// A held prop is switched to [`RigidBody::Kinematic`] and driven from the
/// holder's pose, rather than parented to it. Parenting would leave the solver
/// and the transform hierarchy both writing the same body.
#[derive(Component, Clone, Copy, Debug)]
pub struct Grabbed {
    /// The controller (in VR) or camera (on the desktop) carrying this prop.
    pub holder: Entity,
    /// The prop's pose in the holder's local space, captured on pick-up.
    pub local: Transform,
    /// Body type to restore on release.
    pub restore: RigidBody,
    /// Motion over the last frame, so a released prop keeps the speed the hand
    /// gave it instead of dropping straight down.
    pub linear: Vec3,
    pub angular: Vec3,
    pub previous: Option<(Vec3, Quat)>,
}

impl Grabbed {
    pub fn new(holder: Entity, local: Transform, restore: RigidBody) -> Self {
        Self {
            holder,
            local,
            restore,
            linear: Vec3::ZERO,
            angular: Vec3::ZERO,
            previous: None,
        }
    }
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
