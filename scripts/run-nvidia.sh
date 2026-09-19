#!/usr/bin/env bash
# Run the app on the discrete NVIDIA GPU on a hybrid (Optimus) laptop.
#
# Bevy already asks wgpu for PowerPreference::HighPerformance, so when the
# NVIDIA GPU is visible to Vulkan it is picked without any help. This script is
# for when it is not, or when you want the choice to be explicit rather than
# dependent on what the driver happens to be exposing today.
#
#   scripts/run-nvidia.sh                 # debug
#   scripts/run-nvidia.sh --release       # anything here is passed to cargo run
set -euo pipefail

cd "$(dirname "$0")/.."

# PRIME render offload: route rendering to the NVIDIA GPU and hide the Intel one
# from the Vulkan loader. Same variables `prime-run` sets; inlined so this works
# without nvidia-prime installed.
export __NV_PRIME_RENDER_OFFLOAD=1
export __VK_LAYER_NV_optimus=NVIDIA_only
export __GLX_VENDOR_LIBRARY_NAME=nvidia

# Belt and braces: Bevy matches this against the adapter name, case-insensitively.
# Note this only works as an environment variable — `add_xr_plugins` disables
# Bevy's RenderPlugin and the OpenXR backend re-adds it with
# RenderPlugin::default(), so WgpuSettings set in code is discarded.
export WGPU_ADAPTER_NAME="${WGPU_ADAPTER_NAME:-NVIDIA}"

echo "Requesting adapter matching '${WGPU_ADAPTER_NAME}'..."
exec cargo run "$@"
