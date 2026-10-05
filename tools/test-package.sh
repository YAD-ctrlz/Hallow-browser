#!/usr/bin/env bash
# Install a Hallow .deb on a clean Ubuntu runner and check it end to end:
#
#   1. the package installs with its dependencies and runs,
#   2. package contents: update helper and policy, no MAR updater, VA-API
#      decoders, desktop file,
#   3. icons: every size resolves through the icon theme and fills its
#      canvas,
#   4. window identity (WM_CLASS) and window icons match the desktop file,
#   5. screenshots of the new tab page and a web page,
#   6. browser checks (tools/test_browser.py): startup, graphics and video
#      decoding status, codec preference, playback, YouTube (reported only),
#   7. the built-in update mechanism (tools/test_updates.py), last, because
#      it upgrades the installed package.
#
# Every stage runs; results/summary.md lists each stage and check, and the
# script fails if any did.
#
#   tools/test-package.sh <deb> <results-dir>
#
# Needs sudo. Used by the development and release workflows.
set -uo pipefail

deb=$(realpath "$1")
out=$(realpath -m "$2")
here=$(dirname "$(realpath "$0")")
work=$(mktemp -d)
mkdir -p "$out"
summary="$out/summary.md"
: > "$summary"
failed=0

# stage <name> <command...>: run a stage, log it, record its result.
stage() {
  local name=$1
  shift
  echo "::group::$name"
  if "$@" 2>&1 | tee "$out/$name.log"; then
    echo "- ✅ $name" >> "$summary"
  else
    echo "- ❌ **$name**" >> "$summary"
    echo "::error::$name failed"
    failed=1
  fi
  # Individual checks, from the Python tests.
  grep -E '^(PASS|FAIL|INFO) ' "$out/$name.log" | sed 's/^/  - /' >> "$summary" || true
  echo "::endgroup::"
}

install_package() {
  sudo apt-get update -q &&
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q "$deb" \
      xvfb imagemagick xdotool x11-utils desktop-file-utils binutils \
      python3-gi gir1.2-gtk-3.0 apt-utils gpg ffmpeg polkitd &&
    dpkg -s hallow | sed -n '1,/^Description/p' &&
    hallow --version
}

package_contents() {
  local ok=0 f codec
  for f in /usr/lib/hallow/hallow-update-helper \
    /usr/share/polkit-1/actions/io.github.yad_ctrlz.Hallow.update.policy \
    /usr/lib/hallow/is-packaged-app /usr/share/icons/hicolor/scalable/apps/hallow.svg; do
    if test -e "$f"; then echo "PASS present: $f"; else echo "FAIL missing: $f"; ok=1; fi
  done
  if test -e /usr/lib/hallow/updater; then
    echo "FAIL the package ships Gecko's MAR updater"; ok=1
  else
    echo "PASS no MAR updater (Linux packages update through APT)"
  fi
  if test -e /etc/apt/sources.list.d/hallow.sources; then
    cat /etc/apt/sources.list.d/hallow.sources
    if test -s /usr/share/keyrings/hallow-archive-keyring.asc; then
      echo "PASS update channel and archive key installed"
    else
      echo "FAIL update channel without archive key"; ok=1
    fi
  else
    echo "::warning::package has no update channel (no archive key committed)"
  fi
  # Hardware video decoding: Gecko's GPU probe and the VA-API decoders of
  # its bundled FFmpeg (H.264 goes through the system's libavcodec).
  if test -x /usr/lib/hallow/gfxtest; then echo "PASS gfxtest (GPU probe) present"; else echo "FAIL no gfxtest"; ok=1; fi
  for codec in av1 vp9 vp8; do
    if strings /usr/lib/hallow/libmozavcodec.so | grep -qx "${codec}_vaapi"; then
      echo "PASS bundled FFmpeg has the ${codec} VA-API decoder"
    else
      echo "FAIL bundled FFmpeg lacks the ${codec} VA-API decoder"; ok=1
    fi
  done
  if desktop-file-validate /usr/share/applications/hallow.desktop &&
    grep -q '^Icon=hallow$' /usr/share/applications/hallow.desktop &&
    grep -q '^StartupWMClass=hallow$' /usr/share/applications/hallow.desktop; then
    echo "PASS desktop file: valid, Icon=hallow, StartupWMClass=hallow"
  else
    echo "FAIL desktop file"; ok=1
  fi
  return $ok
}

