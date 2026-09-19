//! Thumbstick locomotion: smooth movement on the left stick, snap turning on
//! the right.
//!
//! Both work by moving the `XrTrackingRoot`, the entity every tracked thing is
//! parented to. The headset keeps reporting poses relative to the player's real
//! room; the root is what places that room in the game world.
//!
//! Turning snaps rather than rotating smoothly on purpose: continuous rotation
//! that the inner ear cannot feel is the fastest way to make people ill.

use bevy::prelude::*;
use bevy_mod_openxr::openxr_session_running;
use bevy_mod_xr::camera::XrCamera;
use bevy_mod_xr::session::XrTrackingRoot;

use crate::components::controller::{ControllerInput, Hand};
use crate::sets::VrSet;

pub struct LocomotionPlugin;

impl Plugin for LocomotionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocomotionSettings>().add_systems(
            Update,
            (smooth_locomotion, snap_turn)
                .chain()
                .in_set(VrSet::Locomotion)
                .run_if(openxr_session_running),
        );
    }
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct LocomotionSettings {
    /// Metres per second at full stick deflection.
    pub speed: f32,
    /// How far one snap turns, in radians.
    pub snap_angle: f32,
    /// Stick deflection below this is ignored — sticks rarely rest at zero.
    pub deadzone: f32,
    /// Deflection at which a snap fires, and below which it re-arms.
    pub snap_threshold: f32,
}

impl Default for LocomotionSettings {
    fn default() -> Self {
        Self {
            speed: 2.0,
            snap_angle: std::f32::consts::FRAC_PI_4,
            deadzone: 0.15,
            snap_threshold: 0.6,
        }
    }
}

/// The head pose, flattened to the horizontal plane.
///
/// Movement follows where the player is *looking*, not where the play space
/// happens to be oriented, so both systems need this.
fn head_yaw_basis(heads: &Query<(&XrCamera, &GlobalTransform)>) -> Option<(Vec3, Vec3)> {
    let (_, head) = heads.iter().find(|(camera, _)| camera.0 == 0)?;
    let forward = head.forward();
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    if flat == Vec3::ZERO {
        // Looking straight up or down: no usable heading this frame.
        return None;
    }
    Some((flat, flat.cross(Vec3::Y)))
}

fn smooth_locomotion(
    settings: Res<LocomotionSettings>,
    time: Res<Time>,
    controllers: Query<(&Hand, &ControllerInput)>,
    heads: Query<(&XrCamera, &GlobalTransform)>,
    mut root: Query<&mut Transform, With<XrTrackingRoot>>,
) {
    let Ok(mut root) = root.single_mut() else {
        return;
    };
    let Some((forward, right)) = head_yaw_basis(&heads) else {
        return;
    };

    for (hand, input) in &controllers {
        if *hand != Hand::Left || !input.active {
            continue;
        }
        if input.stick.length() < settings.deadzone {
            continue;
        }

        let motion = forward * input.stick.y + right * input.stick.x;
        root.translation += motion.clamp_length_max(1.0) * settings.speed * time.delta_secs();
    }
}

fn snap_turn(
    settings: Res<LocomotionSettings>,
    controllers: Query<(&Hand, &ControllerInput)>,
    heads: Query<(&XrCamera, &GlobalTransform)>,
    mut root: Query<&mut Transform, With<XrTrackingRoot>>,
    // Latched so that holding the stick over turns once, not once per frame.
    mut armed: Local<bool>,
) {
    let Ok(mut root) = root.single_mut() else {
        return;
    };

    for (hand, input) in &controllers {
        if *hand != Hand::Right || !input.active {
            continue;
        }

        let deflection = input.stick.x;
        if deflection.abs() < settings.snap_threshold {
            *armed = true;
            continue;
        }
        if !*armed {
            continue;
        }
        *armed = false;

        let rotation = Quat::from_rotation_y(-deflection.signum() * settings.snap_angle);

        // Rotate about the head rather than the root's origin. Turning about
        // the origin would swing the player sideways through the world.
        let pivot = heads
            .iter()
            .find(|(camera, _)| camera.0 == 0)
            .map(|(_, head)| head.translation())
            .unwrap_or(root.translation);

        root.translation = pivot + rotation * (root.translation - pivot);
        root.rotation = rotation * root.rotation;
    }
}
