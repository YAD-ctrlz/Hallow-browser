#!/usr/bin/env bash
# Make the update packages tools/test_windows_updates.py serves for a
# Windows build. With the build's signing key (development builds: a
# throwaway key made for the CI run), also sign the build's own update
# package with it:
#
#   next.mar           a later build (the same files plus update-test-marker.txt),
#                      signed with the build's key: must install
#   rogue.mar          the same, signed with another key: must be refused
#   unsigned.mar       the same, without signature: must be refused
#   tampered.mar       next.mar with one byte changed after signing: refused
#   wrong-channel.mar  signed, but for Firefox's MAR channel: refused
#   downgrade.mar      signed, but for an older Firefox version: refused
#
# Without a key (release builds, whose key only the production environment
# holds), only the packages that must be refused are made: rogue.mar and
# unsigned.mar. The private key never leaves the build job: only signed
# packages do.
#
#   tools/make-windows-update-tests.sh <dist dir> <out dir> [<key.pem> <cert.der>]
set -euo pipefail

dist=$1 out=$2 key=${3:-} cert=${4:-}
root=$(cd "$(dirname "$0")/.." && pwd)
hb() { (cd "$root" && cargo hb "$@"); }

src="${HALLOW_WORK_DIR:-$root/work}/firefox-$(hb version --firefox)"
mar=$(ls "$dist"/hallow-*-win64.complete.mar)
zip=$(ls "$dist"/hallow-*-win64.zip)
info=$(ls "$dist"/hallow-*-win64.json)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"

# A later build: the same files and a marker telling it apart.
unzip -q "$zip" -d "$work/next"
echo next > "$work/next/hallow/update-test-marker.txt"
(cd "$work/next" && zip -q -r -X "$work/next.zip" hallow)
MOZCONFIG="$src/mozconfig" MOZBUILD_SKIP_INTERACTIVE=1 "$src/mach" repackage mar \
  --input "$work/next.zip" --mar "$src/obj-hallow/dist/host/bin/mar" \
  --output "$work/next.mar" --arch x86_64 --mar-channel-id hallow-release

openssl req -x509 -newkey rsa:4096 -nodes -days 1 \
  -subj "/CN=Hallow update test (rogue key)" \
  -keyout "$work/rogue.key" -outform DER -out "$work/rogue.der" 2>/dev/null
hb mar sign "$work/next.mar" "$out/rogue.mar" --key "$work/rogue.key" --cert "$work/rogue.der"
cp "$work/next.mar" "$out/unsigned.mar"
cp "$info" "$out/build.json"
for bad in rogue unsigned; do
  if [ -n "$cert" ] && hb mar verify "$out/$bad.mar" --cert "$cert" 2>/dev/null; then
    echo "$bad.mar verifies, but must not" >&2
    exit 1
  fi
done
if [ -z "$key" ]; then
  ls -l "$out"
  exit 0
fi

# The build's own update package, and the packages only its key can make.
hb mar sign "$mar" "$mar" --key "$key" --cert "$cert"
hb mar sign "$work/next.mar" "$out/next.mar" --key "$key" --cert "$cert"
cp "$out/next.mar" "$out/tampered.mar"
python3 - "$out/tampered.mar" <<'EOF'
import sys
path = sys.argv[1]
data = bytearray(open(path, "rb").read())
data[len(data) // 2] ^= 0x01  # inside a packaged file
open(path, "wb").write(data)
EOF
hb mar sign "$work/next.mar" "$out/wrong-channel.mar" --key "$key" --cert "$cert" \
  --channel firefox-mozilla-release
hb mar sign "$work/next.mar" "$out/downgrade.mar" --key "$key" --cert "$cert" \
  --product-version 156.0

# Check the packages are what their names say before a Windows runner
# spends time on them.
hb mar verify "$out/next.mar" --cert "$cert"
hb mar verify "$mar" --cert "$cert"
if hb mar verify "$out/tampered.mar" --cert "$cert" 2>/dev/null; then
  echo "tampered.mar verifies, but must not" >&2
  exit 1
fi
hb mar info "$out/wrong-channel.mar" | grep -qx "channel: firefox-mozilla-release"
hb mar info "$out/downgrade.mar" | grep -qx "version: 156.0"
ls -l "$out"