icons() {
  # Each size must resolve to its own PNG (not a scaled neighbour) and the
  # artwork must fill it like other apps' icons do.
  /usr/bin/python3 - <<'EOF'
import gi, subprocess, sys
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk
theme = Gtk.IconTheme.get_default()
bad = 0
for size in (16, 22, 24, 32, 48, 64, 96, 128, 192, 256, 512):
    info = theme.lookup_icon("hallow", size, 0)
    path = info.get_filename() if info else None
    expected = f"/usr/share/icons/hicolor/{size}x{size}/apps/hallow.png"
    trim = subprocess.run(
        ["convert", expected, "-alpha", "extract", "-threshold", "6%",
         "-format", "%@", "info:"], capture_output=True, text=True).stdout
    w, h = (int(v) for v in trim.split("+")[0].split("x"))
    fill = max(w, h) / size
    ok = path == expected and fill >= 0.9
    bad += not ok
    print(f"{'PASS' if ok else 'FAIL'} icon {size}px -> {path}, artwork {w}x{h} ({fill:.0%})")
for size in (40, 56, 80):
    info = theme.lookup_icon("hallow", size, 0)
    print(f"     icon {size}px (between sizes) -> {info.get_filename() if info else None}")
sys.exit(1 if bad else 0)
EOF
}

window_identity() {
  local profile pid win icons ok=0 s
  profile=$(mktemp -d)
  hallow --new-instance --profile "$profile" about:blank >/dev/null 2>&1 &
  pid=$!
  for _ in $(seq 60); do
    win=$(xdotool search --classname Navigator 2>/dev/null | tail -1 || true)
    [ -n "$win" ] && break
    sleep 1
  done
  if xprop -id "$win" WM_CLASS | tee /dev/stderr | grep -q '"Navigator", "hallow"'; then
    echo "PASS WM_CLASS is Navigator/hallow, matching StartupWMClass=hallow"
  else
    echo "FAIL WM_CLASS"; ok=1
  fi
  icons=$(xprop -id "$win" -notype -len 99999999 _NET_WM_ICON | grep -o 'Icon ([0-9]* x [0-9]*)' | sort -u | tr '\n' ' ')
  for s in 16 32 48 64 128; do
    echo "$icons" | grep -q "($s x $s)" || { echo "FAIL no ${s}px window icon ($icons)"; ok=1; }
  done
  [ $ok = 0 ] && echo "PASS window icons: $icons"
  kill "$pid"; wait "$pid" 2>/dev/null
  rm -rf "$profile"
  return $ok
}

allow_updates_in_ci() {
  # CI stands in for the administrator who would type their password in
  # the polkit prompt: allow this user the Hallow update action.
  sudo tee /etc/polkit-1/rules.d/49-hallow-ci.rules >/dev/null <<EOF
polkit.addRule(function (action, subject) {
  if (action.id == "io.github.yad_ctrlz.Hallow.update" &&
      subject.user == "$(id -un)") {
    return polkit.Result.YES;
  }
});
EOF
  sudo systemctl restart polkit 2>/dev/null || true
}

stage install install_package
if ! dpkg -s hallow >/dev/null 2>&1; then
  cat "$summary"
  exit 1
fi
stage package-contents package_contents

export DISPLAY=:99
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null || true; rm -rf "$work"' EXIT
sleep 2

stage icons icons
stage window-identity window_identity
stage screenshots bash "$here/screenshot.sh" /usr/bin/hallow "$out/screenshots"
stage browser python3 "$here/test_browser.py" --out "$out/browser" \
  --media "$work/media" --youtube
allow_updates_in_ci
stage updates python3 "$here/test_updates.py" --deb "$deb" \
  --out "$out/updates" --work "$work/updates"

echo
cat "$summary"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  { echo "### Package tests: $(basename "$deb")"; cat "$summary"; } >> "$GITHUB_STEP_SUMMARY"
fi
exit $failed
