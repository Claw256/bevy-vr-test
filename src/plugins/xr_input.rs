//! Owns the OpenXR action set and turns it into plain Bevy components.
//!
//! Everything OpenXR-specific about *input* lives here. The rest of the app
//! reads [`ControllerInput`] and never touches an `openxr::Action`.
//!
//! The lifecycle an OpenXR action set goes through is fixed, and each step maps
//! to a schedule:
//!
//! 1. `Startup`             — create the action set and its actions.
//! 2. `OxrSendActionBindings` — suggest which physical controls they bind to.
//! 3. `XrSessionCreated`    — attach the set to the session (after this point
//!    the set is frozen; no more actions or bindings).
//! 4. `PreUpdate`           — sync the set, then read the state it produced.

use std::borrow::Cow;

use bevy::prelude::*;
use bevy_mod_openxr::action_binding::{OxrSendActionBindings, OxrSuggestActionBinding};
use bevy_mod_openxr::action_set_attaching::OxrAttachActionSet;
use bevy_mod_openxr::action_set_syncing::{OxrActionSetSyncSet, OxrSyncActionSet};
use bevy_mod_openxr::openxr_session_running;
use bevy_mod_openxr::resources::OxrInstance;
use bevy_mod_openxr::session::OxrSession;

use crate::components::controller::{ControllerInput, Hand};
use crate::sets::VrInputSet;

pub struct XrInputPlugin;

impl Plugin for XrInputPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PreUpdate,
            (
                VrInputSet::RequestSync.before(OxrActionSetSyncSet),
                VrInputSet::Read.after(OxrActionSetSyncSet),
            ),
        )
        // `openxr_session_available` is false when no runtime was found, which
        // is the normal case on a machine with no headset attached.
        .add_systems(
            Startup,
            create_actions.run_if(bevy_mod_openxr::openxr_session_available),
        )
        .add_systems(
            OxrSendActionBindings,
            suggest_bindings.run_if(resource_exists::<XrActions>),
        )
        .add_systems(
            bevy_mod_xr::session::XrSessionCreated,
            attach_actions.run_if(resource_exists::<XrActions>),
        )
        .add_systems(
            PreUpdate,
            (
                request_action_sync.in_set(VrInputSet::RequestSync),
                read_controller_input.in_set(VrInputSet::Read),
            )
                .run_if(openxr_session_running)
                .run_if(resource_exists::<XrActions>),
        );
    }
}

/// The application's single action set and the actions inside it.
///
/// Each action is declared once and queried per hand through a *subaction
/// path*, which is why there is one `squeeze` rather than a left and a right.
#[derive(Resource)]
pub struct XrActions {
    pub set: openxr::ActionSet,
    pub pose: openxr::Action<openxr::Posef>,
    pub squeeze: openxr::Action<f32>,
    pub trigger: openxr::Action<f32>,
    pub stick: openxr::Action<openxr::Vector2f>,
    pub haptic: openxr::Action<openxr::Haptic>,
    left: openxr::Path,
    right: openxr::Path,
}

impl XrActions {
    pub fn path(&self, hand: Hand) -> openxr::Path {
        match hand {
            Hand::Left => self.left,
            Hand::Right => self.right,
        }
    }
}

fn create_actions(instance: Res<OxrInstance>, mut commands: Commands) {
    match build_actions(&instance) {
        Ok(actions) => {
            info!("created OpenXR action set");
            commands.insert_resource(actions);
        }
        Err(err) => error!("failed to create OpenXR actions: {err}"),
    }
}

fn build_actions(instance: &OxrInstance) -> openxr::Result<XrActions> {
    let left = instance.string_to_path(Hand::Left.subaction_path())?;
    let right = instance.string_to_path(Hand::Right.subaction_path())?;
    let hands = [left, right];

    let set = instance.create_action_set("gameplay", "Gameplay", 0)?;

    Ok(XrActions {
        pose: set.create_action::<openxr::Posef>("hand_pose", "Hand Pose", &hands)?,
        squeeze: set.create_action::<f32>("squeeze", "Grip Squeeze", &hands)?,
        trigger: set.create_action::<f32>("trigger", "Trigger", &hands)?,
        stick: set.create_action::<openxr::Vector2f>("stick", "Thumbstick", &hands)?,
        haptic: set.create_action::<openxr::Haptic>("haptic", "Haptic Feedback", &hands)?,
        set,
        left,
        right,
    })
}

