#!/usr/bin/env bash
# Derive every icon under assets/app-icon/ from the pinned design-system sources in assets/brand/
# and nothing else. Needs ImageMagick 7
# (`magick`) and cargo. Runs on any OS; the outputs are committed,
# so builds and packaging never need these tools.
#
#   packaging/brand-assets.sh
#
# What is derived, and from what (assets/brand/README.md has the rules):
#   app-icon/hicolor/{16,32,48,64}       the design system's per-size favicons, as is
#   app-icon/hicolor/512                 the 512 tile, as is
#   app-icon/hicolor/{24,128,256}        the next larger tile resized DOWN (32, 192, 512), never up
#   app-icon/hicolor/scalable            favicon.svg, as is
#   app-icon/redrob-board.ico         16, 32, 48, 64 as is + 256 resized down from 512
#   app-icon/redrob-board.icns        16, 32, 64, 128, 256, 512
#   app-icon/redrob-logo-{light,dark}.png  the lockups resized down to 64 px tall (header)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
B="$ROOT/assets/brand"
A="$ROOT/assets/app-icon"
ID="ai.redrob.board"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
command -v magick >/dev/null || { echo "error: magick (ImageMagick 7) not found" >&2; exit 1; }

(cd "$B" && sha256sum -c --quiet SHA256SUMS)

down() { magick "$1" -filter Lanczos -resize "$2x$2" "PNG32:$3"; }

rm -rf "$A"
mkdir -p "$A"
for s in 16 24 32 48 64 128 256 512; do mkdir -p "$A/hicolor/${s}x${s}/apps"; done
mkdir -p "$A/hicolor/scalable/apps"
for s in 16 32 48 64; do cp "$B/favicon-${s}x${s}.png" "$A/hicolor/${s}x${s}/apps/$ID.png"; done
cp "$B/icon-512.png" "$A/hicolor/512x512/apps/$ID.png"
down "$B/favicon-32x32.png" 24 "$A/hicolor/24x24/apps/$ID.png"
down "$B/icon-192.png" 128 "$A/hicolor/128x128/apps/$ID.png"
down "$B/icon-512.png" 256 "$A/hicolor/256x256/apps/$ID.png"
cp "$B/favicon.svg" "$A/hicolor/scalable/apps/$ID.svg"

(cd "$ROOT" && cargo run -q -p xtask -- ico "$A/redrob-board.ico" \
  "$B/favicon-16x16.png" "$B/favicon-32x32.png" "$B/favicon-48x48.png" "$B/favicon-64x64.png" \
  "$A/hicolor/256x256/apps/$ID.png")
(cd "$ROOT" && cargo run -q -p xtask -- icns "$A/redrob-board.icns" \
  "$B/favicon-16x16.png" "$B/favicon-32x32.png" "$B/favicon-64x64.png" \
  "$A/hicolor/128x128/apps/$ID.png" "$A/hicolor/256x256/apps/$ID.png" "$B/icon-512.png")

# The header lockup, by ground. 64 px tall covers an 18 pt header at up to 3.5x.
magick "$B/redrob-logo-primary.png" -filter Lanczos -resize x64 "PNG32:$A/redrob-logo-light.png"
magick "$B/redrob-logo-primary-white.png" -filter Lanczos -resize x64 "PNG32:$A/redrob-logo-dark.png"

echo "brand assets written to $A"
