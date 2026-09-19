//! Picking props up and putting them down, in VR and on the desktop.
//!
//! A held prop is switched to [`RigidBody::Kinematic`] and driven from the
//! holder's pose each frame. The earlier version parented the prop to the
//! controller and let transform propagation carry it; with a physics engine in
//! the world that no longer works, because the solver and the hierarchy would
//! both be writing the same body's transform.
//!
//! Kinematic carry also buys throwing for free: the carry system already knows
//! how far the prop moved last frame, so releasing it just hands that velocity
//! to the solver.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_mod_openxr::openxr_session_running;
use bevy_mod_openxr::session::OxrSession;
use bevy_mod_xr::session::session_running;

use crate::components::controller::{Controller, ControllerInput, Hand};
use crate::components::interaction::{Grabbable, GrabbableMaterials, Grabbed, InReach};
use crate::messages::HapticPulse;
use crate::plugins::controllers::grab_point;
use crate::plugins::desktop::DesktopCamera;
use crate::plugins::physics::GrabTuning;
use crate::plugins::settings::settings_menu_open;
use crate::plugins::xr_input::XrActions;
use crate::sets::VrSet;

/// How far the desktop cursor ray reaches, in metres.
const DESKTOP_REACH: f32 = 8.0;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HapticPulse>()
            .init_resource::<GrabTuning>()
            .add_systems(
                Update,
                // Release first so a prop dropped this frame is a candidate
                // again, then grab, then carry whatever is now held, then
                // refresh the highlight from the state the rest left behind.
                (
                    (vr_release, vr_grab)
                        .chain()
                        .run_if(session_running),
                    (desktop_click)
                        .run_if(not(session_running))
                        .run_if(not(settings_menu_open)),
                    carry_held,
                    mark_reach_vr.run_if(session_running),
                    mark_reach_desktop.run_if(not(session_running)),
                    sync_highlight,
                )
                    .chain()
                    .in_set(VrSet::Interaction),
            )
            // A controller `despawn` would otherwise leave a prop kinematic and
            // held by an entity that no longer exists.
            .add_systems(
                bevy_mod_xr::session::XrPreDestroySession,
                release_everything.before(crate::plugins::controllers::despawn_controllers),
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

// ---------------------------------------------------------------- VR grabbing

fn vr_grab(
    controllers: Query<(Entity, &Hand, &ControllerInput, &GlobalTransform), With<Controller>>,
    props: Query<
        (Entity, &Grabbable, &GlobalTransform, &RigidBody),
        (With<InReach>, Without<Grabbed>, Without<Controller>),
    >,
    mut haptics: MessageWriter<HapticPulse>,
    mut commands: Commands,
) {
    for (controller, hand, input, controller_at) in &controllers {
        if !input.just_gripped() {
            continue;
        }

        // Nearest wins, so two props inside the grab radius is not ambiguous.
        let nearest = props
            .iter()
            .filter(|(_, grabbable, at, _)| reach_distance(controller_at, at) <= grabbable.reach)
            .min_by(|(_, _, a, _), (_, _, b, _)| {
                reach_distance(controller_at, a).total_cmp(&reach_distance(controller_at, b))
            });

        let Some((prop, _, prop_at, body)) = nearest else {
            continue;
        };

        commands
            .entity(prop)
            .insert((hold(controller, controller_at, prop_at, *body), RigidBody::Kinematic));

        haptics.write(HapticPulse::click(*hand, 0.5));
    }
}

fn vr_release(
    controllers: Query<(&Hand, &ControllerInput), With<Controller>>,
    held: Query<(Entity, &Grabbed)>,
    tuning: Res<GrabTuning>,
    mut haptics: MessageWriter<HapticPulse>,
    mut commands: Commands,
) {
    for (prop, grabbed) in &held {
        let Ok((hand, input)) = controllers.get(grabbed.holder) else {
            continue;
        };
        if !input.just_released() {
            continue;
        }
        throw(&mut commands, prop, grabbed, &tuning);
        haptics.write(HapticPulse::click(*hand, 0.3));
    }
}

// ----------------------------------------------------------- desktop grabbing

/// Left click picks up whatever the cursor is over, and clicking again drops it.
fn desktop_click(
    buttons: Res<ButtonInput<MouseButton>>,
    camera: Query<(Entity, &Camera, &GlobalTransform), With<DesktopCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    spatial: SpatialQuery,
    props: Query<(&GlobalTransform, &RigidBody), (With<Grabbable>, Without<Grabbed>)>,
    held: Query<(Entity, &Grabbed)>,
    tuning: Res<GrabTuning>,
    mut commands: Commands,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    // Holding something? Then a click is a drop, wherever the cursor is.
    if let Some((prop, grabbed)) = held.iter().next() {
        throw(&mut commands, prop, grabbed, &tuning);
        return;
    }

    let Some((camera_entity, camera_at, ray)) = cursor_ray(&camera, &windows) else {
        return;
    };
    let Some(hit) = spatial.cast_ray(
        ray.origin,
        ray.direction,
        DESKTOP_REACH,
        true,
        &SpatialQueryFilter::default(),
    ) else {
        return;
    };
    // The ray stops at the first solid thing, so you cannot grab through the
    // table — if that is not a prop, the click simply misses.
    let Ok((prop_at, body)) = props.get(hit.entity) else {
        return;
    };

    commands.entity(hit.entity).insert((
        hold(camera_entity, camera_at, prop_at, *body),
        RigidBody::Kinematic,
    ));
}

/// The world-space ray under the mouse cursor, if there is one.
fn cursor_ray<'a>(
    camera: &'a Query<(Entity, &Camera, &GlobalTransform), With<DesktopCamera>>,
    windows: &Query<&Window, With<PrimaryWindow>>,
) -> Option<(Entity, &'a GlobalTransform, Ray3d)> {
    let (entity, camera, camera_at) = camera.iter().next()?;
    let cursor = windows.iter().next()?.cursor_position()?;
    let ray = camera.viewport_to_world(camera_at, cursor).ok()?;
    Some((entity, camera_at, ray))
}

