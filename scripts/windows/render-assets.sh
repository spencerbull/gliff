#!/usr/bin/env bash
# Deterministic SVG rasterization; librsvg is needed only when regenerating.
set -euo pipefail
cd "$(dirname "$0")/../.."
mkdir -p packaging/windows/Assets
for spec in StoreLogo:50 Square44x44Logo:44 Square150x150Logo:150; do
    name=${spec%:*}
    size=${spec#*:}
    rsvg-convert --width "$size" --height "$size" pkgbuild/gliff.svg \
        --output "packaging/windows/Assets/$name.png"
done
