#!/bin/sh
# Build the Decky Loader plugin zip (sideload: Decky > Settings > Developer >
# "Install plugin from ZIP").
#   decky-plugin/package.sh [version-tag] [out-dir]
set -eu

TAG="${1:-dev}"
OUT="${2:-dist-release}"
here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
out=$(mkdir -p "$OUT" && cd "$OUT" && pwd)
name="BC-250 Dashboard"

cd "$here"
npm ci --no-audit --no-fund
npm run build

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name/dist"
cp dist/index.js "$stage/$name/dist/"
cp main.py plugin.json package.json "$stage/$name/"
cp "$root/LICENSE"* "$stage/$name/" 2>/dev/null || true
echo "$TAG" > "$stage/$name/VERSION"

rm -f "$out/bc250-dashboard-decky.zip"
(cd "$stage" && python3 -m zipfile -c "$out/bc250-dashboard-decky.zip" "$name")
echo ">> $out/bc250-dashboard-decky.zip ($TAG)"
