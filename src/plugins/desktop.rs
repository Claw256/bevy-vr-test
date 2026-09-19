//! The flat-screen camera.
//!
//! `add_xr_plugins` falls back to ordinary rendering when no OpenXR runtime is
//! present, so this camera does double duty: it is the desktop mirror while a
//! headset is connected, and the only view when one is not. The fly controls
//! are enabled only in the second case, so they cannot fight the headset.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy_mod_xr::session::session_running;

use crate::plugins::settings::settings_menu_open;

pub struct DesktopPlugin;

impl Plugin for DesktopPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera).add_systems(
            Update,
            fly_camera
                // Not "no runtime", but "not currently in a session" — so the
                // controls come back when you switch out of VR at runtime.
                .run_if(not(session_running))
                // Yield to the settings menu, so clicking a row does not also
                // fly the camera.
                .run_if(not(settings_menu_open)),
        );
    }
}

/// Marks the flat camera so the fly controls do not grab an XR eye.
#[derive(Component)]
pub struct DesktopCamera;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("Desktop Camera"),
        DesktopCamera,
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.6, 1.2).looking_at(Vec3::new(0.0, 0.9, -0.6), Vec3::Y),
    ));
}

fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    mut camera: Query<&mut Transform, With<DesktopCamera>>,
) {
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };

    if buttons.pressed(MouseButton::Right) && mouse.delta != Vec2::ZERO {
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        let yaw = yaw - mouse.delta.x * 0.003;
        let pitch = (pitch - mouse.delta.y * 0.003).clamp(-1.54, 1.54);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    }

    let mut motion = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        motion += *transform.forward();
    }
    if keys.pressed(KeyCode::KeyS) {
        motion += *transform.back();
    }
    if keys.pressed(KeyCode::KeyA) {
        motion += *transform.left();
    }
    if keys.pressed(KeyCode::KeyD) {
        motion += *transform.right();
    }
    if keys.pressed(KeyCode::KeyE) {
        motion += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        motion -= Vec3::Y;
    }

    let speed = if keys.pressed(KeyCode::ShiftLeft) { 6.0 } else { 2.5 };
    transform.translation += motion.normalize_or_zero() * speed * time.delta_secs();
}
