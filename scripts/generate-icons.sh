#!/bin/bash
#
# Render src-tauri/icons/{32x32,128x128,128x128@2x}.png from
# assets/eMterm.svg. The PNGs are consumed by scripts/build-dpkg.sh as
# hicolor icon set entries. Falls back to a no-op when neither
# rsvg-convert nor magick is installed (matches the legacy behaviour:
# postinstall must not block apt installs that lack image tooling).
set -euo pipefail

ICON_SVG="assets/eMterm.svg"
ICONS_DIR="src-tauri/icons"

mkdir -p "$ICONS_DIR"

if command -v rsvg-convert &> /dev/null; then
    CONVERT="rsvg"
elif command -v magick &> /dev/null; then
    CONVERT="magick"
else
    echo "Warning: No SVG converter found (rsvg-convert or magick). Skipping icon generation."
    exit 0
fi

render_png() {
    local size="$1"
    local out="$2"
    if [ "$CONVERT" = "rsvg" ]; then
        rsvg-convert -w "$size" -h "$size" "$ICON_SVG" -o "$out"
    else
        magick -background none "$ICON_SVG" -resize "${size}x${size}" "$out"
    fi
}

render_png 32 "$ICONS_DIR/32x32.png"
render_png 128 "$ICONS_DIR/128x128.png"
render_png 256 "$ICONS_DIR/128x128@2x.png"

echo "Generated icons in $ICONS_DIR"
