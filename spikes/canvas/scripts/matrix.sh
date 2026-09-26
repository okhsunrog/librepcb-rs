#!/usr/bin/env bash
# Runs every (scene, Slint renderer, canvas mode) combination: on-screen animation
# (frame period via rendering notifier) + snapshot bench. Logs go to out/logs/.
set -u
cd "$(dirname "$0")/.."
BIN=./target/release/canvas-spike
mkdir -p out/logs out/win
CONFIGS=(
  "femtovg transform" "femtovg viewbox" "femtovg tiny-skia" "femtovg vello-cpu"
  "femtovg-wgpu transform" "femtovg-wgpu viewbox" "femtovg-wgpu gpu30"
  "skia transform" "skia viewbox" "skia gpu29" "skia gpu30"
  "software viewbox" "software tiny-skia" "software vello-cpu"
  "vello viewbox"
)
for scene in ${SCENES:-gerber stress}; do
  for cfg in "${CONFIGS[@]}"; do
    set -- $cfg
    name="$scene-$1-$2"
    echo "== $name"
    SLINT_BACKEND=winit-$1 timeout 120 $BIN --scene $scene --mode $2 --animate ${ANIM:-2} \
      > out/logs/$name-anim.log 2>&1
    grep "\[" out/logs/$name-anim.log | sed 's/^/   /'
    SLINT_BACKEND=winit-$1 timeout 300 $BIN --scene $scene --mode $2 --bench --iters ${ITERS:-8} --out-dir out/win \
      > out/logs/$name-bench.log 2>&1
    grep -E "\[|slint model|gpu:|rror|panic" out/logs/$name-bench.log | sed 's/^/   /'
  done
done
