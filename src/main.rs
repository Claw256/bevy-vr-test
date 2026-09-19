//! A starter 3D VR application: Bevy 0.19 on OpenXR via `bevy_mod_openxr`.
//!
//! Runs with a headset if an OpenXR runtime is available, and falls back to an
//! ordinary desktop window if not — so it is testable either way.

mod components;
mod messages;
mod plugins;
mod sets;

use bevy::prelude::*;
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_mod_openxr::add_xr_plugins;

use plugins::controllers::ControllersPlugin;
use plugins::desktop::DesktopPlugin;
use plugins::interaction::InteractionPlugin;
use plugins::locomotion::LocomotionPlugin;
use plugins::pacing::FramePacePlugin;
use plugins::physics::GamePhysicsPlugin;
use plugins::quality::RenderQualityPlugin;
use plugins::settings::SettingsMenuPlugin;
use plugins::world::WorldPlugin;
use plugins::xr_input::XrInputPlugin;
use sets::VrSet;

fn main() -> AppExit {
    let mut app = App::new();

    app
        .add_plugins(add_xr_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Bevy VR Starter".into(),
                        ..default()
                    }),
                    ..default()
                })
                .build()
                // Pipelined rendering buys throughput at the cost of a frame of
                // latency. In VR that latency is felt directly, and poses are
                // sampled late on purpose, so it is disabled here.
                .disable::<PipelinedRenderingPlugin>(),
        ))
        // The mirror window is usually unfocused while the headset is on, and
        // winit throttles unfocused apps by default. That throttle would stall
        // the XR frame loop, so keep updating regardless of focus.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .configure_sets(
            Update,
            (VrSet::Locomotion, VrSet::Interaction, VrSet::Feedback).chain(),
        )
        .add_plugins((
            RenderQualityPlugin,
            FramePacePlugin,
            GamePhysicsPlugin,
            WorldPlugin,
            DesktopPlugin,
            SettingsMenuPlugin,
            XrInputPlugin,
            ControllersPlugin,
            LocomotionPlugin,
            InteractionPlugin,
        ));

    // Debug instrumentation: redraws 26 joints per hand every frame. Dev builds
    // only — it is not worth the cost in a shipping VR frame.
    if cfg!(debug_assertions) {
        app.add_plugins(bevy_mod_xr::hand_debug_gizmos::HandGizmosPlugin);
    }

    app.run()
}