/// Where each action sits on a given class of controller.
///
/// Paths are suffixes appended to a hand's subaction path. A runtime ignores
/// profiles it does not know, so listing several costs nothing and is what
/// makes the app work on hardware you have not tested on.
struct InteractionProfile {
    path: &'static str,
    pose: &'static str,
    squeeze: &'static str,
    trigger: &'static str,
    /// `None` for controllers without a stick or pad.
    stick: Option<&'static str>,
    haptic: &'static str,
}

const PROFILES: &[InteractionProfile] = &[
    // The baseline every runtime must support: one button, no axes. Binding
    // the float actions to a click is legal — OpenXR converts bool to 0.0/1.0.
    InteractionProfile {
        path: "/interaction_profiles/khr/simple_controller",
        pose: "/input/grip/pose",
        squeeze: "/input/select/click",
        trigger: "/input/select/click",
        stick: None,
        haptic: "/output/haptic",
    },
    InteractionProfile {
        path: "/interaction_profiles/oculus/touch_controller",
        pose: "/input/grip/pose",
        squeeze: "/input/squeeze/value",
        trigger: "/input/trigger/value",
        stick: Some("/input/thumbstick"),
        haptic: "/output/haptic",
    },
    InteractionProfile {
        path: "/interaction_profiles/valve/index_controller",
        pose: "/input/grip/pose",
        squeeze: "/input/squeeze/value",
        trigger: "/input/trigger/value",
        stick: Some("/input/thumbstick"),
        haptic: "/output/haptic",
    },
    InteractionProfile {
        path: "/interaction_profiles/htc/vive_controller",
        pose: "/input/grip/pose",
        squeeze: "/input/squeeze/click",
        trigger: "/input/trigger/value",
        stick: Some("/input/trackpad"),
        haptic: "/output/haptic",
    },
    InteractionProfile {
        path: "/interaction_profiles/microsoft/motion_controller",
        pose: "/input/grip/pose",
        squeeze: "/input/squeeze/click",
        trigger: "/input/trigger/value",
        stick: Some("/input/thumbstick"),
        haptic: "/output/haptic",
    },
];

fn suggest_bindings(actions: Res<XrActions>, mut suggestions: MessageWriter<OxrSuggestActionBinding>) {
    for profile in PROFILES {
        let mut suggest = |action: openxr::sys::Action, suffix: &str| {
            suggestions.write(OxrSuggestActionBinding {
                action,
                interaction_profile: Cow::Borrowed(profile.path),
                bindings: Hand::ALL
                    .iter()
                    .map(|hand| Cow::Owned(format!("{}{}", hand.subaction_path(), suffix)))
                    .collect(),
            });
        };

        suggest(actions.pose.as_raw(), profile.pose);
        suggest(actions.squeeze.as_raw(), profile.squeeze);
        suggest(actions.trigger.as_raw(), profile.trigger);
        suggest(actions.haptic.as_raw(), profile.haptic);
        if let Some(stick) = profile.stick {
            suggest(actions.stick.as_raw(), stick);
        }
    }
}

fn attach_actions(actions: Res<XrActions>, mut attach: MessageWriter<OxrAttachActionSet>) {
    attach.write(OxrAttachActionSet(actions.set.clone()));
}

fn request_action_sync(actions: Res<XrActions>, mut sync: MessageWriter<OxrSyncActionSet>) {
    sync.write(OxrSyncActionSet(actions.set.clone()));
}

/// Copies the synced action state onto each controller entity.
///
/// This is the only system that reads OpenXR action state, so it is the only
/// one that has to care about the sync ordering above.
fn read_controller_input(
    actions: Res<XrActions>,
    session: Res<OxrSession>,
    mut controllers: Query<(&Hand, &mut ControllerInput)>,
) {
    for (hand, mut input) in &mut controllers {
        let path = actions.path(*hand);

        let squeeze = actions
            .squeeze
            .state(&session, path)
            .map(|s| s.current_state)
            .unwrap_or_default();
        let trigger = actions
            .trigger
            .state(&session, path)
            .map(|s| s.current_state)
            .unwrap_or_default();
        let stick = actions
            .stick
            .state(&session, path)
            .map(|s| Vec2::new(s.current_state.x, s.current_state.y))
            .unwrap_or_default();
        let active = actions.pose.is_active(&session, path).unwrap_or(false);

        input.was_gripping = input.gripping;
        input.squeeze = squeeze;
        input.trigger = trigger;
        input.stick = stick;
        input.active = active;
        input.gripping = squeeze >= ControllerInput::GRAB_THRESHOLD;
    }
}
