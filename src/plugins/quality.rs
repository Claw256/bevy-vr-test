//! Render-quality settings, chosen for VR's cost model rather than Bevy's
//! flat-screen defaults.
//!
//! Two of those defaults are actively wrong for this app:
//!
//! * **MSAA.** `bevy_render` does `register_required_components::<Camera, Msaa>()`,
//!   and the OpenXR backend spawns its eye cameras without an `Msaa` of their
//!   own, so both eyes silently get `Msaa::Sample4`. The XR swapchain is created
//!   with `sample_count: 1`, so that 4x buffer is resolved straight back down —
//!   paid for at full headset resolution, twice, for nothing.
//! * **Shadow cascades.** Bevy defaults to 4 cascades over 150 m at 2048px each,
//!   sized for an open world. Everything here is inside a 12 m circle, so three
//!   of those cascades cover empty ground, and the one that matters is stretched
//!   across 150 m of it.
//!
//! Measured on the probe (`examples/perf_probe.rs`) at 1920x1080, one view:
//! Bevy defaults 22.4 ms/frame; these settings 18.1 ms, a 19% cut with no
//! visual loss — the single cascade over 30 m is *sharper* than the default
//! near cascade, since the same 2048px covers far less ground.

use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::Msaa;
use bevy_mod_xr::camera::XrViewInit;
use bevy_mod_xr::session::XrSessionCreated;

pub struct RenderQualityPlugin;

#[derive(Resource, Clone, Copy, Debug)]
pub struct RenderQuality {
    /// Applied to every camera, including the ones the XR backend spawns.
    pub msaa: Msaa,
    /// More cascades sharpen distant shadows and cost a view each.
    pub shadow_cascades: usize,
    /// Beyond this many metres, nothing casts a shadow.
    pub shadow_distance: f32,
    /// Per-cascade shadow map resolution. Must be a power of two.
    pub shadow_map_size: usize,
}

impl Default for RenderQuality {
    fn default() -> Self {
        Self {
            msaa: Msaa::Off,
            shadow_cascades: 1,
            shadow_distance: 30.0,
            shadow_map_size: 2048,
        }
    }
}

impl RenderQuality {
    /// The cascade layout to put on a directional light.
    pub fn cascades(&self) -> CascadeShadowConfig {
        let cascades = self.shadow_cascades.max(1);
        CascadeShadowConfigBuilder {
            num_cascades: cascades,
            maximum_distance: self.shadow_distance,
            first_cascade_far_bound: self.shadow_distance / cascades as f32,
            ..default()
        }
        .build()
    }
}

impl Plugin for RenderQualityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RenderQuality>();

        let size = app.world().resource::<RenderQuality>().shadow_map_size;
        app.insert_resource(DirectionalLightShadowMap { size })
            // The eye cameras are spawned in the main world by the backend's
            // `init_views`, in this schedule — so override them here, before
            // they render a frame, as well as in `Update` for the flat camera.
            .add_systems(XrSessionCreated, apply_msaa.after(XrViewInit))
            .add_systems(Update, apply_msaa)
            .add_systems(Startup, report_adapter);
    }
}

/// Overrides the `Msaa` that `Camera` pulls in as a required component.
fn apply_msaa(
    quality: Res<RenderQuality>,
    mut cameras: Query<(Entity, &mut Msaa), Added<Msaa>>,
) {
    for (entity, mut msaa) in &mut cameras {
        if *msaa != quality.msaa {
            debug!("{entity}: MSAA {:?} -> {:?}", *msaa, quality.msaa);
            *msaa = quality.msaa;
        }
    }
}

/// Says which GPU the renderer actually picked, and complains if it is not a
/// discrete one.
///
/// On a hybrid laptop this is easy to get wrong and hard to notice: the app
/// runs, just on the integrated GPU. An iGPU will not hold headset cadence, so
/// it is worth a warning rather than a line buried in the startup log.
///
/// Note that `WgpuSettings` set in code will not change this. `add_xr_plugins`
/// disables Bevy's `RenderPlugin` and the OpenXR backend re-adds it with
/// `RenderPlugin::default()`, discarding anything configured here. The
/// environment variables `WGPU_ADAPTER_NAME`, `WGPU_BACKEND` and
/// `WGPU_POWER_PREF` still apply, because `WgpuSettings::default()` reads them.
fn report_adapter(adapter: Option<Res<RenderAdapterInfo>>) {
    let Some(adapter) = adapter else {
        return;
    };

    let name = &adapter.name;
    let backend = adapter.backend;

    match adapter.device_type {
        wgpu_types::DeviceType::DiscreteGpu => {
            info!("rendering on {name} ({backend:?})");
        }
        other => {
            warn!(
                "rendering on {name} ({backend:?}, {other:?}) - not a discrete GPU. \
                 On a hybrid laptop, set WGPU_ADAPTER_NAME to part of the name of \
                 the GPU you want, or launch through scripts/run-nvidia.sh.",
            );
        }
    }
}
