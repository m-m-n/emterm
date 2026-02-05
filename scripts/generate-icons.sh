#!/bin/bash
set -euo pipefail

ICON_SVG="assets/eMterm.svg"
ICON_PNG="src-tauri/icons/_tmp_1024.png"

if command -v rsvg-convert &> /dev/null; then
    rsvg-convert -w 1024 -h 1024 "$ICON_SVG" -o "$ICON_PNG"
elif command -v magick &> /dev/null; then
    magick "$ICON_SVG" -resize 1024x1024 "$ICON_PNG"
else
    echo "Warning: No SVG converter found (rsvg-convert or magick). Skipping icon generation."
    exit 0
fi

bun tauri icon "$ICON_PNG"
rm -f "$ICON_PNG"
