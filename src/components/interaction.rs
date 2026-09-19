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

/// How a held prop is driven.
///
/// The two modes follow the *input device*, not the player. In VR that is the
/// hand; on the desktop it is the mouse cursor. Carrying relative to the camera
/// instead would weld the prop to the view, so it could only be moved by
/// walking, and could only be thrown by moving the player.
#[derive(Clone, Copy, Debug)]
pub enum Carry {
    /// Locked to a holder entity's pose — a VR controller. The prop keeps the
    /// offset it had on pick-up, so it tracks the hand exactly.
    Holder { holder: Entity, local: Transform },
    /// Floating on the cursor ray, a fixed distance out from the camera. The
    /// mouse moves the prop; the camera only moves it by moving the ray origin.
    Cursor {
        /// Distance along the ray, fixed at pick-up.
        distance: f32,
        /// World rotation, held steady while carried. The solver takes over
        /// again on release.
        rotation: Quat,
    },
}

/// Present on a [`Grabbable`] while it is held. Its absence is what makes a
/// prop eligible to be grabbed, so queries filter on `Without<Grabbed>` rather
/// than on a boolean field.
///
/// A held prop is switched to [`RigidBody::Kinematic`] and driven by [`Carry`],
/// rather than parented to the holder. Parenting would leave the solver and the
/// transform hierarchy both writing the same body.
#[derive(Component, Clone, Copy, Debug)]
pub struct Grabbed {
    pub carry: Carry,
    /// Body type to restore on release.
    pub restore: RigidBody,
    /// Motion over the last frame, so a released prop keeps the speed it was
    /// given instead of dropping straight down.
    pub linear: Vec3,
    pub angular: Vec3,
    pub previous: Option<(Vec3, Quat)>,
}

impl Grabbed {
    pub fn new(carry: Carry, restore: RigidBody) -> Self {
        Self {
            carry,
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
