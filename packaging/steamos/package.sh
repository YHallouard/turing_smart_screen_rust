#!/bin/sh
# Build the release tarball consumed by install.sh / update.sh.
#   packaging/steamos/package.sh <version-tag> [out-dir]
# Expects target/release/bc250-dashboard to be built (make release).
# Output: <out-dir>/bc250-dashboard-x86_64-linux.tar.gz (+ .sha256) and the
# standalone installer assets uploaded next to it.
set -eu

TAG="$1"
OUT="${2:-dist}"
ASSET="bc250-dashboard-x86_64-linux.tar.gz"
stage="target/steamos-pkg"
pkg="$stage/bc250-dashboard"

rm -rf "$stage"
mkdir -p "$pkg/bin" "$pkg/steamos" "$OUT"
install -m755 target/release/bc250-dashboard "$pkg/bin/"
cp -r assets "$pkg/"
cp config.toml "$pkg/"
echo "$TAG" > "$pkg/VERSION"
cp packaging/bc250-dashboard.service packaging/70-turzx-panel.rules \
   packaging/steamos/bc250-update.service packaging/steamos/bc250-update.timer \
   packaging/steamos/update.sh "$pkg/steamos/"

tar -czf "$OUT/$ASSET" -C "$stage" bc250-dashboard
(cd "$OUT" && sha256sum "$ASSET" > "$ASSET.sha256")
cp packaging/steamos/install.sh packaging/steamos/install-bc250.desktop "$OUT/"
cp packaging/steamos/update.sh "$OUT/bc250-dashboard-update.sh"
echo ">> $OUT/$ASSET ($TAG)"
