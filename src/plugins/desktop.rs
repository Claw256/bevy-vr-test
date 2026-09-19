//! Flat-screen presentation: the desktop camera and the frame-time overlay.
//!
//! `add_xr_plugins` falls back to ordinary rendering when no OpenXR runtime is
//! present, so this camera does double duty: it is the desktop mirror while a
//! headset is connected, and the only view when one is not. The fly controls
//! and the overlay keybind are enabled only in the second case, so they cannot
//! fight the headset.

use std::time::Duration;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy_mod_xr::session::{XrState, state_equals};

/// Toggles the FPS readout and frame-time graph.
pub const OVERLAY_KEY: KeyCode = KeyCode::F3;

pub struct DesktopPlugin;

impl Plugin for DesktopPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FpsOverlayPlugin {
            config: FpsOverlayConfig {
                text_config: TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                text_color: Color::srgb(0.55, 1.0, 0.6),
                // Hidden until F3. Bevy applies this on the first frame, since
                // its toggle runs on `resource_changed` and an inserted
                // resource counts as changed.
                enabled: false,
                refresh_interval: Duration::from_millis(100),
                frame_time_graph_config: FrameTimeGraphConfig {
                    enabled: false,
                    // Headset cadence, not monitor cadence: the point of
                    // watching frame times in a VR project is knowing whether
                    // you would hold 72-90 Hz, even while working flat.
                    min_fps: 72.0,
                    target_fps: 90.0,
                },
            },
        })
        .add_systems(Startup, spawn_camera)
        .add_systems(
            Update,
            (fly_camera, toggle_overlay).run_if(state_equals(XrState::Unavailable)),
        );
    }
}

/// Marks the flat camera so the fly controls do not grab an XR eye.
#[derive(Component)]
struct DesktopCamera;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Name::new("Desktop Camera"),
        DesktopCamera,
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.6, 1.2).looking_at(Vec3::new(0.0, 0.9, -0.6), Vec3::Y),
    ));
}

/// Shows or hides the FPS readout and the frame-time graph together.
///
/// Both flags have to move: Bevy drives the graph's visibility from
/// `frame_time_graph_config.enabled` alone, so flipping only the outer
/// `enabled` would hide the text and leave the graph on screen.
fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut config: ResMut<FpsOverlayConfig>) {
    if keys.just_pressed(OVERLAY_KEY) {
        let showing = !config.enabled;
        config.enabled = showing;
        config.frame_time_graph_config.enabled = showing;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden_overlay_app() -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(FpsOverlayConfig {
                enabled: false,
                frame_time_graph_config: FrameTimeGraphConfig {
                    enabled: false,
                    ..default()
                },
                ..default()
            })
            .add_systems(Update, toggle_overlay);
        app
    }

    /// Press and release, as a real tap does. `ButtonInput::press` only sets
    /// `just_pressed` when the key was not already held, so a helper that never
    /// releases silently stops registering after the first call.
    fn tap(app: &mut App, key: KeyCode) {
        {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.press(key);
        }
        app.update();
        {
            let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            input.release(key);
            input.clear();
        }
        app.update();
    }

    #[test]
    fn the_key_shows_readout_and_graph_together() {
        let mut app = hidden_overlay_app();
        tap(&mut app, OVERLAY_KEY);

        let config = app.world().resource::<FpsOverlayConfig>();
        assert!(config.enabled, "FPS readout should be visible");
        // Bevy drives the graph from its own flag, so a toggle that moved only
        // `enabled` would show the number and leave the graph hidden.
        assert!(
            config.frame_time_graph_config.enabled,
            "frame-time graph should be visible"
        );
    }

    #[test]
    fn pressing_again_hides_both() {
        let mut app = hidden_overlay_app();
        tap(&mut app, OVERLAY_KEY);
        tap(&mut app, OVERLAY_KEY);

        let config = app.world().resource::<FpsOverlayConfig>();
        assert!(!config.enabled);
        assert!(!config.frame_time_graph_config.enabled);
    }

    #[test]
    fn other_keys_leave_it_alone() {
        let mut app = hidden_overlay_app();
        tap(&mut app, KeyCode::KeyW);

        assert!(!app.world().resource::<FpsOverlayConfig>().enabled);
    }
}
