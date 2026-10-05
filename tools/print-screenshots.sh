#!/usr/bin/env bash
# Print the PNG screenshots under a directory as base64 JPEGs in collapsed
# log groups, so they can be looked at from the job log alone.
#
#   tools/print-screenshots.sh <dir>
set -uo pipefail

find "$1" -name '*.png' -print0 | sort -z | while IFS= read -r -d '' png; do
  jpg=${png%.png}.jpg
  convert "$png" -resize 900x -quality 55 "$jpg" || continue
  echo "::group::${png#"$1"/} (base64 jpeg)"
  base64 "$jpg"
  echo "::endgroup::"
done
