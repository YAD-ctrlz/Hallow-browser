#!/usr/bin/env bash
# Launch a Gecko browser under Xvfb with a fresh profile and capture the whole
# screen: the new tab page in light and dark mode, and a web page.
#
#   tools/screenshot.sh <browser-binary> <output-dir>
#
# Needs Xvfb and ImageMagick (`import`). Used by the UI preview and release
# workflows.
set -euo pipefail

browser=$1
out=$2
mkdir -p "$out"

# Use the caller's display if there is one, else start Xvfb.
if [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:99
  Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
  xvfb=$!
  trap 'kill "$xvfb" 2>/dev/null || true' EXIT
  sleep 2
fi

shoot() {
  local name=$1 theme=$2 url=$3 profile pid
  profile=$(mktemp -d)
  GTK_THEME=$theme "$browser" --new-instance --profile "$profile" \
    "$url" >"$out/$name.log" 2>&1 &
  pid=$!
  sleep 30
  import -window root "$out/$name.png"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -rf "$profile"
  echo "captured $out/$name.png"
}

shoot newtab-light Adwaita about:newtab
shoot newtab-dark Adwaita:dark about:newtab
shoot page-light Adwaita https://example.com
