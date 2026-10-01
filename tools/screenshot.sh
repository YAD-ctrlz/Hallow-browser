#!/usr/bin/env bash
# Launch a Gecko browser under Xvfb with a fresh profile and capture the whole
# screen in light and dark mode.
#
#   tools/screenshot.sh <browser-binary> <output-dir>
#
# Needs Xvfb and ImageMagick (`import`). Used by the UI preview and release
# workflows.
set -euo pipefail

browser=$1
out=$2
mkdir -p "$out"

export DISPLAY=:99
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null || true' EXIT
sleep 2

shoot() {
  local name=$1 theme=$2 profile pid
  profile=$(mktemp -d)
  GTK_THEME=$theme "$browser" --new-instance --profile "$profile" \
    https://example.com about:newtab >"$out/$name.log" 2>&1 &
  pid=$!
  sleep 30
  import -window root "$out/$name.png"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -rf "$profile"
  echo "captured $out/$name.png"
}

shoot light Adwaita
shoot dark Adwaita:dark
