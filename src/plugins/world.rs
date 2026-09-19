//! The static scene: floor, lights, a table and the props on it.
//!
//! Scale is in metres and matters more than usual — in VR the player has a real
//! body to compare against, and a table at the wrong height reads as wrong
//! immediately.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::components::interaction::{Grabbable, GrabbableMaterials};
use crate::plugins::quality::RenderQuality;

/// Height of the table surface, in metres.
const TABLE_HEIGHT: f32 = 0.75;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.7, 0.75, 0.9),
            brightness: 220.0,
            ..default()
        })
        .add_systems(Startup, (spawn_environment, spawn_props, spawn_overlay));
    }
}

fn spawn_environment(
    quality: Res<RenderQuality>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // A slab rather than a plane, so the mesh and the collider are the same
    // shape. Sunk by half its depth to put the walking surface at y = 0.
    commands.spawn((
        Name::new("Floor"),
        Mesh3d(meshes.add(Cuboid::new(30.0, 0.2, 30.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.34, 0.38),
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.1, 0.0),
        RigidBody::Static,
        Collider::cuboid(30.0, 0.2, 30.0),
    ));

    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        // Bevy's default cascade layout spans 150 m; this scene is 12 m
        // across. See `plugins::quality` for the measurement.
        quality.cascades(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Pillars. Without vertical edges nearby, smooth locomotion gives almost no
    // sense of motion — the floor alone is not enough parallax.
    let pillar = meshes.add(Cuboid::new(0.4, 3.0, 0.4));
    let pillar_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.42, 0.4),
        perceptual_roughness: 0.8,
        ..default()
    });
    for index in 0..8 {
        let angle = index as f32 / 8.0 * std::f32::consts::TAU;
        commands.spawn((
            Name::new(format!("Pillar {index}")),
            Mesh3d(pillar.clone()),
            MeshMaterial3d(pillar_material.clone()),
            Transform::from_xyz(angle.cos() * 6.0, 1.5, angle.sin() * 6.0),
            RigidBody::Static,
            Collider::cuboid(0.4, 3.0, 0.4),
        ));
    }
}

fn spawn_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Name::new("Table"),
        Mesh3d(meshes.add(Cuboid::new(1.2, 0.05, 0.7))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.38, 0.27, 0.19),
            perceptual_roughness: 0.7,
            ..default()
        })),
        Transform::from_xyz(0.0, TABLE_HEIGHT, -0.6),
        RigidBody::Static,
        Collider::cuboid(1.2, 0.05, 0.7),
    ));

    let cube = meshes.add(Cuboid::from_length(0.09));
    let sphere = meshes.add(Sphere::new(0.055).mesh().uv(24, 12));

    // One highlight material, shared by every prop: the highlight is a swap of
    // the handle on `MeshMaterial3d`, never an edit of the material itself.
    let highlight = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.3),
        emissive: LinearRgba::rgb(0.6, 0.45, 0.1),
        ..default()
    });

    let colors = [
        Color::srgb(0.85, 0.25, 0.3),
        Color::srgb(0.25, 0.6, 0.85),
        Color::srgb(0.3, 0.75, 0.4),
        Color::srgb(0.8, 0.55, 0.2),
    ];

    for (index, color) in colors.into_iter().enumerate() {
        let idle = materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.4,
            ..default()
        });
        let x = -0.45 + index as f32 * 0.3;
        let round = index % 2 == 1;

        commands.spawn((
            Name::new(format!("Prop {index}")),
            Mesh3d(if round { sphere.clone() } else { cube.clone() }),
            MeshMaterial3d(idle.clone()),
            GrabbableMaterials {
                idle,
                highlight: highlight.clone(),
            },
            Grabbable::default(),
            Transform::from_xyz(x, TABLE_HEIGHT + 0.08, -0.6),
            RigidBody::Dynamic,
            if round {
                Collider::sphere(0.055)
            } else {
                Collider::cuboid(0.09, 0.09, 0.09)
            },
        ));
    }
}

/// On-screen help. Bevy UI draws to the window camera, so this is visible on
/// the desktop mirror only — an in-headset version would have to be world-space
/// geometry parented to the tracking root.
fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Name::new("Help Overlay"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            // Bottom-left: the FPS overlay and its graph own the top-left.
            bottom: Val::Px(16.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        children![(
            Text::new(
                "VR: left stick moves - right stick snap-turns - squeeze to grab\n\
                 Desktop: WASD/QE moves - right mouse looks - left click grabs\n\
                 Esc for settings (VR/desktop, frame stats, v-sync, display)",
            ),
            TextFont {
                font_size: FontSize::Px(15.0),
                ..default()
            },
            TextColor(Color::WHITE),
        )],
    ));
}
