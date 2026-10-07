#!/usr/bin/env bash
# Builds .deb, .rpm, .AppImage and .tar.gz from an already-built tree.
# Usage (from the repo root): packaging/linux/package.sh <version> <x86_64|aarch64>
# Expects: app/build/linux/<x64|arm64>/release/bundle/ (flutter build linux --release)
#          target/release/{leemusync,leemusyncd} (cargo build --release)
# Needs nfpm and appimagetool on PATH. Output: dist/out/. Layout: docs/specs/release.md.
set -euo pipefail
version="$1"; arch="$2"
case "$arch" in
  x86_64) flutter_arch=x64; nfpm_arch=amd64 ;;
  aarch64) flutter_arch=arm64; nfpm_arch=arm64 ;;
  *) echo "unknown arch: $arch" >&2; exit 1 ;;
esac
desktop=packaging/linux/io.github.danielesc0911.leemusync.desktop
icon=packaging/icons/leemusync.png
base="leemusync-${version}-linux-${arch}"

rm -rf dist/stage dist/AppDir dist/tar
mkdir -p dist/stage/gui dist/stage/bin dist/out
cp -r "app/build/linux/${flutter_arch}/release/bundle/." dist/stage/gui/
cp target/release/leemusync target/release/leemusyncd dist/stage/bin/

# .deb and .rpm (nfpm reads LEEMUSYNC_VERSION and NFPM_ARCH from the environment)
export LEEMUSYNC_VERSION="$version" NFPM_ARCH="$nfpm_arch"
nfpm package --config packaging/linux/nfpm.yaml --packager deb --target "dist/out/${base}.deb"
nfpm package --config packaging/linux/nfpm.yaml --packager rpm --target "dist/out/${base}.rpm"

# .AppImage: AppRun -> leemusync-gui; CLI + daemon in usr/bin/
mkdir -p dist/AppDir/usr/bin
cp -r dist/stage/gui/. dist/AppDir/
cp dist/stage/bin/leemusync dist/stage/bin/leemusyncd dist/AppDir/usr/bin/
cp "$desktop" dist/AppDir/
cp "$icon" dist/AppDir/io.github.danielesc0911.leemusync.png
cat > dist/AppDir/AppRun <<'APPRUN'
#!/bin/sh
APPDIR="${APPDIR:-$(dirname "$(readlink -f "$0")")}"
exec "$APPDIR/leemusync-gui" "$@"
APPRUN
chmod +x dist/AppDir/AppRun
ARCH="$arch" appimagetool --no-appstream dist/AppDir "dist/out/${base}.AppImage"

# .tar.gz: leemusync/leemusync-gui (+ bundle) and leemusync/bin/
mkdir -p dist/tar/leemusync
cp -r dist/stage/gui/. dist/tar/leemusync/
cp -r dist/stage/bin dist/tar/leemusync/bin
tar -C dist/tar -czf "dist/out/${base}.tar.gz" leemusync
