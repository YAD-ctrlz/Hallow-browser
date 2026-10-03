#!/usr/bin/env bash
# Pack what Firefox's PGO training run needs from the source tree, so it can
# run on another machine (the Windows training run, tools/pgo_windows.py):
# the profile server and its corpus (Mozilla's PGO pages, Speedometer 2 and
# 3, the web audio benchmark, layout and style singletons), the training
# profile's prefs, the add-on that quits the browser at the end, and the
# Python packages the server uses.
#
#   tools/pgo-kit.sh <firefox source tree> <kit.tar.gz>
set -euo pipefail
src=$1 out=$2
paths=(
  build/pgo
  testing/profiles
  tools/quitter/quitter@mozilla.org.xpi
  testing/talos/talos/tests/perf-reftest-singletons
  third_party/webkit/PerformanceTests/Speedometer
  third_party/webkit/PerformanceTests/Speedometer3
  third_party/webkit/PerformanceTests/webaudio
  testing/mozbase
  python/mozterm
  third_party/python/redo
  third_party/python/distro
)
for p in "${paths[@]}"; do
  test -e "$src/$p" || { echo "$src/$p is missing" >&2; exit 1; }
done
tar -C "$src" -czf "$out" "${paths[@]}"
ls -l "$out"
