#!/bin/sh
# Regenerate packages/desktop/assets/icon.icns from assets/icon.png
# (checkpoint 11 §E). Requires Pillow + iconutil (macOS).
# Run from the repo root.
set -e
dir=$(mktemp -d)/icon.iconset
mkdir -p "$dir"
for s in 16 32 64 128 256 512 1024; do
  python3 - "$s" "$dir" <<'PY'
import sys
from PIL import Image
s = int(sys.argv[1]); d = sys.argv[2]
src = Image.open("packages/desktop/assets/icon.png").convert("RGBA")
src.resize((s, s), Image.LANCZOS).save(f"{d}/icon_{s}x{s}.png")
if s <= 512:
    src.resize((s*2, s*2), Image.LANCZOS).save(f"{d}/icon_{s}x{s}@2x.png")
PY
done
iconutil -c icns "$dir" -o packages/desktop/assets/icon.icns
echo "wrote packages/desktop/assets/icon.icns"
