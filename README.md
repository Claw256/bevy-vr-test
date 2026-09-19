# bevy_vr_starter

A starter 3D VR project: **Bevy 0.19** rendering through **OpenXR** via
[`bevy_mod_openxr`](https://github.com/awtterpip/bevy_oxr).

It runs with a headset if an OpenXR runtime is available, and falls back to an
ordinary desktop window if not — so you can work on it either way.

## What is in it

| Feature | Where |
| --- | --- |
| OpenXR action set, bound across five controller profiles | `src/plugins/xr_input.rs` |
| Tracked controllers (grip pose → entity transform) | `src/plugins/controllers.rs` |
| Smooth locomotion + snap turning on the tracking root | `src/plugins/locomotion.rs` |
| Grab, carry and throw props, with haptic feedback | `src/plugins/interaction.rs` |
| Physics: Avian bodies and colliders | `src/plugins/physics.rs`, `world.rs` |
| Settings menu: VR/desktop, frame stats, v-sync, display mode | `src/plugins/settings.rs` |
| Scene, props and the on-screen help overlay | `src/plugins/world.rs` |
| Desktop mirror camera and fly controls | `src/plugins/desktop.rs` |
| Hand-tracking skeleton gizmos | `HandGizmosPlugin`, registered in `main.rs` |

## Controls

**In the headset**

- Left thumbstick — move, relative to where you are looking
- Right thumbstick — snap turn 45°
- Squeeze either grip — pick up a highlighted prop; let go to drop, or throw it
  by releasing while moving your hand

**On the desktop** (whenever a VR session is not running)

- `W` `A` `S` `D`, `Q` `E` — move; hold `Shift` to go faster
- Hold right mouse button — look around
- **Left click** — pick up the prop under the cursor; click again to drop it
- `Esc` — open the settings menu

## Running it

```bash
cargo run            # dev build
cargo run --release  # do this before judging performance or comfort
```

On Linux you need an OpenXR runtime installed and active — [Monado](https://monado.dev/)
or SteamVR. Without one you will see this in the log, which is expected and not
an error in this project:

```
ERROR Failed to initialize openxr: ...
```

The app then renders to the window instead. To confirm which runtime is active,
check `/usr/share/openxr/1/active_runtime.json` (or the `XR_RUNTIME_JSON`
environment variable).

`cargo run --release` is worth repeating: a debug build will not hold 72–90 fps,
and a headset that misses frames is unpleasant rather than just slow.

## Architecture

The layout follows one plugin per domain, with components as pure data:

```
src/
├── main.rs                   app + plugin registration, nothing else
├── sets.rs                   the frame pipeline (system sets and their order)
├── messages.rs               buffered messages (HapticPulse)
├── components/
│   ├── controller.rs         Hand, Controller, ControllerInput
│   └── interaction.rs        Grabbable, Grabbed, InReach, GrabbableMaterials
└── plugins/
    ├── world.rs  desktop.rs  xr_input.rs
    ├── controllers.rs        locomotion.rs        interaction.rs
```

Two ordering constraints are load-bearing and enforced in `src/sets.rs`:

1. **`PreUpdate`** — `VrInputSet::RequestSync` runs *before* the backend's
   `OxrActionSetSyncSet`, and `VrInputSet::Read` *after* it. Reading an action
   before the runtime has synced it returns last frame's value.
2. **`Update`** — `Locomotion → Interaction → Feedback`. The player moves, then
   gameplay reacts to where they ended up, then output goes back to the hardware.

`XrInputPlugin` is the only place that talks to `openxr::Action`; everything
downstream reads the `ControllerInput` component. Adding a button means adding
one action and one field, not touching five systems.

### Tracking root

Tracked entities (both eyes, both controllers, hand joints) are parented to the
`XrTrackingRoot` that `bevy_mod_xr` spawns. The headset reports poses relative
to the player's real room; the root is what places that room in the world. So
**locomotion moves the root**, never the camera — moving a camera that the
runtime overwrites every frame does nothing.

### Adding an input

1. Add the action to `XrActions` in `src/plugins/xr_input.rs`.
2. Add its binding suffix to each entry in `PROFILES`.
3. Add a field to `ControllerInput` and fill it in `read_controller_input`.

Note that suggesting bindings only works *before* the action set is attached to
the session, which is why all of this happens in `Startup` and
`OxrSendActionBindings` rather than on demand.

## Performance

The two Bevy defaults that cost the most in this app, and what replaced them,
live in `src/plugins/quality.rs`.

**MSAA.** `bevy_render` calls `register_required_components::<Camera, Msaa>()`,
and the OpenXR backend spawns its eye cameras without an `Msaa` of their own
(`bevy_mod_openxr/src/openxr/render.rs`), so both eyes silently get
`Msaa::Sample4`. The XR swapchain is created with `sample_count: 1`, so that 4x
buffer is resolved straight back down — paid at full headset resolution, twice,
for nothing.

**Shadow cascades.** Bevy defaults to 4 cascades over 150 m at 2048px each.
Everything in this scene sits inside a 12 m circle, so three of those cascades
cover empty ground and the one that matters is stretched over 150 m of it. One
cascade over 30 m is both cheaper *and* sharper, since the same 2048px covers
far less ground.

Measured with `examples/perf_probe.rs`, release build, 1920x1080, one view,
500 frames after a 120-frame warmup:

| Configuration | p50 frame time |
| --- | --- |
| Bevy defaults (MSAA 4, 4 cascades / 150 m) | 22.4 ms |
| MSAA off only | 21.1 ms |
| 1 cascade / 30 m only | 19.5 ms |
| **Both — the current defaults** | **18.1 ms** (-19%) |
| Shadows off entirely | 11.1 ms (-50%) |

Shadows are the single largest cost: ~9.8 ms of the 22.4 ms baseline, and most
of that is per-fragment shadow sampling in the main pass, not the shadow pass
itself — dropping the shadow map from 2048px to 1024px changed nothing
measurable. If you need more headroom than the settings above give you, turning
shadows off is the next big lever, at a real cost to depth perception in VR.

Tune it through the `RenderQuality` resource, inserted before the plugins:

```rust
app.insert_resource(RenderQuality {
    msaa: Msaa::Off,
    shadow_cascades: 1,
    shadow_distance: 30.0,
    shadow_map_size: 2048,
});
```

### The settings menu

`Esc` opens a settings menu (`src/plugins/settings.rs`). Click a row to cycle it:

| Row | Options |
| --- | --- |
| Mode | Desktop / VR — starts or ends the OpenXR session at runtime |
| Frame stats | Hidden / Shown — FPS readout plus a rolling frame-time graph |
| V-Sync | On (auto), Off (auto), On – Fifo, Adaptive – Fifo relaxed, Off – Mailbox, Off – Immediate |
| Display | Windowed, Borderless fullscreen, Exclusive fullscreen |

The menu stays available while a headset is running, because switching *out*
of VR has to be reachable from somewhere — you drive it from the mirror window.
It is not visible inside the headset: Bevy's `Node` UI draws to the window
camera and never reaches the XR eye cameras. The fly controls yield while the
menu is open, so clicking a row does not also fly the camera.

Switching mode sends `XrCreateSessionMessage` to enter VR and
`XrRequestExitMessage` to leave. Leaving is safe from an auto-restart loop:
the backend reports `Exiting { should_restart: false }` and then re-inserts
`XrState::Available` *without* an `XrStateChanged`, so `auto_handle_session`
does not immediately recreate the session.

Every present mode is safe to pick. `bevy_render`'s `present_mode` chooses the
closest supported option and always ends at `Fifo`, logging when it substitutes
— so on a driver without Mailbox, that row still works, it just quietly gets
something else.

The frame-time graph is coloured against **headset** cadence rather than monitor
cadence — red below 72 fps, green above 90 — because the useful question while
working flat is whether the frame would survive in a headset. Change it via
`FpsOverlayConfig` if you want monitor thresholds.

The overlay needs Bevy's `bevy_dev_tools` feature, which `Cargo.toml` enables.
It is not a default feature; drop it for a shipping build if you would rather
not compile the dev tooling in. The overlay starts hidden, so it costs nothing
until you switch it on.

### Read these numbers carefully

They were measured on an Intel HD Graphics 530 integrated GPU, rendering **one**
1920x1080 view, with **no headset attached**. A VR frame is two views at roughly
twice that resolution each, on a very different GPU. The *ranking* of the levers
should carry over, because both MSAA and shadow sampling scale with pixels times
views. The *magnitudes* will not. Re-measure on your target hardware.

The probe renders to a window by default, which is clamped to your display size
— `PROBE_RES` above the monitor resolution is silently capped. `PROBE_OFFSCREEN=1`
renders to an image instead and lifts that cap, but off-screen removes present
backpressure, so the CPU runs ahead of the GPU and the frame deltas stop meaning
anything. Prefer the window path.

```bash
PROBE_RES=1920x1080 PROBE_MSAA=off PROBE_CASCADES=1 PROBE_MAXDIST=30 \
  cargo run --release --example perf_probe
```

### The other VR lever: resolution scale

Not measurable here, but on real hardware it is usually the biggest dial of all,
because it multiplies every per-fragment cost above. `bevy_mod_openxr` picks the
runtime's recommended per-eye resolution unless you name one, and it accepts any
size up to the runtime's maximum. Set it *before* `add_xr_plugins`, since the
swapchain is built during plugin construction:

```rust
app.insert_resource(OxrSessionConfig {
    resolutions: Some(vec![UVec2::new(1600, 1600)]),
    ..default()
});
```

The backend logs `XrCamera resolution: ...` at startup — start from that number
and scale it down.

### Two more knobs

- **Hand gizmos** redraw 26 joints per hand every frame. `main.rs` now registers
  `HandGizmosPlugin` only under `cfg!(debug_assertions)`.
- **Pipelined rendering** is disabled in `main.rs` on purpose: it buys throughput
  by adding a frame of latency, which you feel in a headset. If you end up
  CPU-bound rather than fill-bound, re-enabling it is the trade to reconsider.

## Physics

[Avian](https://github.com/avianphysics/avian) (`avian3d 0.7`, the release that
targets Bevy 0.19). Floor, pillars and table are `RigidBody::Static`; the props
are `RigidBody::Dynamic`.

A held prop switches to `RigidBody::Kinematic` and is driven from the holder's
pose each frame, rather than being parented to it. Parenting is what the
pre-physics version did, and it stops working once a solver is in the world:
the solver and the transform hierarchy would both be writing the same body.

Carrying kinematically is also what makes throwing work. The carry system
already knows how far the prop moved last frame, so releasing hands that
velocity to the solver — clamped by `GrabTuning`, because tracking jitter can
produce a single huge frame delta and fling a prop out of the room.

One scheduling detail matters. Avian runs in `FixedPostUpdate`, which is
*earlier* in the frame than `Update`. A `Position` written from `Update` would
not reach `Transform` until the next fixed step, and fixed steps do not happen
every frame — the prop would visibly stutter in your hand. So the carry system
writes `Transform`, which renders correctly this frame; Avian's
`transform_to_position` picks it up before the next step.

## Known limits

- **UI is flat only.** Bevy's `Node` UI draws to the window camera, so the help
  overlay is invisible in the headset. In-headset UI has to be world-space
  geometry parented to the tracking root.
- **No teleport locomotion.** Smooth movement makes some people motion-sick;
  a teleport option is the usual accompaniment.

## Version pinning

Bevy and the XR crates move together — `bevy 0.19` ↔ `bevy_mod_openxr 0.6` ↔
`bevy_mod_xr 0.6`. Bump them in one commit or the graph will not resolve.

`Cargo.lock` pins `encase` to 0.12.1 on purpose. `encase_derive_impl 0.12.2`
moved to `syn 3`, which does not compile against `bevy_encase_derive 0.19.1`
(`expected Punctuated<PathSegment, PathSep>, found a different ...`). If you
regenerate the lockfile and hit that error:

```bash
cargo update -p encase --precise 0.12.1
```

## Build settings

`.cargo/config.toml` links with `clang` + `lld` for faster incremental builds.
Delete it if you do not have them installed.

`[profile.dev.package."*"]` builds dependencies at `opt-level = 3` with
`debug = false`. The optimization is what makes a debug build playable at all;
dropping dependency debug info is what keeps LLVM's memory use in range — with
it on, rustc was killed by SIGSEGV part-way through the Bevy tree on a 16 GB
machine.

For faster iteration you can also add `--features bevy/dynamic_linking`, which
resolves against `bevy_dylib 0.19.1`. Check that a matching `bevy_dylib` exists
for your exact Bevy patch version before relying on it — 0.19.0 shipped without
one.
