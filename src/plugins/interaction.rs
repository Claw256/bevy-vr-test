//! Picking props up and putting them down.
//!
//! Grabbing is implemented by re-parenting the prop to the controller entity,
//! so the existing transform propagation carries it along — no per-frame
//! "follow the hand" system, and no chance of the prop lagging the hand.

use bevy::prelude::*;
use bevy_mod_openxr::openxr_session_running;
use bevy_mod_openxr::session::OxrSession;

use crate::components::controller::{Controller, ControllerInput, Hand};
use crate::components::interaction::{Grabbable, GrabbableMaterials, Grabbed, InReach};
use crate::messages::HapticPulse;
use crate::plugins::controllers::{despawn_controllers, grab_point};
use crate::plugins::xr_input::XrActions;
use crate::sets::VrSet;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HapticPulse>()
            .add_systems(
                Update,
                // Release first so a prop dropped this frame is immediately a
                // candidate again, then grab, then refresh the highlight from
                // whatever state the first two left behind.
                (release_props, grab_props, update_reach)
                    .chain()
                    .in_set(VrSet::Interaction),
            )
            // A controller `despawn` takes its descendants with it, and a held
            // prop is one of them. Let go before the session tears them down.
            .add_systems(
                bevy_mod_xr::session::XrPreDestroySession,
                release_all_props.before(despawn_controllers),
            )
            .add_systems(
                Update,
                apply_haptics
                    .in_set(VrSet::Feedback)
                    .run_if(openxr_session_running)
                    .run_if(resource_exists::<XrActions>),
            );
    }
}

/// Marks the nearest reachable prop so the player can see what a squeeze would
/// pick up.
fn update_reach(
    controllers: Query<&GlobalTransform, With<Controller>>,
    mut props: Query<
        (
            Entity,
            &Grabbable,
            &GlobalTransform,
            &GrabbableMaterials,
            &mut MeshMaterial3d<StandardMaterial>,
            Has<InReach>,
        ),
        (Without<Grabbed>, Without<Controller>),
    >,
    mut commands: Commands,
) {
    for (entity, grabbable, transform, materials, mut material, in_reach) in &mut props {
        let reachable = controllers
            .iter()
            .any(|hand| hand_distance(hand, transform) <= grabbable.reach);

        if reachable == in_reach {
            continue;
        }

        if reachable {
            commands.entity(entity).insert(InReach);
            material.0 = materials.highlight.clone();
        } else {
            commands.entity(entity).remove::<InReach>();
            material.0 = materials.idle.clone();
        }
    }
}

fn grab_props(
    controllers: Query<(Entity, &Hand, &ControllerInput, &GlobalTransform), With<Controller>>,
    mut props: Query<
        (Entity, &Grabbable, &GlobalTransform, &mut Transform),
        (With<InReach>, Without<Grabbed>, Without<Controller>),
    >,
    mut haptics: MessageWriter<HapticPulse>,
    mut commands: Commands,
) {
    for (controller, hand, input, controller_transform) in &controllers {
        if !input.just_gripped() {
            continue;
        }

        // Nearest wins, so two props inside the grab radius is not ambiguous.
        let nearest = props
            .iter()
            .filter(|(_, grabbable, transform, _)| {
                hand_distance(controller_transform, transform) <= grabbable.reach
            })
            .min_by(|(_, _, a, _), (_, _, b, _)| {
                hand_distance(controller_transform, a)
                    .total_cmp(&hand_distance(controller_transform, b))
            })
            .map(|(entity, ..)| entity);

        let Some(prop) = nearest else { continue };
        let Ok((_, _, prop_transform, mut local)) = props.get_mut(prop) else {
            continue;
        };

        // Re-parenting replaces the meaning of `Transform`, so convert the
        // prop's world pose into the controller's space or it will jump.
        *local = prop_transform.reparented_to(controller_transform);

        commands
            .entity(prop)
            .insert((Grabbed { controller }, ChildOf(controller)));

        haptics.write(HapticPulse::click(*hand, 0.5));
    }
}

fn release_props(
    controllers: Query<(&Hand, &ControllerInput), With<Controller>>,
    mut props: Query<(Entity, &Grabbed, &GlobalTransform, &mut Transform)>,
    mut haptics: MessageWriter<HapticPulse>,
    mut commands: Commands,
) {
    for (prop, grabbed, world, mut local) in &mut props {
        let Ok((hand, input)) = controllers.get(grabbed.controller) else {
            // The controller went away — drop the prop where it stands.
            *local = world.compute_transform();
            commands.entity(prop).remove::<(Grabbed, ChildOf)>();
            continue;
        };

        if !input.just_released() {
            continue;
        }

        // Back to world space, otherwise the prop snaps to the play-space origin.
        *local = world.compute_transform();
        commands.entity(prop).remove::<(Grabbed, ChildOf)>();

        haptics.write(HapticPulse::click(*hand, 0.3));
    }
}

/// Drops every held prop where it stands, without the haptic tick.
fn release_all_props(
    mut props: Query<(Entity, &GlobalTransform, &mut Transform), With<Grabbed>>,
    mut commands: Commands,
) {
    for (prop, world, mut local) in &mut props {
        *local = world.compute_transform();
        commands.entity(prop).remove::<(Grabbed, ChildOf)>();
    }
}

/// Turns [`HapticPulse`] messages into OpenXR feedback.
///
/// The only system in the interaction domain that knows OpenXR exists.
fn apply_haptics(
    actions: Res<XrActions>,
    session: Res<OxrSession>,
    mut pulses: MessageReader<HapticPulse>,
) {
    for pulse in pulses.read() {
        let event = openxr::HapticVibration::new()
            .amplitude(pulse.amplitude.clamp(0.0, 1.0))
            .frequency(pulse.frequency)
            .duration(openxr::Duration::from_nanos(
                pulse.duration.as_nanos().min(i64::MAX as u128) as i64,
            ));

        if let Err(err) = actions
            .haptic
            .apply_feedback(&session, actions.path(pulse.hand), &event)
        {
            warn!("haptic feedback failed: {err}");
        }
    }
}

fn hand_distance(controller: &GlobalTransform, prop: &GlobalTransform) -> f32 {
    grab_point(controller).distance(prop.translation())
}
