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
| Grab and release props, with haptic feedback | `src/plugins/interaction.rs` |
| Scene, props and the on-screen help overlay | `src/plugins/world.rs` |
| Desktop mirror camera and fly controls | `src/plugins/desktop.rs` |
| Hand-tracking skeleton gizmos | `HandGizmosPlugin`, registered in `main.rs` |

## Controls

**In the headset**

- Left thumbstick — move, relative to where you are looking
- Right thumbstick — snap turn 45°
- Squeeze either grip — pick up a highlighted prop; let go to drop it

**On the desktop** (only when no OpenXR runtime is found)

- `W` `A` `S` `D`, `Q` `E` — move; hold `Shift` to go faster
- Hold right mouse button — look around

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

## Known limits

- **No physics.** A released prop stays where it was let go. Add
  [`avian3d`](https://crates.io/crates/avian3d) or `bevy_rapier3d` for throwing
  and collisions — and match the version to Bevy 0.19.
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
