#!/usr/bin/env bash
# Usage: make-dmg.sh <path/to/LemuSync.app> <dir with lemusync + lemusyncd> <output.dmg>
set -euo pipefail
app="$1"; helpers="$2"; out="$3"
mkdir -p "$app/Contents/Helpers"
cp "$helpers/lemusync" "$helpers/lemusyncd" "$app/Contents/Helpers/"
codesign --force --deep --sign - "$app"   # ad-hoc signature (D23); Developer ID later
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname LemuSync -srcfolder "$stage" -ov -format UDZO "$out"
