#!/usr/bin/env bash
set -euo pipefail

# Prep state for both VHS recordings.
#
#   /tmp/shot-demo/
#     channel/        ← lumen package (populated via `shot build`)
#     work/           ← cwd for install-and-run.tape
#       photo.jpg     (seed image for lumen.thumbnail)
#     lumen-build/    ← fresh copy of examples/lumen for build.tape
#     build-channel/  ← cold output dir for build.tape
#
# Also primes the rattler package cache by running a throwaway
# `shot install` once, so the in-tape install hits warm-cache speeds.
#
# Prerequisite: `target/release/shot` already built (build-release is a
# depends-on of the pixi task). We use the global `shot` on PATH for the
# recording itself (user has it installed at ~/.pixi/bin/shot), but prep
# uses the local debug/release binary so a fresh clone works.

REPO="$PWD"
SHOT="$REPO/target/release/shot"
ROOT=/tmp/shot-demo

if [[ ! -x "$SHOT" ]]; then
    echo "error: $SHOT not found or not executable" >&2
    echo "hint: run \`pixi run build-release\` first" >&2
    exit 1
fi

rm -rf "$ROOT"
mkdir -p "$ROOT/channel" "$ROOT/work"

# Build the lumen package into ../channel.
cp -r "$REPO/examples/lumen" "$ROOT/lumen-src"
(cd "$ROOT/lumen-src" && "$SHOT" build --output-dir ../channel)

# Seed the input image for lumen.thumbnail.
magick -size 512x512 gradient:blue-orange "$ROOT/work/photo.jpg"

# Prime rattler's package cache with lumen + its transitive deps so the
# tape's `shot install` completes in ~1 s instead of 15-30 s.
mkdir -p "$ROOT/warmup"
(
    cd "$ROOT/warmup"
    "$SHOT" init warmup --channel ../channel --channel conda-forge >/dev/null
    "$SHOT" add lumen >/dev/null
    "$SHOT" install >/dev/null
)
rm -rf "$ROOT/warmup"

# Fresh build dir for build.tape (no sharing with the prep build).
rm -rf "$ROOT/lumen-build" "$ROOT/build-channel"
cp -r "$REPO/examples/lumen" "$ROOT/lumen-build"
mkdir -p "$ROOT/build-channel"

echo "record-prep: ready at $ROOT"
