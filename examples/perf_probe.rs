//! Frame-time probe for the starter's scene, without XR.
//!
//! Renders the same geometry the app does, at a configurable resolution and
//! quality, with vsync off, and reports frame-time percentiles. Used to compare
//! render settings on one machine; the absolute numbers mean nothing across
//! machines.
//!
//! PROBE_RES=3000x3000 PROBE_MSAA=off PROBE_SHADOWS=0 cargo run --example perf_probe

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::render::view::Msaa;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{PresentMode, WindowResolution};
use bevy::winit::{UpdateMode, WinitSettings};

const WARMUP: usize = 120;

fn env(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn main() -> AppExit {
    let res = env("PROBE_RES", "2048x2048");
    let (w, h) = res.split_once('x').expect("PROBE_RES must look like 2048x2048");
    let (w, h) = (w.parse::<f32>().unwrap(), h.parse::<f32>().unwrap());

    let msaa = match env("PROBE_MSAA", "4").as_str() {
        "off" | "1" => Msaa::Off,
        "2" => Msaa::Sample2,
        "8" => Msaa::Sample8,
        _ => Msaa::Sample4,
    };
    let shadows = env("PROBE_SHADOWS", "1") != "0";
    let cascades: usize = env("PROBE_CASCADES", "4").parse().unwrap();
    let shadow_map: usize = env("PROBE_SHADOWMAP", "2048").parse().unwrap();
    let max_dist: f32 = env("PROBE_MAXDIST", "150").parse().unwrap();
    let frames: usize = env("PROBE_FRAMES", "400").parse().unwrap();

    App::new()
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "perf probe".into(),
                    // NOTE: a window is clamped to the display size, so
                    // PROBE_RES above the monitor resolution is silently capped.
                    resolution: WindowResolution::new(w as u32, h as u32),
                    present_mode: PresentMode::AutoNoVsync,
                    ..default()
                }),
                ..default()
            }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .insert_resource(TargetSize(UVec2::new(w as u32, h as u32)))
        .insert_resource(DirectionalLightShadowMap { size: shadow_map })
        .insert_resource(Probe {
            msaa,
            shadows,
            cascades,
            shadow_map,
            max_dist,
            frames,
            samples: Vec::with_capacity(frames),
            seen: 0,
        })
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.7, 0.75, 0.9),
            brightness: 220.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, record)
        .run()
}

#[derive(Resource)]
struct TargetSize(UVec2);

#[derive(Resource)]
struct Probe {
    msaa: Msaa,
    shadows: bool,
    cascades: usize,
    shadow_map: usize,
    max_dist: f32,
    frames: usize,
    samples: Vec<f32>,
    seen: usize,
}

fn setup(
    probe: Res<Probe>,
    target: Res<TargetSize>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(30.0, 30.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.34, 0.38),
            perceptual_roughness: 0.9,
            ..default()
        })),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: probe.shadows,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: probe.cascades,
            maximum_distance: probe.max_dist,
            first_cascade_far_bound: (probe.max_dist / probe.cascades as f32).min(10.0),
            ..default()
        }
        .build(),
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let pillar = meshes.add(Cuboid::new(0.4, 3.0, 0.4));
    let pillar_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.42, 0.4),
        perceptual_roughness: 0.8,
        ..default()
    });
    for index in 0..8 {
        let angle = index as f32 / 8.0 * std::f32::consts::TAU;
        commands.spawn((
            Mesh3d(pillar.clone()),
            MeshMaterial3d(pillar_material.clone()),
            Transform::from_xyz(angle.cos() * 6.0, 1.5, angle.sin() * 6.0),
        ));
    }

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.2, 0.05, 0.7))),
        MeshMaterial3d(materials.add(Color::srgb(0.38, 0.27, 0.19))),
        Transform::from_xyz(0.0, 0.75, -0.6),
    ));
    let cube = meshes.add(Cuboid::from_length(0.09));
    let sphere = meshes.add(Sphere::new(0.055).mesh().uv(24, 12));
    for index in 0..4 {
        commands.spawn((
            Mesh3d(if index % 2 == 1 { sphere.clone() } else { cube.clone() }),
            MeshMaterial3d(materials.add(Color::srgb(0.85, 0.25, 0.3))),
            Transform::from_xyz(-0.45 + index as f32 * 0.3, 0.83, -0.6),
        ));
    }

    // Off-screen rendering removes present backpressure: the CPU runs ahead,
    // the GPU queue saturates, and frame deltas become bimodal and useless.
    // Default to a window target, which throttles honestly. Off-screen stays
    // available for exceeding the display size, with that caveat.
    if std::env::var("PROBE_OFFSCREEN").as_deref() != Ok("1") {
        commands.spawn((
            Camera3d::default(),
            probe.msaa,
            Transform::from_xyz(0.0, 1.6, 1.2).looking_at(Vec3::new(0.0, 0.9, -0.6), Vec3::Y),
        ));
        return;
    }

    let size = Extent3d {
        width: target.0.x,
        height: target.0.y,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);

    commands.spawn((
        Camera3d::default(),
        // In 0.19 the target is its own component, not a `Camera` field.
        RenderTarget::Image(ImageRenderTarget {
            handle,
            scale_factor: 1.0,
        }),
        probe.msaa,
        Transform::from_xyz(0.0, 1.6, 1.2).looking_at(Vec3::new(0.0, 0.9, -0.6), Vec3::Y),
    ));
}

fn record(
    time: Res<Time>,
    target: Res<TargetSize>,
    mut probe: ResMut<Probe>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    probe.seen += 1;
    if probe.seen <= WARMUP {
        return;
    }
    let ms = time.delta_secs() * 1000.0;
    probe.samples.push(ms);
    if probe.samples.len() < probe.frames {
        return;
    }

    let mut s = probe.samples.clone();
    s.sort_by(f32::total_cmp);
    let pick = |q: f32| s[((s.len() - 1) as f32 * q) as usize];
    let mean = s.iter().sum::<f32>() / s.len() as f32;
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.average())
        .unwrap_or(0.0);

    println!(
        "RESULT res={}x{} msaa={:?} shadows={} cascades={} smap={} maxdist={} n={} mean={:.2}ms p50={:.2}ms p95={:.2}ms p99={:.2}ms fps_avg={:.1}",
        target.0.x,
        target.0.y,
        probe.msaa,
        probe.shadows,
        probe.cascades,
        probe.shadow_map,
        probe.max_dist,
        s.len(),
        mean,
        pick(0.50),
        pick(0.95),
        pick(0.99),
        fps,
    );
    exit.write(AppExit::Success);
}
