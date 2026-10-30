#!/usr/bin/env bash
# Build the release binaries for this machine and zip them with the quick
# start guide: dist/mapTemplateGenerator-<version>-<platform>.zip
#
#   scripts/package.sh                 # platform name defaults to linux-x86_64
#   scripts/package.sh linux-x86_64
#
# The GitHub release workflow (.github/workflows/release.yml) runs this on
# Linux; Windows is packaged there with PowerShell.
set -euo pipefail
cd "$(dirname "$0")/.."

platform="${1:-linux-x86_64}"
version="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
name="mapTemplateGenerator-${version}-${platform}"

cargo build --release --bins

rm -rf "dist/${name}" "dist/${name}.zip"
mkdir -p "dist/${name}/configs"
cp target/release/mapTemplateGenerator target/release/mapgen-mcp "dist/${name}/"
cp packaging/QUICKSTART.md "dist/${name}/"
(cd dist && zip -qr "${name}.zip" "${name}")
echo "dist/${name}.zip"
