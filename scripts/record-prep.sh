#!/usr/bin/env bash
set -euo pipefail

# Build the `lumen` package into /tmp/shot-demo/channel/ and set up a clean
# working directory at /tmp/shot-demo/work/ for recording.
#
# Prerequisite: `target/release/shot` already built (task has depends-on = build-release).

REPO="$PWD"
SHOT="$REPO/target/release/shot"
ROOT=/tmp/shot-demo

if [[ ! -x "$SHOT" ]]; then
    echo "error: $SHOT not found or not executable" >&2
    echo "hint: run \`pixi run build-release\` first" >&2
    exit 1
fi

rm -rf "$ROOT"
mkdir -p "$ROOT/channel"

cp -r "$REPO/examples/lumen" "$ROOT/lumen-src"
(cd "$ROOT/lumen-src" && "$SHOT" build --output-dir ../channel)

mkdir -p "$ROOT/work"

# Seed a demo photo.jpg so `shot run lua -e "require('lumen').thumbnail(...)"`
# has an input to operate on. magick is in the recording pixi env.
magick -size 512x512 gradient:blue-orange "$ROOT/work/photo.jpg"

echo "record-prep: lumen channel ready at $ROOT/channel"
