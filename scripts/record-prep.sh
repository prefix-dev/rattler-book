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

# Seed the input image for lumen.thumbnail. Use ImageMagick's built-in
# `logo:` sample (colorful wizard + text) so the chafa rendering at the
# end of the cast is visibly a scaled-down version of a real image,
# not a smooth gradient that reads as a solid block.
magick logo: -resize 512x512 "$ROOT/work/photo.jpg"

# Margin backgrounds for VHS: solid warm beige + a pre-baked drop shadow
# positioned where each tape's window will land. The window (opaque)
# covers the rect interior at render time; only the blurred halo shows.
# Layout parameters must match the tape: Width=1200, Margin=40,
# WindowBarSize=40. Shadow offset +6 right, +10 down.
#
# install-and-run.tape: Height=900 → canvas 1280 x 1020, window 1200x940 at (40,40)
magick -size 1280x1020 xc:'#ebe4d3' \
    \( -size 1280x1020 xc:none \
       -fill '#00000055' -draw 'roundrectangle 46,50 1246,990 12,12' \
       -channel A -blur 0x18 +channel \) \
    -compose Over -composite \
    "$REPO/casts/bg-install.png"
#
# build.tape: Height=560 → canvas 1280 x 680, window 1200x600 at (40,40)
magick -size 1280x680 xc:'#ebe4d3' \
    \( -size 1280x680 xc:none \
       -fill '#00000055' -draw 'roundrectangle 46,50 1246,650 12,12' \
       -channel A -blur 0x18 +channel \) \
    -compose Over -composite \
    "$REPO/casts/bg-build.png"

# Fish config used by both tapes (via XDG_CONFIG_HOME).
mkdir -p "$ROOT/fish-config/fish"
cp "$REPO/scripts/fish-config.fish" "$ROOT/fish-config/fish/config.fish"

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