// -------------------------------------------------------------- shared carry

fn hold(
    holder: Entity,
    holder_at: &GlobalTransform,
    prop_at: &GlobalTransform,
    body: RigidBody,
) -> Grabbed {
    Grabbed::new(holder, prop_at.reparented_to(holder_at), body)
}

/// Drives every held prop from its holder, and records how fast it is moving.
///
/// Writes `Transform`, not `Position`. Avian runs in `FixedPostUpdate`, which
/// is earlier in the frame than `Update`, so a `Position` written here would
/// not reach `Transform` until the next fixed step — and fixed steps do not
/// happen every frame, so the prop would visibly stutter in the hand. Writing
/// `Transform` renders correctly this frame, and Avian's `transform_to_position`
/// picks it up before the next step.
///
/// The velocities are kept live too, so a carried prop shoves dynamic bodies
/// it collides with instead of sliding through them at an implied standstill.
fn carry_held(
    time: Res<Time>,
    holders: Query<&GlobalTransform>,
    mut held: Query<(
        &mut Grabbed,
        &mut Transform,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let dt = time.delta_secs();

    for (mut grabbed, mut transform, mut linear, mut angular) in &mut held {
        let Ok(holder_at) = holders.get(grabbed.holder) else {
            continue;
        };

        let target = holder_at.mul_transform(grabbed.local);
        let (_, next_rotation, next_translation) = target.to_scale_rotation_translation();

        if dt > 0.0 {
            if let Some((previous_translation, previous_rotation)) = grabbed.previous {
                grabbed.linear = (next_translation - previous_translation) / dt;

                // Spin is the rotation from last frame to this one, as an axis
                // scaled by radians per second.
                let delta = next_rotation * previous_rotation.inverse();
                let (axis, angle) = delta.to_axis_angle();
                grabbed.angular = axis * (angle / dt);
            }
            grabbed.previous = Some((next_translation, next_rotation));
        }

        transform.translation = next_translation;
        transform.rotation = next_rotation;
        linear.0 = grabbed.linear;
        angular.0 = grabbed.angular;
    }
}

/// Hands a held prop back to the solver with the speed it was moving at.
fn throw(commands: &mut Commands, prop: Entity, grabbed: &Grabbed, tuning: &GrabTuning) {
    commands.entity(prop).remove::<Grabbed>().insert((
        grabbed.restore,
        LinearVelocity(grabbed.linear.clamp_length_max(tuning.max_throw_speed)),
        AngularVelocity(grabbed.angular.clamp_length_max(tuning.max_throw_spin)),
    ));
}

/// Drops everything, without the haptic tick.
fn release_everything(
    held: Query<(Entity, &Grabbed)>,
    tuning: Res<GrabTuning>,
    mut commands: Commands,
) {
    for (prop, grabbed) in &held {
        throw(&mut commands, prop, grabbed, &tuning);
    }
}

// ------------------------------------------------------------------ highlight

fn mark_reach_vr(
    controllers: Query<&GlobalTransform, With<Controller>>,
    props: Query<
        (Entity, &Grabbable, &GlobalTransform, Has<InReach>),
        (Without<Grabbed>, Without<Controller>),
    >,
    mut commands: Commands,
) {
    for (prop, grabbable, at, in_reach) in &props {
        let reachable = controllers
            .iter()
            .any(|hand| reach_distance(hand, at) <= grabbable.reach);
        set_reach(&mut commands, prop, reachable, in_reach);
    }
}

fn mark_reach_desktop(
    camera: Query<(Entity, &Camera, &GlobalTransform), With<DesktopCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    spatial: SpatialQuery,
    props: Query<(Entity, Has<InReach>), (With<Grabbable>, Without<Grabbed>)>,
    mut commands: Commands,
) {
    let under_cursor = cursor_ray(&camera, &windows).and_then(|(_, _, ray)| {
        spatial
            .cast_ray(
                ray.origin,
                ray.direction,
                DESKTOP_REACH,
                true,
                &SpatialQueryFilter::default(),
            )
            .map(|hit| hit.entity)
    });

    for (prop, in_reach) in &props {
        set_reach(&mut commands, prop, under_cursor == Some(prop), in_reach);
    }
}

fn set_reach(commands: &mut Commands, prop: Entity, reachable: bool, in_reach: bool) {
    if reachable == in_reach {
        return;
    }
    if reachable {
        commands.entity(prop).insert(InReach);
    } else {
        commands.entity(prop).remove::<InReach>();
    }
}

/// Swaps the material handle rather than editing the material, which several
/// props share.
fn sync_highlight(
    mut props: Query<
        (
            &GrabbableMaterials,
            &mut MeshMaterial3d<StandardMaterial>,
            Has<InReach>,
        ),
        Changed<InReach>,
    >,
) {
    for (materials, mut material, in_reach) in &mut props {
        material.0 = if in_reach {
            materials.highlight.clone()
        } else {
            materials.idle.clone()
        };
    }
}

// ------------------------------------------------------------------- haptics

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

fn reach_distance(controller: &GlobalTransform, prop: &GlobalTransform) -> f32 {
    grab_point(controller).distance(prop.translation())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    const DT: f32 = 0.1;

    /// A world with just the carry systems and a fixed, non-advancing clock, so
    /// velocity estimates are exact instead of frame-rate dependent.
    fn carry_app() -> App {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_secs_f32(DT));
        app.insert_resource(time)
            .init_resource::<GrabTuning>()
            .add_systems(Update, carry_held);
        app
    }

    fn spawn_holder(app: &mut App, at: Vec3) -> Entity {
        app.world_mut()
            .spawn(GlobalTransform::from(Transform::from_translation(at)))
            .id()
    }

    fn spawn_held(app: &mut App, holder: Entity, local: Transform) -> Entity {
        app.world_mut()
            .spawn((
                Grabbed::new(holder, local, RigidBody::Dynamic),
                Transform::default(),
                LinearVelocity::default(),
                AngularVelocity::default(),
            ))
            .id()
    }

    fn move_holder(app: &mut App, holder: Entity, to: Vec3) {
        app.world_mut()
            .entity_mut(holder)
            .insert(GlobalTransform::from(Transform::from_translation(to)));
    }

    #[test]
    fn a_held_prop_sits_at_its_offset_from_the_holder() {
        let mut app = carry_app();
        let holder = spawn_holder(&mut app, Vec3::new(0.0, 1.0, 0.0));
        let prop = spawn_held(&mut app, holder, Transform::from_xyz(0.0, 0.0, -0.5));

        app.update();

        let at = app.world().entity(prop).get::<Transform>().unwrap();
        assert_eq!(at.translation, Vec3::new(0.0, 1.0, -0.5));
    }

    #[test]
    fn carrying_tracks_the_holder_and_measures_its_speed() {
        let mut app = carry_app();
        let holder = spawn_holder(&mut app, Vec3::ZERO);
        let prop = spawn_held(&mut app, holder, Transform::IDENTITY);

        // First frame only seeds the previous pose, so speed is still zero.
        app.update();
        assert_eq!(
            app.world().entity(prop).get::<Grabbed>().unwrap().linear,
            Vec3::ZERO
        );

        move_holder(&mut app, holder, Vec3::new(1.0, 0.0, 0.0));
        app.update();

        let prop_ref = app.world().entity(prop);
        assert_eq!(
            prop_ref.get::<Transform>().unwrap().translation,
            Vec3::new(1.0, 0.0, 0.0)
        );
        // One metre in DT seconds.
        assert_eq!(
            prop_ref.get::<Grabbed>().unwrap().linear,
            Vec3::new(1.0 / DT, 0.0, 0.0)
        );
        // The live velocity is published too, so collisions behave.
        assert_eq!(prop_ref.get::<LinearVelocity>().unwrap().0.x, 1.0 / DT);
    }

    /// `release_everything` is teardown, not a per-frame system, so it is run
    /// once by hand after the carry frames rather than registered in `Update`.
    fn release(app: &mut App) {
        app.world_mut().run_system_once(release_everything).unwrap();
    }

    #[test]
    fn releasing_restores_the_body_and_throws_it() {
        let mut app = carry_app();
        let holder = spawn_holder(&mut app, Vec3::ZERO);
        let prop = spawn_held(&mut app, holder, Transform::IDENTITY);

        app.update();
        move_holder(&mut app, holder, Vec3::new(0.5, 0.0, 0.0));
        app.update();
        release(&mut app);

        let prop_ref = app.world().entity(prop);
        assert!(
            prop_ref.get::<Grabbed>().is_none(),
            "prop should no longer be held"
        );
        assert_eq!(
            prop_ref.get::<RigidBody>().copied(),
            Some(RigidBody::Dynamic),
            "the body type it had before the grab should come back"
        );
        // 0.5m over DT is 5 m/s, under the default cap.
        assert_eq!(prop_ref.get::<LinearVelocity>().unwrap().0.x, 5.0);
    }

    #[test]
    fn a_violent_release_is_capped() {
        let mut app = carry_app();
        let cap = app.world().resource::<GrabTuning>().max_throw_speed;

        let holder = spawn_holder(&mut app, Vec3::ZERO);
        let prop = spawn_held(&mut app, holder, Transform::IDENTITY);

        app.update();
        // A 50 m jump in one frame is what tracking jitter can look like; without
        // the cap the prop would leave at 500 m/s.
        move_holder(&mut app, holder, Vec3::new(50.0, 0.0, 0.0));
        app.update();
        release(&mut app);

        let thrown = app.world().entity(prop).get::<LinearVelocity>().unwrap().0;
        assert!(
            (thrown.length() - cap).abs() < 1e-3,
            "expected the throw clamped to {cap}, got {}",
            thrown.length()
        );
    }
}
