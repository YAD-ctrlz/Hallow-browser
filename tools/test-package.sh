#!/usr/bin/env bash
# Install a Hallow .deb on a clean Ubuntu runner and check it end to end:
#
#   1. the package installs with its dependencies and runs,
#   2. desktop integration: desktop file, every icon size resolves through
#      the icon theme and fills its canvas, window identity (WM_CLASS) and
#      window icons match the desktop file,
#   3. screenshots of the new tab page and a web page,
#   4. browser checks (tools/test_browser.py): startup, graphics and video
#      decoding status, codec preference, playback, YouTube (reported only),
#   5. the built-in update mechanism (tools/test_updates.py), last, because
#      it upgrades the installed package.
#
#   tools/test-package.sh <deb> <results-dir>
#
# Needs sudo. Used by the development and release workflows.
set -euo pipefail

deb=$(realpath "$1")
out=$(realpath -m "$2")
here=$(dirname "$(realpath "$0")")
mkdir -p "$out"

echo "::group::Install"
sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q "$deb" \
  xvfb imagemagick xdotool x11-utils desktop-file-utils \
  python3-gi gir1.2-gtk-3.0 apt-utils gpg ffmpeg polkitd binutils
dpkg -s hallow | sed -n '1,/^Description/p'
version=$(dpkg-query -W -f='${Version}' hallow)
hallow --version
echo "::endgroup::"

echo "::group::Package contents"
for f in /usr/lib/hallow/hallow-update-helper \
  /usr/share/polkit-1/actions/io.github.yad_ctrlz.Hallow.update.policy \
  /usr/lib/hallow/is-packaged-app /usr/share/icons/hicolor/scalable/apps/hallow.svg; do
  test -e "$f" || { echo "::error::missing $f"; exit 1; }
done
if [ -e /usr/lib/hallow/updater ]; then
  echo "::error::the package must not ship Gecko's MAR updater"
  exit 1
fi
if [ -e /etc/apt/sources.list.d/hallow.sources ]; then
  cat /etc/apt/sources.list.d/hallow.sources
  test -s /usr/share/keyrings/hallow-archive-keyring.asc
else
  echo "::warning::package has no update channel (no archive key committed)"
fi
# Hardware video decoding: Gecko's GPU probe (gfxtest) and the VA-API
# decoders of its bundled FFmpeg (H.264 goes through the system's libavcodec).
test -x /usr/lib/hallow/gfxtest
for codec in av1 vp9 vp8; do
  strings /usr/lib/hallow/libmozavcodec.so | grep -qx "${codec}_vaapi" ||
    { echo "::error::bundled FFmpeg lacks the ${codec} VA-API decoder"; exit 1; }
done
desktop-file-validate /usr/share/applications/hallow.desktop
grep -q '^Icon=hallow$' /usr/share/applications/hallow.desktop
grep -q '^StartupWMClass=hallow$' /usr/share/applications/hallow.desktop
echo "::endgroup::"

export DISPLAY=:99
Xvfb :99 -screen 0 1280x800x24 -nolisten tcp &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null || true' EXIT
sleep 2

echo "::group::Icons"
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
    print(f"{'ok ' if ok else 'BAD'} {size:3}px -> {path}  artwork {w}x{h} ({fill:.0%})")
for size in (40, 56, 80):
    info = theme.lookup_icon("hallow", size, 0)
    print(f"    {size:3}px -> {info.get_filename() if info else None}")
sys.exit(1 if bad else 0)
EOF
echo "::endgroup::"

echo "::group::Window identity"
profile=$(mktemp -d)
hallow --new-instance --profile "$profile" about:blank >/dev/null 2>&1 &
pid=$!
for _ in $(seq 60); do
  win=$(xdotool search --classname Navigator 2>/dev/null | tail -1 || true)
  [ -n "$win" ] && break
  sleep 1
done
xprop -id "$win" WM_CLASS
xprop -id "$win" WM_CLASS | grep -q '"Navigator", "hallow"'
icons=$(xprop -id "$win" -notype -len 99999999 _NET_WM_ICON | grep -o 'Icon ([0-9]* x [0-9]*)' | sort -u | tr '\n' ' ')
echo "window icons: $icons"
for s in 16 32 48 64 128; do
  echo "$icons" | grep -q "($s x $s)" || { echo "::error::no ${s}px window icon"; exit 1; }
done
kill "$pid"; wait "$pid" 2>/dev/null || true
rm -rf "$profile"
echo "::endgroup::"

echo "::group::Screenshots"
bash "$here/screenshot.sh" /usr/bin/hallow "$out/screenshots"
echo "::endgroup::"

echo "::group::Browser checks"
python3 "$here/test_browser.py" --out "$out/browser" --youtube
echo "::endgroup::"

echo "::group::Built-in updates"
# CI stands in for the administrator who would type their password in the
# polkit prompt: allow this user the Hallow update action without one.
sudo tee /etc/polkit-1/rules.d/49-hallow-ci.rules >/dev/null <<EOF
polkit.addRule(function (action, subject) {
  if (action.id == "io.github.yad_ctrlz.Hallow.update" &&
      subject.user == "$(id -un)") {
    return polkit.Result.YES;
  }
});
EOF
sudo systemctl restart polkit 2>/dev/null || true
python3 "$here/test_updates.py" --deb "$deb" --out "$out/updates"
echo "::endgroup::"

echo "hallow $version passed the package tests"
