//! Spawns one entity per controller, tracking its grip pose.
//!
//! An entity carrying an [`XrSpace`] is located by the backend every frame and
//! its `Transform` written for us. `XrSpace` also requires `XrTracker`, whose
//! component hook parents the entity to the `XrTrackingRoot` — so controller
//! transforms are relative to the play space, and moving the root moves them.

use bevy::prelude::*;
use bevy_mod_openxr::session::OxrSession;
use bevy_mod_xr::session::{XrPreDestroySession, XrSessionCreated};

use crate::components::controller::{Controller, Hand};
use crate::plugins::xr_input::XrActions;

pub struct ControllersPlugin;

impl Plugin for ControllersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            XrSessionCreated,
            spawn_controllers.run_if(resource_exists::<XrActions>),
        )
        // Spaces belong to the session that created them. When it goes away the
        // handles dangle, so the entities holding them have to go too.
        // `InteractionPlugin` orders its cleanup before this one.
        .add_systems(XrPreDestroySession, despawn_controllers);
    }
}

fn spawn_controllers(
    actions: Res<XrActions>,
    session: Res<OxrSession>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Cuboid::new(0.045, 0.045, 0.12));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.17, 0.2),
        perceptual_roughness: 0.6,
        ..default()
    });

    for hand in Hand::ALL {
        // `IDENTITY` puts the space exactly on the grip pose; a non-identity
        // pose here would offset the tracked point along the controller.
        let space = match session.create_action_space(&actions.pose, actions.path(hand), Isometry3d::IDENTITY) {
            Ok(space) => space,
            Err(err) => {
                error!("could not create action space for {hand:?} hand: {err}");
                continue;
            }
        };

        commands.spawn((
            Name::new(format!("{hand:?} Controller")),
            Controller,
            hand,
            space,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
        ));
    }
}

pub fn despawn_controllers(controllers: Query<Entity, With<Controller>>, mut commands: Commands) {
    for entity in &controllers {
        commands.entity(entity).despawn();
    }
}

/// Convenience for other plugins: the world-space point a controller grabs from.
///
/// Kept here so the offset from the grip pose is defined in one place.
pub fn grab_point(controller: &GlobalTransform) -> Vec3 {
    controller.translation() + *controller.forward() * 0.04
}
