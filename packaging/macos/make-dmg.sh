#!/usr/bin/env bash
# Usage: make-dmg.sh <path/to/LeemuSync.app> <dir with leemusync + leemusyncd> <output.dmg>
set -euo pipefail
app="$1"; helpers="$2"; out="$3"
mkdir -p "$app/Contents/Helpers"
cp "$helpers/leemusync" "$helpers/leemusyncd" "$app/Contents/Helpers/"
codesign --force --deep --sign - "$app"   # ad-hoc signature (D23); Developer ID later
stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname LeemuSync -srcfolder "$stage" -ov -format UDZO "$out"
